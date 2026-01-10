// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 测试宏模块
///
/// 提供常用的测试宏

/// 导入所有公共模块的宏
#[macro_export]
macro_rules! test_setup {
    () => {
        use crate::common::assertions::*;
        use crate::common::factories::*;
        use crate::common::fixtures::*;
        use crate::common::helpers::*;
    };
}

/// 创建抓取请求负载的宏
#[macro_export]
macro_rules! create_scrape_payload {
    ($url:expr) => {
        serde_json::json!({
            "url": $url
        })
    };
}

/// 创建带选项的抓取请求负载的宏
#[macro_export]
macro_rules! create_scrape_payload_with_options {
    ($url:expr, $options:expr) => {
        serde_json::json!({
            "url": $url,
            "options": $options
        })
    };
}

/// 断言 HTTP 201 Created 或 202 Accepted
#[macro_export]
macro_rules! assert_http_created_or_accepted {
    ($response:expr) => {
        let status = $response.status_code();
        assert!(
            status == axum::http::StatusCode::CREATED || status == axum::http::StatusCode::ACCEPTED,
            "Expected 201 or 202, got {}",
            status
        );
    };
}

/// 断言 HTTP 成功状态 (2xx)
#[macro_export]
macro_rules! assert_http_success {
    ($response:expr) => {
        let status = $response.status_code();
        assert!(
            status.is_success(),
            "Expected success status (2xx), got {}",
            status
        );
    };
}

/// 创建测试任务 URL
#[macro_export]
macro_rules! test_url {
    () => {
        format!("https://example.com/test-{}", uuid::Uuid::new_v4())
    };
    ($path:expr) => {
        format!("https://example.com/{}", $path)
    };
}
