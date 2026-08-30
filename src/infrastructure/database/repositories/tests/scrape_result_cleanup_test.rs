use super::*;
use crate::common::test_helpers::create_test_db_pool;
use crate::domain::retention_policy::RetentionBatchPolicy;
use chrono::Duration;

/// 序列化清理测试：共享 testcontainers 容器下，两个清理用例并行时
/// 会互相删除对方刚插入的数据（retention 覆盖全部行）。
static RETENTION_TEST_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();

fn retention_lock() -> &'static tokio::sync::Mutex<()> {
    RETENTION_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Build a ScrapeResult whose created_at is `days_ago` days before now.
/// Fresh UUIDs per call so tests are isolated.
fn sample_result_with_age(days_ago: i64) -> ScrapeResult {
    ScrapeResult {
        id: Uuid::new_v4(),
        task_id: Uuid::new_v4(),
        url: "https://example.com/retention".to_string(),
        status_code: 200,
        content: "<html>retention test</html>".to_string(),
        content_type: "text/html".to_string(),
        headers: serde_json::json!({}),
        meta_data: serde_json::json!({}),
        screenshot: None,
        response_time_ms: 100,
        created_at: chrono::Utc::now() - Duration::days(days_ago),
    }
}

/// R-retention-002：`cleanup_expired(30)` 删除 created_at 超过 30 天的行，
/// 保留新行，返回值为删除行数。
///
/// 注意：testcontainers 容器在进程内由 OnceLock 共享（test_helpers），
/// 其他测试可能已插入历史数据，因此返回值断言用 `>= 1`（本测试插入的
/// 40 天前行必然在删除范围内）；精确的"删旧留新"行为由 find_by_task_id 断言。
#[tokio::test]
async fn cleanup_expired_deletes_old_keeps_recent() {
    let _guard = retention_lock().lock().await;
    let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());

    let old = sample_result_with_age(40);
    let recent = sample_result_with_age(1);
    repo.save(old.clone()).await.expect("save old failed");
    repo.save(recent.clone()).await.expect("save recent failed");

    let deleted = repo
        .cleanup_expired(30, &RetentionBatchPolicy::default())
        .await
        .expect("cleanup_expired failed");
    assert!(
        deleted >= 1,
        "deleted count should include our old row: {deleted}"
    );

    // Exact behavior: our 40-day-old row is gone, our 1-day-old row survives.
    assert!(
        repo.find_by_task_id(old.task_id)
            .await
            .expect("query failed")
            .is_none(),
        "40-day-old row should be deleted"
    );
    let found = repo
        .find_by_task_id(recent.task_id)
        .await
        .expect("query failed");
    assert!(found.is_some(), "1-day-old row should be kept");
}

/// R-retention-002：retention_days=0 时所有历史行都过期；新插入行立即清理不报错。
#[tokio::test]
async fn cleanup_expired_with_zero_retention_removes_fresh_rows() {
    let _guard = retention_lock().lock().await;
    let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
    let fresh = sample_result_with_age(0);
    repo.save(fresh.clone()).await.expect("save failed");

    let deleted = repo
        .cleanup_expired(0, &RetentionBatchPolicy::default())
        .await
        .expect("cleanup_expired failed");
    assert!(
        deleted >= 1,
        "fresh row should be removable with 0 retention: {deleted}"
    );
    assert!(
        repo.find_by_task_id(fresh.task_id)
            .await
            .expect("query failed")
            .is_none(),
        "row should be deleted with 0-day retention"
    );
}

/// R-retention-002：过期行数 > batch_size 时循环分批删净，返回总数 = 过期行数，新行保留。
#[tokio::test]
async fn cleanup_expired_batches_until_drained() {
    let _guard = retention_lock().lock().await;
    let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());

    let mut old_tasks = Vec::new();
    for _ in 0..5 {
        let r = sample_result_with_age(40);
        old_tasks.push(r.task_id);
        repo.save(r).await.expect("save old failed");
    }
    let recent = sample_result_with_age(1);
    repo.save(recent.clone()).await.expect("save recent failed");

    let policy = RetentionBatchPolicy {
        batch_size: 2,
        max_rows_per_cycle: 100_000,
        statement_timeout_ms: 60_000,
    };
    let deleted = repo
        .cleanup_expired(30, &policy)
        .await
        .expect("cleanup_expired failed");
    assert_eq!(deleted, 5, "all 5 old rows must be deleted across batches");

    for task_id in old_tasks {
        assert!(
            repo.find_by_task_id(task_id)
                .await
                .expect("query failed")
                .is_none(),
            "old row should be deleted"
        );
    }
    assert!(
        repo.find_by_task_id(recent.task_id)
            .await
            .expect("query failed")
            .is_some(),
        "recent row must survive"
    );
}

/// R-retention-002：删除总数达到 `max_rows_per_cycle` 即封顶停止，超出部分留给下一周期。
#[tokio::test]
async fn cleanup_expired_caps_at_max_rows_per_cycle() {
    let _guard = retention_lock().lock().await;
    let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());

    for _ in 0..5 {
        repo.save(sample_result_with_age(40))
            .await
            .expect("save old failed");
    }

    let policy = RetentionBatchPolicy {
        batch_size: 2,
        max_rows_per_cycle: 3,
        statement_timeout_ms: 60_000,
    };
    let deleted = repo
        .cleanup_expired(30, &policy)
        .await
        .expect("cleanup_expired failed");
    assert_eq!(
        deleted, 3,
        "deletion must stop once max_rows_per_cycle is reached"
    );
}
