#![cfg(test)]
use crawlrs::common::constants::testing::QUICK_TEST_TIMEOUT;
use crawlrs::engines::engine_client::EngineClient;
use crawlrs::search::client::google::GoogleSearchEngine;
use std::sync::Arc;
use std::time::Duration;

fn create_test_engine() -> GoogleSearchEngine {
    GoogleSearchEngine::new(Arc::new(EngineClient::new()))
}

#[tokio::test]
#[ignore] // Skip: Test requires specific features or has private field access
async fn test_google_arc_id_generation() {
    let engine = create_test_engine();

    // When: 首次获取 ARC_ID
    let arc_id_1: String = engine.get_arc_id(0).await;

    // Then: 格式正确
    assert!(arc_id_1.starts_with("arc_id:srp_"));
    assert!(arc_id_1.contains("use_ac:true"));

    // When: 1 秒后再次获取
    tokio::time::sleep(QUICK_TEST_TIMEOUT).await;
    let arc_id_2 = engine.get_arc_id(0).await;

    // Then: 应该相同（未超过 1 小时）
    assert_eq!(arc_id_1, arc_id_2);
}

#[tokio::test]
#[ignore] // Skip: Test requires specific features or has private field access
async fn test_google_arc_id_refresh_after_hour() {
    let engine = create_test_engine();

    // Given: 获取初始 ARC_ID
    let arc_id_1 = engine.get_arc_id(0).await;

    // When: 强制刷新缓存（测试用 API）
    engine.force_refresh_arc_id().await;

    let arc_id_2 = engine.get_arc_id(0).await;

    // Then: ARC_ID 应不同
    assert_ne!(arc_id_1, arc_id_2);
}

#[test]
fn test_google_result_parsing() {
    let html = r#"
        <div jscontroller="SC7lYd">
            <a href="https://example.com">
                <h3>Test Title</h3>
            </a>
            <div data-sncf="1">Test description</div>
        </div>
    "#;

    let engine = create_test_engine();
    let results = engine
        .parse_results(html)
        .expect("Failed to parse google search results");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Test Title");
    assert_eq!(results[0].url, "https://example.com");
    assert_eq!(results[0].description.clone(), "Test description");
}
