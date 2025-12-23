use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use std::time::Duration;

#[tokio::test]
async fn test_google_arc_id_generation() {
    let engine = GoogleSearchEngine::new();

    // When: 首次获取 ARC_ID
    let arc_id_1 = engine.get_arc_id(0).await;

    // Then: 格式正确
    assert!(arc_id_1.starts_with("arc_id:srp_"));
    assert!(arc_id_1.contains("use_ac:true"));

    // When: 1 秒后再次获取
    tokio::time::sleep(Duration::from_secs(1)).await;
    let arc_id_2 = engine.get_arc_id(0).await;

    // Then: 应该相同（未超过 1 小时）
    assert_eq!(arc_id_1, arc_id_2);
}

#[tokio::test]
async fn test_google_arc_id_refresh_after_hour() {
    let engine = GoogleSearchEngine::new();

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

    let engine = GoogleSearchEngine::new();
    let results = engine.parse_results(html).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Test Title");
    assert_eq!(results[0].url, "https://example.com");
    assert_eq!(results[0].description.clone().unwrap(), "Test description");
}
