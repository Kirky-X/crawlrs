use super::*;
use crate::common::test_helpers::create_test_db_pool;
use crate::domain::models::webhook_model::{WebhookEventType, WebhookStatus};
use chrono::{Duration, Utc};
use uuid::Uuid;

/// 序列化清理测试：共享 testcontainers 容器下，多个清理用例并行时会
/// 互相删除对方刚插入的数据（retention 覆盖全部行）。
static WEBHOOK_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();

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
        None,
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
        .cleanup_terminal(30)
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
}

/// R-retention-004：新的 delivered 事件（1 天前投递）不因 30 天保留期被删。
#[tokio::test]
async fn cleanup_terminal_keeps_recent_delivered() {
    let _guard = webhook_lock().lock().await;
    let repo = WebhookEventRepoImpl::new(create_test_db_pool());

    let recent_delivered = make_event(WebhookStatus::Delivered, 1, Some(1));
    repo.create(&recent_delivered)
        .await
        .expect("create failed");

    repo.cleanup_terminal(30).await.expect("cleanup failed");

    assert!(
        repo.find_by_id(recent_delivered.id)
            .await
            .expect("query failed")
            .is_some(),
        "1-day-old delivered event should be kept"
    );
}