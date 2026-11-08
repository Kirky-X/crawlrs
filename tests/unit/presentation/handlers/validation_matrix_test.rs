// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! DTO 校验矩阵外部契约测试（validation matrix）。
//!
//! 补齐内联单测未覆盖的校验边界：extract urls 数量界、include/exclude 标签数
//! 上限、actions 数量上限、metadata 嵌套深度、ScrapeOptionsDto 嵌套字段界。
//! 全部为纯 DTO 校验（无 DB / 无 HTTP），验证公共 API 面
//! `crawlrs::application::dto` 的 `Validate` 契约（sdforge::validator）。

use sdforge::validator::Validate;

use crawlrs::application::dto::extract_request::ExtractRequestDto;
use crawlrs::application::dto::scrape_request::ScrapeRequestDto;

// ---------------------------------------------------------------------------
// extract：urls 数量界（DTO: length(min=1, max=100)）
// ---------------------------------------------------------------------------

#[test]
fn tc_extract_urls_empty_fails_validation() {
    let json = r#"{"urls":[],"prompt":"p"}"#;
    let dto: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(dto.validate().is_err(), "empty urls must fail validation");
}

#[test]
fn tc_extract_urls_at_max_100_passes_validation() {
    let urls: Vec<String> = (0..100).map(|i| format!("https://a{i}.com")).collect();
    let json = serde_json::json!({ "urls": urls, "prompt": "p" }).to_string();
    let dto: ExtractRequestDto = serde_json::from_str(&json).expect("must parse");
    assert!(
        dto.validate().is_ok(),
        "100 urls (boundary) must pass validation"
    );
}

#[test]
fn tc_extract_urls_over_max_101_fails_validation() {
    let urls: Vec<String> = (0..101).map(|i| format!("https://a{i}.com")).collect();
    let json = serde_json::json!({ "urls": urls, "prompt": "p" }).to_string();
    let dto: ExtractRequestDto = serde_json::from_str(&json).expect("must parse");
    assert!(dto.validate().is_err(), "101 urls must fail validation");
}

#[test]
fn tc_extract_non_http_url_fails_validation() {
    let json = r#"{"urls":["ftp://example.com/file"],"prompt":"p"}"#;
    let dto: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(
        dto.validate().is_err(),
        "non-http scheme must fail validation"
    );
}

#[test]
fn tc_extract_sync_wait_at_max_30000_passes_validation() {
    let json = r#"{"urls":["https://example.com"],"prompt":"p","sync_wait_ms":30000}"#;
    let dto: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(
        dto.validate().is_ok(),
        "sync_wait_ms=30000 (boundary) must pass validation"
    );
}

#[test]
fn tc_extract_sync_wait_over_max_30001_fails_validation() {
    let json = r#"{"urls":["https://example.com"],"prompt":"p","sync_wait_ms":30001}"#;
    let dto: ExtractRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(
        dto.validate().is_err(),
        "sync_wait_ms=30001 must fail DTO validation"
    );
}

// ---------------------------------------------------------------------------
// scrape：include/exclude 标签数（DTO: length(max=50)）
// ---------------------------------------------------------------------------

fn scrape_request_with_tags(tag_key: &str, count: usize) -> ScrapeRequestDto {
    let tags: Vec<String> = (0..count).map(|i| format!("tag{i}")).collect();
    let json = serde_json::json!({ "url": "https://example.com", tag_key: tags }).to_string();
    serde_json::from_str(&json).expect("must parse")
}

#[test]
fn tc_scrape_include_tags_at_max_50_passes_validation() {
    let dto = scrape_request_with_tags("include_tags", 50);
    assert!(
        dto.validate().is_ok(),
        "50 include_tags (boundary) must pass validation"
    );
}

#[test]
fn tc_scrape_include_tags_over_max_51_fails_validation() {
    let dto = scrape_request_with_tags("include_tags", 51);
    assert!(
        dto.validate().is_err(),
        "51 include_tags must fail validation"
    );
}

#[test]
fn tc_scrape_exclude_tags_over_max_51_fails_validation() {
    let dto = scrape_request_with_tags("exclude_tags", 51);
    assert!(
        dto.validate().is_err(),
        "51 exclude_tags must fail validation"
    );
}

// ---------------------------------------------------------------------------
// scrape：actions 数量（DTO: length(max=20)）
// ---------------------------------------------------------------------------

fn scrape_request_with_actions(count: usize) -> ScrapeRequestDto {
    let actions: Vec<serde_json::Value> = (0..count)
        .map(|_| serde_json::json!({"type": "wait", "milliseconds": 100}))
        .collect();
    let json = serde_json::json!({ "url": "https://example.com", "actions": actions }).to_string();
    serde_json::from_str(&json).expect("must parse")
}

#[test]
fn tc_scrape_actions_at_max_20_passes_validation() {
    let dto = scrape_request_with_actions(20);
    assert!(
        dto.validate().is_ok(),
        "20 actions (boundary) must pass validation"
    );
}

#[test]
fn tc_scrape_actions_over_max_21_fails_validation() {
    let dto = scrape_request_with_actions(21);
    assert!(dto.validate().is_err(), "21 actions must fail validation");
}

// ---------------------------------------------------------------------------
// scrape：metadata 嵌套深度（MAX_METADATA_DEPTH = 5）
// ---------------------------------------------------------------------------

fn metadata_at_depth(depth: usize) -> serde_json::Value {
    let mut value = serde_json::json!(1);
    for _ in 0..depth {
        value = serde_json::json!({ "nested": value });
    }
    value
}

#[test]
fn tc_scrape_metadata_at_max_depth_5_passes_validation() {
    let json =
        serde_json::json!({ "url": "https://example.com", "metadata": metadata_at_depth(5) })
            .to_string();
    let dto: ScrapeRequestDto = serde_json::from_str(&json).expect("must parse");
    assert!(
        dto.validate().is_ok(),
        "metadata depth 5 (boundary) must pass validation"
    );
}

#[test]
fn tc_scrape_metadata_over_max_depth_6_fails_validation() {
    let json =
        serde_json::json!({ "url": "https://example.com", "metadata": metadata_at_depth(6) })
            .to_string();
    let dto: ScrapeRequestDto = serde_json::from_str(&json).expect("must parse");
    assert!(
        dto.validate().is_err(),
        "metadata depth 6 must fail validation"
    );
}

// ---------------------------------------------------------------------------
// scrape：options 嵌套校验（wait_for ≤ 60000ms / timeout ≤ 300s）
// ---------------------------------------------------------------------------

#[test]
fn tc_scrape_options_wait_for_over_60s_fails_validation() {
    let json = r#"{"url":"https://example.com","options":{"wait_for":60001}}"#;
    let dto: ScrapeRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(
        dto.validate().is_err(),
        "options.wait_for > 60000ms must fail nested validation"
    );
}

#[test]
fn tc_scrape_options_timeout_over_300s_fails_validation() {
    let json = r#"{"url":"https://example.com","options":{"timeout":301}}"#;
    let dto: ScrapeRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(
        dto.validate().is_err(),
        "options.timeout > 300s must fail nested validation"
    );
}

#[test]
fn tc_scrape_options_within_limits_pass_validation() {
    let json = r#"{"url":"https://example.com","options":{"wait_for":60000,"timeout":300}}"#;
    let dto: ScrapeRequestDto = serde_json::from_str(json).expect("must parse");
    assert!(
        dto.validate().is_ok(),
        "options at boundary values must pass validation"
    );
}
