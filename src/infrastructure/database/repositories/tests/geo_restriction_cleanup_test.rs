use super::*;
use crate::common::test_helpers::create_test_db_pool;
use crate::domain::retention_policy::RetentionBatchPolicy;
use crate::infrastructure::database::entities::geo_restriction_log;
use chrono::Duration;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use uuid::Uuid;

/// 序列化清理测试：共享 testcontainers 容器下，两个清理用例并行时
/// 会互相删除对方刚插入的数据（retention 覆盖全部行）。
static RESTRICTION_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

fn restriction_lock() -> &'static tokio::sync::Mutex<()> {
    RESTRICTION_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 直接插入带自定义 created_at 的日志行（repo 的 log 方法固定 NOW，
/// 无法构造历史行），返回日志 id。
async fn insert_log_with_age(days_ago: i64) -> Uuid {
    let pool = create_test_db_pool();
    let session = pool.get_session("admin").await.expect("get session");
    let conn = session.connection().expect("get connection");
    let id = Uuid::new_v4();
    let model = geo_restriction_log::ActiveModel {
        id: Set(id),
        team_id: Set(Uuid::new_v4()),
        ip_address: Set("192.168.1.1".to_string()),
        country_code: Set(Some("US".to_string())),
        restriction_type: Set("country_block".to_string()),
        url: Set(None),
        reason: Set("retention test".to_string()),
        created_at: Set((chrono::Utc::now() - Duration::days(days_ago)).fixed_offset()),
    };
    model.insert(conn).await.expect("insert log");
    id
}

async fn row_exists(id: Uuid) -> bool {
    let pool = create_test_db_pool();
    let session = pool.get_session("admin").await.expect("session");
    let conn = session.connection().expect("connection");
    geo_restriction_log::Entity::find_by_id(id)
        .one(conn)
        .await
        .expect("query row")
        .is_some()
}

/// R-retention-003：`cleanup_expired(90)` 删除 created_at 超过 90 天的行，
/// 保留新行，返回值为删除行数。
#[tokio::test]
async fn cleanup_expired_deletes_old_keeps_recent() {
    let _guard = restriction_lock().lock().await;
    let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());

    let old_id = insert_log_with_age(100).await;
    let recent_id = insert_log_with_age(1).await;

    let deleted = repo
        .cleanup_expired(90, &RetentionBatchPolicy::default())
        .await
        .expect("cleanup_expired failed");
    assert!(
        deleted >= 1,
        "deleted count should include our old row: {deleted}"
    );

    assert!(
        !row_exists(old_id).await,
        "100-day-old row should be deleted"
    );
    assert!(row_exists(recent_id).await, "1-day-old row should be kept");
}

/// R-retention-003：retention_days=0 时所有历史行都过期。
#[tokio::test]
async fn cleanup_expired_with_zero_retention_removes_fresh_rows() {
    let _guard = restriction_lock().lock().await;
    let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());

    let fresh_id = insert_log_with_age(0).await;

    let deleted = repo
        .cleanup_expired(0, &RetentionBatchPolicy::default())
        .await
        .expect("cleanup_expired failed");
    assert!(
        deleted >= 1,
        "fresh row should be removable with 0 retention: {deleted}"
    );

    assert!(
        !row_exists(fresh_id).await,
        "row should be deleted with 0-day retention"
    );
}

/// R-retention-003：过期行数 > batch_size 时循环分批删净，返回总数 = 过期行数，新行保留。
#[tokio::test]
async fn cleanup_expired_batches_until_drained() {
    let _guard = restriction_lock().lock().await;
    let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());

    let mut old_ids = Vec::new();
    for _ in 0..5 {
        old_ids.push(insert_log_with_age(100).await);
    }
    let recent_id = insert_log_with_age(1).await;

    let policy = RetentionBatchPolicy {
        batch_size: 2,
        max_rows_per_cycle: 100_000,
        statement_timeout_ms: 60_000,
    };
    let deleted = repo
        .cleanup_expired(90, &policy)
        .await
        .expect("cleanup_expired failed");
    assert!(
        deleted >= 5,
        "our 5 old rows must be among the deleted: {deleted}"
    );

    for id in old_ids {
        assert!(!row_exists(id).await, "100-day-old row should be deleted");
    }
    assert!(row_exists(recent_id).await, "1-day-old row must survive");
}

/// R-retention-003：删除总数达到 `max_rows_per_cycle` 即封顶停止。
#[tokio::test]
async fn cleanup_expired_caps_at_max_rows_per_cycle() {
    let _guard = restriction_lock().lock().await;
    let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());

    for _ in 0..5 {
        insert_log_with_age(100).await;
    }

    let policy = RetentionBatchPolicy {
        batch_size: 2,
        max_rows_per_cycle: 3,
        statement_timeout_ms: 60_000,
    };
    // 清掉共享容器中的历史过期行，保证封顶断言精确
    repo.cleanup_expired(90, &RetentionBatchPolicy::default())
        .await
        .expect("pre-clean failed");
    for _ in 0..5 {
        insert_log_with_age(100).await;
    }
    let deleted = repo
        .cleanup_expired(90, &policy)
        .await
        .expect("cleanup_expired failed");
    assert_eq!(
        deleted, 3,
        "deletion must stop once max_rows_per_cycle is reached"
    );
}
