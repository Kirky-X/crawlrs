// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ssrf/redirect.rs 单元测试
//!
//! 补齐 src/presentation/helpers/ssrf/redirect.rs 中未覆盖的行：
//! - RedirectValidator::would_validate 的 max_redirects 超限路径
//! - RedirectValidator::would_validate 的 same-host 违规路径
//! - RedirectValidator: 无 original_host 时的 same-host 跳过路径
//! - RedirectValidator: 多次 validate 后 visited_hosts 累积
//! - RedirectValidator: reset 后重新使用
//!
//! Task9: redirect.rs 中重复的 create_ssrf_safe_redirect_policy 函数已删除
//! （src/utils/http_client.rs 已有同名实现并被 create_client 实际调用），
//! 其相关内联测试一并清理。本文件仅测试 RedirectValidator/RedirectPolicy 公共 API。

#![cfg(test)]

use crawlrs::presentation::helpers::ssrf::{RedirectPolicy, RedirectValidator, SsrfError};

// ============================================================
// RedirectValidator::would_validate max_redirects 超限路径
// ============================================================

#[test]
fn tc_would_validate_max_redirects_exceeded() {
    let validator = RedirectValidator::with_policy(RedirectPolicy::follow_with_validation(3));
    // current_count >= max_redirects 应返回 MaxRedirectsExceeded
    let result = validator.would_validate("http://example.com", 3);
    match result {
        Err(SsrfError::MaxRedirectsExceeded { limit }) => {
            assert_eq!(limit, 3);
        }
        Err(e) => panic!("expected MaxRedirectsExceeded, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[test]
fn tc_would_validate_max_redirects_exceeded_high_count() {
    let validator = RedirectValidator::with_policy(RedirectPolicy::follow_with_validation(2));
    // current_count 远超 max_redirects
    let result = validator.would_validate("http://example.com", 10);
    match result {
        Err(SsrfError::MaxRedirectsExceeded { limit }) => {
            assert_eq!(limit, 2);
        }
        Err(e) => panic!("expected MaxRedirectsExceeded, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// RedirectValidator::would_validate same-host 违规路径
// ============================================================

#[test]
fn tc_would_validate_same_host_violation() {
    let validator = RedirectValidator::with_policy(RedirectPolicy::same_host_only(10))
        .with_original_url("http://example.com");
    // 跨主机重定向应返回 ValidationFailed
    let result = validator.would_validate("http://other.com", 0);
    match result {
        Err(SsrfError::ValidationFailed(msg)) => {
            assert!(msg.contains("Cross-host redirect"), "got: {}", msg);
            assert!(msg.contains("example.com"), "got: {}", msg);
            assert!(msg.contains("other.com"), "got: {}", msg);
        }
        Err(e) => panic!("expected ValidationFailed, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[test]
fn tc_would_validate_same_host_allowed() {
    let validator = RedirectValidator::with_policy(RedirectPolicy::same_host_only(10))
        .with_original_url("http://example.com");
    // 同主机重定向应通过
    let result = validator.would_validate("http://example.com/page", 0);
    assert!(result.is_ok(), "same-host redirect should be allowed");
}

// ============================================================
// RedirectValidator::would_validate redirects 禁用路径
// ============================================================

#[test]
fn tc_would_validate_redirects_disabled() {
    let validator = RedirectValidator::with_policy(RedirectPolicy::none());
    let result = validator.would_validate("http://example.com", 0);
    match result {
        Err(SsrfError::ValidationFailed(msg)) => {
            assert!(msg.contains("not allowed"), "got: {}", msg);
        }
        Err(e) => panic!("expected ValidationFailed, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// RedirectValidator::would_validate 内部 URL 拦截
// ============================================================

#[test]
fn tc_would_validate_internal_url_blocked() {
    let validator = RedirectValidator::new();
    // 内部 URL 应被拦截
    let result = validator.would_validate("http://192.168.1.1", 0);
    match result {
        Err(SsrfError::RedirectToInternal { url }) => {
            assert!(url.contains("192.168.1.1"));
        }
        Err(e) => panic!("expected RedirectToInternal, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// RedirectValidator::would_validate 循环检测
// ============================================================

#[test]
fn tc_would_validate_loop_detected() {
    let mut validator = RedirectValidator::new();
    // 先添加一个 URL 到 chain
    validator.validate("http://example.com", 0).unwrap();
    // would_validate 同一 URL 应检测到循环
    let result = validator.would_validate("http://example.com", 1);
    match result {
        Err(SsrfError::ValidationFailed(msg)) => {
            assert!(msg.contains("loop"), "got: {}", msg);
        }
        Err(e) => panic!("expected ValidationFailed with loop, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// RedirectValidator: 无 original_host 时的 same-host 验证
// ============================================================

#[test]
fn tc_validate_same_host_without_original_host_skips_check() {
    // SameHostOnly 策略但未设置 original_host 时，same-host 检查应被跳过
    let mut validator = RedirectValidator::with_policy(RedirectPolicy::same_host_only(10));
    // 未调用 with_original_url，original_host 为 None
    let result = validator.validate("http://example.com", 0);
    assert!(
        result.is_ok(),
        "same-host check should be skipped when original_host is None"
    );
}

// ============================================================
// RedirectValidator: 多次 validate 后 visited_hosts 累积
// ============================================================

#[test]
fn tc_validate_multiple_redirects_accumulate_visited_hosts() {
    let mut validator = RedirectValidator::with_policy(RedirectPolicy::follow_with_validation(10))
        .with_original_url("http://start.com");

    validator.validate("http://host1.com", 0).unwrap();
    validator.validate("http://host2.com", 1).unwrap();
    validator.validate("http://host3.com", 2).unwrap();

    let visited = validator.visited_hosts();
    assert!(visited.contains("start.com"), "should contain start.com");
    assert!(visited.contains("host1.com"), "should contain host1.com");
    assert!(visited.contains("host2.com"), "should contain host2.com");
    assert!(visited.contains("host3.com"), "should contain host3.com");
    assert_eq!(visited.len(), 4, "should have 4 visited hosts");
}

// ============================================================
// RedirectValidator: reset 后重新使用
// ============================================================

#[test]
fn tc_reset_clears_state_and_allows_reuse() {
    let mut validator = RedirectValidator::with_policy(RedirectPolicy::follow_with_validation(5))
        .with_original_url("http://example.com");

    // 添加状态
    validator.validate("http://example.com/page1", 0).unwrap();
    validator.validate("http://example.com/page2", 1).unwrap();
    assert!(!validator.redirect_chain().is_empty());

    // Reset
    validator.reset();
    assert!(validator.redirect_chain().is_empty());
    assert!(validator.visited_hosts().is_empty());

    // Reset 后可以重新使用（不会因为之前的 chain 报循环错误）
    let result = validator.validate("http://example.com/page1", 0);
    assert!(result.is_ok(), "should be able to validate after reset");
}
