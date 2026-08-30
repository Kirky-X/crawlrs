use super::*;
use crate::common::test_helpers::create_test_db_pool;
use crate::domain::models::webhook_model::{WebhookEventType, WebhookStatus};
use crate::domain::retention_policy::RetentionBatchPolicy;
use chrono::{Duration, Utc};
use uuid::Uuid;

/// 序列化清理测试：共享 testcontainers 容器下，多个清理用例并行时会
/// 互相删除对方刚插入的数据（retention 覆盖全部行）。
static WEBHOOK_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

fn webhook_lock() -> &'static tokio::sync::Mutex<()> {
    WEBHOOK_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 构建指定状态与年龄的 WebhookEvent（updated_at 为 `updated_days_ago` 天前；
/// delivered_at 为 `delivered_days_ago` 天前或 None）。
fn make_event(
    status: WebhookStatus,
    updated_days_ago: i64,
    delivered_days_ago: Option<i64>,
) -> WebhookEvent {
    // find_pending 的重试集要求 next_retry_at < now，failed 事件须带已过期的重试时间
    let next_retry_at = match status {
        WebhookStatus::Failed => Some(Utc::now() - Duration::hours(1)),
        _ => None,
    };
    WebhookEvent::with_all_fields(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        WebhookEventType::CrawlCompleted,
        serde_json::json!({"crawl_id": "c-1"}),
        "https://example.com/hook".to_string(),
        status,
        0,
        3,
        None,
        None,
        None,
        next_retry_at,
        Utc::now() - Duration::days(updated_days_ago),
        Utc::now() - Duration::days(updated_days_ago),
        delivered_days_ago.map(|d| Utc::now() - Duration::days(d)),
    )
}

/// R-retention-004：`cleanup_terminal(30)` 清理 30 天前的 delivered/dead 终态事件，
/// 保留 pending/failed 活事件。
#[tokio::test]
async fn cleanup_terminal_removes_old_terminal_keeps_active() {
    let _guard = webhook_lock().lock().await;
    let repo = WebhookEventRepoImpl::new(create_test_db_pool());

    let delivered = make_event(WebhookStatus::Delivered, 35, Some(35));
    let dead = make_event(WebhookStatus::Dead, 35, None);
    let pending = make_event(WebhookStatus::Pending, 1, None);
    let failed = make_event(WebhookStatus::Failed, 1, None);
    for e in [&delivered, &dead, &pending, &failed] {
        repo.create(e).await.expect("create failed");
    }

    let deleted = repo
        .cleanup_terminal(30, &RetentionBatchPolicy::default())
        .await
        .expect("cleanup_terminal failed");
    assert!(
        deleted >= 2,
        "deleted count should include our old terminal rows: {deleted}"
    );

    assert!(
        repo.find_by_id(delivered.id)
            .await
            .expect("query failed")
            .is_none(),
        "35-day-old delivered event should be deleted"
    );
    assert!(
        repo.find_by_id(dead.id)
            .await
            .expect("query failed")
            .is_none(),
        "35-day-old dead event should be deleted"
    );
    assert!(
        repo.find_by_id(pending.id)
            .await
            .expect("query failed")
            .is_some(),
        "pending event should be kept"
    );
    assert!(
        repo.find_by_id(failed.id)
            .await
            .expect("query failed")
            .is_some(),
        "failed event should be kept"
    );

    // 活事件不仅要"没被删"，还要仍可被 worker 经 find_pending 捞取投递
    let fetchable_ids: Vec<Uuid> = repo
        .find_pending(10_000)
        .await
        .expect("find_pending failed")
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert!(
        fetchable_ids.contains(&pending.id),
        "pending event must remain fetchable via find_pending after cleanup"
    );
    assert!(
        fetchable_ids.contains(&failed.id),
        "failed event with due next_retry_at must remain fetchable via find_pending after cleanup"
    );
}

/// R-retention-004：新的 delivered 事件（1 天前投递）不因 30 天保留期被删。
#[tokio::test]
async fn cleanup_terminal_keeps_recent_delivered() {
    let _guard = webhook_lock().lock().await;
    let repo = WebhookEventRepoImpl::new(create_test_db_pool());

    let recent_delivered = make_event(WebhookStatus::Delivered, 1, Some(1));
    repo.create(&recent_delivered).await.expect("create failed");

    repo.cleanup_terminal(30, &RetentionBatchPolicy::default())
        .await
        .expect("cleanup failed");

    assert!(
        repo.find_by_id(recent_delivered.id)
            .await
            .expect("query failed")
            .is_some(),
        "1-day-old delivered event should be kept"
    );
}

/// R-retention-004：delivered/dead 过期行数 > batch_size 时循环分批删净，活事件不受影响。
#[tokio::test]
async fn cleanup_terminal_batches_until_drained() {
    let _guard = webhook_lock().lock().await;
    let repo = WebhookEventRepoImpl::new(create_test_db_pool());

    let mut old_terminal_ids = Vec::new();
    for _ in 0..3 {
        let d = make_event(WebhookStatus::Delivered, 35, Some(35));
        old_terminal_ids.push(d.id);
        repo.create(&d).await.expect("create failed");
    }
    for _ in 0..3 {
        let dead = make_event(WebhookStatus::Dead, 35, None);
        old_terminal_ids.push(dead.id);
        repo.create(&dead).await.expect("create failed");
    }
    let pending = make_event(WebhookStatus::Pending, 1, None);
    repo.create(&pending).await.expect("create failed");

    let policy = RetentionBatchPolicy {
        batch_size: 2,
        max_rows_per_cycle: 100_000,
        statement_timeout_ms: 60_000,
    };
    let deleted = repo
        .cleanup_terminal(30, &policy)
        .await
        .expect("cleanup_terminal failed");
    assert!(
        deleted >= 6,
        "our 6 old terminal rows must be among the deleted: {deleted}"
    );
    for id in old_terminal_ids {
        assert!(
            repo.find_by_id(id).await.expect("query failed").is_none(),
            "old terminal event should be deleted"
        );
    }
    assert!(
        repo.find_by_id(pending.id)
            .await
            .expect("query failed")
            .is_some(),
        "pending event must survive batched cleanup"
    );
}

/// R-retention-004：删除总数达到 `max_rows_per_cycle` 即封顶停止。
#[tokio::test]
async fn cleanup_terminal_caps_at_max_rows_per_cycle() {
    let _guard = webhook_lock().lock().await;
    let repo = WebhookEventRepoImpl::new(create_test_db_pool());

    // 清掉共享容器中的历史过期终态行，保证封顶断言精确
    repo.cleanup_terminal(30, &RetentionBatchPolicy::default())
        .await
        .expect("pre-clean failed");

    for _ in 0..5 {
        let d = make_event(WebhookStatus::Delivered, 35, Some(35));
        repo.create(&d).await.expect("create failed");
    }

    let policy = RetentionBatchPolicy {
        batch_size: 2,
        max_rows_per_cycle: 3,
        statement_timeout_ms: 60_000,
    };
    let deleted = repo
        .cleanup_terminal(30, &policy)
        .await
        .expect("cleanup_terminal failed");
    assert_eq!(
        deleted, 3,
        "deletion must stop once max_rows_per_cycle is reached"
    );
}
