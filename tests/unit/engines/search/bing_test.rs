use crawlrs::infrastructure::search::bing::BingSearchEngine;
use std::time::Duration;

#[tokio::test]
async fn test_bing_cookie_management() {
    let engine = BingSearchEngine::new();

    // When: 首次获取 Cookie
    let cookie_1 = engine.get_bing_cookies("en", "US");

    // When: 1 秒后再次获取
    tokio::time::sleep(Duration::from_secs(1)).await;
    let cookie_2 = engine.get_bing_cookies("en", "US");

    // Then: 应该相同（未超过 1 小时）
    assert_eq!(cookie_1, cookie_2);
}

#[test]
fn test_bing_cookie_construction() {
    let engine = BingSearchEngine::new();
    let cookies = engine.get_bing_cookies("en", "US");

    assert_eq!(cookies.get("_EDGE_CD"), Some(&"m=US&u=en".to_string()));
    assert_eq!(cookies.get("_EDGE_S"), Some(&"mkt=US&ui=en".to_string()));
}

#[test]
fn test_bing_form_parameter_logic() {
    let engine = BingSearchEngine::new();

    // Page 1: 无 FORM 参数
    let params_1 = engine.build_params("rust", 1);
    assert!(!params_1.contains_key("FORM"));

    // Page 2: FORM=PERE
    let params_2 = engine.build_params("rust", 2);
    assert_eq!(params_2.get("FORM"), Some(&"PERE".to_string()));

    // Page 3: FORM=PERE1
    let params_3 = engine.build_params("rust", 3);
    assert_eq!(params_3.get("FORM"), Some(&"PERE1".to_string()));

    // Page 4: FORM=PERE2
    let params_4 = engine.build_params("rust", 4);
    assert_eq!(params_4.get("FORM"), Some(&"PERE2".to_string()));
}

#[test]
fn test_bing_url_decoding() {
    let engine = BingSearchEngine::new();
    let encoded = "https://www.bing.com/ck/a?u=a1aHR0cHM6Ly9leGFtcGxlLmNvbQ";
    let decoded = engine.decode_bing_url(encoded);

    assert_eq!(decoded, "https://example.com");
}
