// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 高级缓存模式（design.md §13，T057-T059 / R-cache-001、R-cache-002）
//!
//! 提供 4 种 `CacheMode` 与 `CacheContext` 门控，允许调用方按场景精细控制
//! 缓存读写行为。`scrape_worker` 在读写 `CacheService` 前经 `CacheContext`
//! 门控，复用现有 `CacheService`，不改 oxcache 底层。
//!
//! # 跨层位置说明（MEDIUM-1 跨层依赖修复）
//!
//! 本模块原位于 `infrastructure::oxcache::cache_mode`，因 `CacheMode` 是策略
//! 枚举（domain 概念），被 `engines` 和 `application` 层依赖，造成跨层依赖
//! （engines → infrastructure、application → infrastructure）。提升至 `common`
//! 层后，依赖方向恢复正常：
//! - `engines` → `common`（合法）
//! - `application` → `common`（合法）
//! - `infrastructure::oxcache` → `common`（合法）
//!
//! # 4 模式语义矩阵
//!
//! | Mode | should_read | should_write | 场景 |
//! |------|-------------|--------------|------|
//! | Enabled | true | true | 默认：正常读写 |
//! | Disabled | false | false | 完全禁用缓存（配置层关闭） |
//! | ReadOnly | true | false | 只读：命中直返，未命中抓取不写回 |
//! | Bypass | false | true | 应急绕过：跳过读（不信任缓存），正常写回（更新缓存） |
//!
//! # WriteOnly 已删除（架构审查 CRITICAL-1 修复，规则4 + 规则5）
//!
//! 原设计中 `WriteOnly` 与 `Bypass` 在 `should_read`/`should_write`/`is_cacheable`
//! 三维行为完全等价 `(false, true)`，违反规则4（暴露冲突不要折中）与规则5（简洁优先）。
//! 经用户决策（删除 WriteOnly，统一用 Bypass），合并为单一 `Bypass` 变体。
//! 调用方需要"只写不读"语义时统一用 `Bypass`。

use crate::common::HttpMethod;
use serde::{Deserialize, Serialize};

/// 缓存模式枚举（design.md §13）
///
/// 控制 `CacheContext` 的读写行为。详见模块级文档的语义矩阵。
///
/// # Serde
///
/// 使用 `camelCase` 序列化（与 `ScrapeActionDto` 等现有 DTO 惯例一致）：
/// `enabled` / `disabled` / `readOnly` / `bypass`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheMode {
    /// 默认：正常读写
    #[default]
    Enabled,
    /// 完全禁用缓存（配置层关闭）
    Disabled,
    /// 只读：命中直返，未命中抓取不写回
    ReadOnly,
    /// 应急绕过：跳过读（不信任缓存），正常写回（更新缓存）
    ///
    /// 行为：`should_read=false, should_write=true`。
    /// 用于运行时不信任缓存脏数据的应急场景，或策略性预热（原 WriteOnly 语义合并至此）。
    Bypass,
}

/// 不可缓存 URL scheme 黑名单（T062 安全审查 LOW-1 修复）
///
/// 这些 scheme 的请求不应进入缓存：
/// - `data:` / `blob:`：内联资源，无网络往返，缓存无意义
/// - `javascript:`：脚本 URI，缓存后可能被其他请求读到并触发 XSS 执行
/// - `file:`：本地文件访问，缓存无意义且可能泄露本地文件内容
/// - `vbscript:`：IE 脚本注入向量
/// - `about:`：浏览器内部页面（about:blank 等），无网络往返
///
/// 使用小写匹配（`is_cacheable` 中 `to_ascii_lowercase` 后比较）。
const UNCACHEABLE_SCHEMES: &[&str] = &[
    "data:",
    "blob:",
    "javascript:",
    "file:",
    "vbscript:",
    "about:",
];

/// 缓存上下文（design.md §13）
///
/// 封装单次抓取请求的缓存决策输入：URL、HTTP method、缓存模式。
/// `scrape_worker` 在读写 `CacheService` 前构造 `CacheContext` 并调用
/// `should_read`/`should_write`/`is_cacheable` 门控。
#[derive(Debug, Clone)]
pub struct CacheContext {
    /// 目标 URL（用于 `is_cacheable` 判断 scheme）
    pub url: String,
    /// HTTP 方法（用于 `is_cacheable` 判断非幂等）
    pub method: HttpMethod,
    /// 缓存模式
    pub mode: CacheMode,
}

impl CacheContext {
    /// 是否允许读缓存（命中直返）
    ///
    /// - `Enabled` / `ReadOnly` → `true`
    /// - `Disabled` / `Bypass` → `false`
    #[must_use]
    pub fn should_read(&self) -> bool {
        match self.mode {
            CacheMode::Enabled | CacheMode::ReadOnly => true,
            CacheMode::Disabled | CacheMode::Bypass => false,
        }
    }

    /// 是否允许写缓存（抓取后写回）
    ///
    /// - `Enabled` / `Bypass` → `true`
    /// - `Disabled` / `ReadOnly` → `false`
    #[must_use]
    pub fn should_write(&self) -> bool {
        match self.mode {
            CacheMode::Enabled | CacheMode::Bypass => true,
            CacheMode::Disabled | CacheMode::ReadOnly => false,
        }
    }

    /// 是否可缓存（design.md §13：非 data:/blob:/POST 等）
    ///
    /// 判断条件（任一命中即返回 `false`）：
    /// - URL scheme 在不可缓存黑名单中（`data:` / `blob:` / `javascript:` / `file:` /
    ///   `vbscript:` / `about:`，T062 安全审查 LOW-1 扩展）
    /// - HTTP method 非幂等（`POST`）
    ///
    /// 其余情况返回 `true`。
    ///
    /// 注意：`is_cacheable=false` 时，`should_read`/`should_write` 的返回值
    /// 仍由 `mode` 决定；调用方应先查 `is_cacheable`，若 `false` 则跳过整个
    /// 缓存流程（既不读也不写），避免对不可缓存请求做无意义的缓存操作。
    ///
    /// # T062 安全审查 LOW-1 修复说明
    ///
    /// 原 impl 仅检查 `data:` 和 `blob:`，未覆盖以下危险 scheme：
    /// - `javascript:`：缓存后可能被其他请求读到并触发 XSS 执行
    /// - `file:`：本地文件访问，缓存无意义且可能泄露本地文件内容
    /// - `vbscript:`：IE 脚本注入向量
    /// - `about:`：浏览器内部页面（about:blank 等），无网络往返
    #[must_use]
    pub fn is_cacheable(&self) -> bool {
        // 1. URL scheme 检查：不可缓存 scheme 黑名单（T062 安全审查 LOW-1 扩展）
        //    性能审查 LOW-1：用字节切片 + eq_ignore_ascii_case 零分配比较，
        //    替代原 to_ascii_lowercase() 的 String 分配（热路径每次请求一次）。
        //    URL scheme 按 RFC 3986 为 ASCII，字节切片安全。
        for scheme in UNCACHEABLE_SCHEMES {
            if self.url.len() >= scheme.len()
                && self.url[..scheme.len()].eq_ignore_ascii_case(scheme)
            {
                return false;
            }
        }
        // 2. HTTP method 检查：POST 非幂等，不可缓存
        if matches!(self.method, HttpMethod::Post) {
            return false;
        }
        true
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // CacheMode::Default
    // =========================================================================

    #[test]
    fn test_cache_mode_default_is_enabled() {
        assert_eq!(CacheMode::default(), CacheMode::Enabled);
    }

    // =========================================================================
    // should_read: 4 模式覆盖（R-cache-001）
    // =========================================================================

    #[test]
    fn test_should_read_enabled_returns_true() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(ctx.should_read());
    }

    #[test]
    fn test_should_read_disabled_returns_false() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Disabled,
        };
        assert!(!ctx.should_read());
    }

    #[test]
    fn test_should_read_read_only_returns_true() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::ReadOnly,
        };
        assert!(ctx.should_read());
    }

    #[test]
    fn test_should_read_bypass_returns_false() {
        // Bypass：跳过读（不信任缓存），合并原 WriteOnly 语义
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Bypass,
        };
        assert!(!ctx.should_read());
    }

    // =========================================================================
    // should_write: 4 模式覆盖（R-cache-001）
    // =========================================================================

    #[test]
    fn test_should_write_enabled_returns_true() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(ctx.should_write());
    }

    #[test]
    fn test_should_write_disabled_returns_false() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Disabled,
        };
        assert!(!ctx.should_write());
    }

    #[test]
    fn test_should_write_read_only_returns_false() {
        // ReadOnly：未命中抓取不写回
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::ReadOnly,
        };
        assert!(!ctx.should_write());
    }

    #[test]
    fn test_should_write_bypass_returns_true() {
        // Bypass：跳过读，正常写回（更新缓存）
        // 合并原 WriteOnly 语义（架构审查 CRITICAL-1 修复）
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Bypass,
        };
        assert!(ctx.should_write());
    }

    // =========================================================================
    // should_read / should_write 组合矩阵（R-cache-001 完整覆盖）
    // =========================================================================

    #[test]
    fn test_read_write_matrix_all_modes() {
        let cases = [
            // (mode, expected_read, expected_write)
            (CacheMode::Enabled, true, true),
            (CacheMode::Disabled, false, false),
            (CacheMode::ReadOnly, true, false),
            (CacheMode::Bypass, false, true),
        ];

        for (mode, expected_read, expected_write) in cases {
            let ctx = CacheContext {
                url: "https://example.com".to_string(),
                method: HttpMethod::Get,
                mode,
            };
            assert_eq!(
                ctx.should_read(),
                expected_read,
                "should_read mismatch for mode {:?}",
                mode
            );
            assert_eq!(
                ctx.should_write(),
                expected_write,
                "should_write mismatch for mode {:?}",
                mode
            );
        }
    }

    // =========================================================================
    // is_cacheable: URL scheme 检查
    // =========================================================================

    #[test]
    fn test_is_cacheable_https_url_returns_true() {
        let ctx = CacheContext {
            url: "https://example.com/page".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_http_url_returns_true() {
        let ctx = CacheContext {
            url: "http://example.com/page".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_data_scheme_returns_false() {
        let ctx = CacheContext {
            url: "data:text/plain;base64,SGVsbG8=".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_blob_scheme_returns_false() {
        let ctx = CacheContext {
            url: "blob:https://example.com/uuid".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    // =========================================================================
    // T062 安全审查 LOW-1：扩展 scheme 黑名单测试
    // =========================================================================

    #[test]
    fn test_is_cacheable_javascript_scheme_returns_false() {
        // javascript: URI 缓存后可能被其他请求读到并触发 XSS 执行
        let ctx = CacheContext {
            url: "javascript:alert('xss')".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_file_scheme_returns_false() {
        // file:// 本地文件访问，缓存无意义且可能泄露本地文件内容
        let ctx = CacheContext {
            url: "file:///etc/passwd".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_vbscript_scheme_returns_false() {
        // vbscript: IE 脚本注入向量
        let ctx = CacheContext {
            url: "vbscript:msgbox('xss')".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_about_scheme_returns_false() {
        // about: 浏览器内部页面（about:blank 等），无网络往返
        let ctx = CacheContext {
            url: "about:blank".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_extended_schemes_case_insensitive() {
        // 大小写不敏感检查（防御性编程）
        let cases = [
            "JAVASCRIPT:alert(1)",
            "File:///etc/passwd",
            "VBSCRIPT:msgbox(1)",
            "ABOUT:blank",
        ];
        for url in cases {
            let ctx = CacheContext {
                url: url.to_string(),
                method: HttpMethod::Get,
                mode: CacheMode::Enabled,
            };
            assert!(
                !ctx.is_cacheable(),
                "scheme of {} should be case-insensitive rejected",
                url
            );
        }
    }

    #[test]
    fn test_is_cacheable_data_scheme_case_insensitive() {
        // scheme 检查应大小写不敏感（URL scheme 规范定义为小写，但防御性编程）
        let ctx = CacheContext {
            url: "DATA:text/plain,hello".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_blob_scheme_case_insensitive() {
        let ctx = CacheContext {
            url: "BLOB:https://example.com/uuid".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    // =========================================================================
    // is_cacheable: HTTP method 检查
    // =========================================================================

    #[test]
    fn test_is_cacheable_get_method_returns_true() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        assert!(ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_post_method_returns_false() {
        // POST 非幂等，不可缓存
        let ctx = CacheContext {
            url: "https://example.com/api".to_string(),
            method: HttpMethod::Post,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    #[test]
    fn test_is_cacheable_post_with_data_scheme_returns_false() {
        // 两个条件都命中，仍返回 false（短路 OR）
        let ctx = CacheContext {
            url: "data:text/plain,hello".to_string(),
            method: HttpMethod::Post,
            mode: CacheMode::Enabled,
        };
        assert!(!ctx.is_cacheable());
    }

    // =========================================================================
    // is_cacheable 与 mode 无关（独立判断）
    // =========================================================================

    #[test]
    fn test_is_cacheable_independent_of_mode() {
        // is_cacheable 只看 url + method，不看 mode
        let modes = [
            CacheMode::Enabled,
            CacheMode::Disabled,
            CacheMode::ReadOnly,
            CacheMode::Bypass,
        ];
        for mode in modes {
            let ctx = CacheContext {
                url: "https://example.com".to_string(),
                method: HttpMethod::Get,
                mode,
            };
            assert!(
                ctx.is_cacheable(),
                "is_cacheable should be true for HTTPS GET regardless of mode {:?}",
                mode
            );
        }
    }

    #[test]
    fn test_is_cacheable_disabled_mode_still_checks_scheme() {
        // 即使 mode=Disabled，is_cacheable 仍按 url+method 判断
        // 调用方应先查 is_cacheable，若 false 则跳过整个缓存流程
        let ctx = CacheContext {
            url: "data:text/plain,hello".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Disabled,
        };
        assert!(!ctx.is_cacheable());
    }

    // =========================================================================
    // CacheContext 字段访问与构造
    // =========================================================================

    #[test]
    fn test_cache_context_fields_accessible() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::ReadOnly,
        };
        assert_eq!(ctx.url, "https://example.com");
        assert_eq!(ctx.method, HttpMethod::Get);
        assert_eq!(ctx.mode, CacheMode::ReadOnly);
    }

    #[test]
    fn test_cache_context_clone() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        let cloned = ctx.clone();
        assert_eq!(ctx.url, cloned.url);
        assert_eq!(ctx.method, cloned.method);
        assert_eq!(ctx.mode, cloned.mode);
    }

    // =========================================================================
    // CacheMode Copy + PartialEq
    // =========================================================================

    #[test]
    fn test_cache_mode_copy_and_eq() {
        let mode1 = CacheMode::ReadOnly;
        let mode2 = mode1; // Copy
        assert_eq!(mode1, mode2);
        assert_ne!(CacheMode::ReadOnly, CacheMode::Bypass);
    }

    // =========================================================================
    // Serde 序列化（camelCase）
    // =========================================================================

    #[test]
    fn test_cache_mode_serde_camel_case() {
        let cases = [
            (CacheMode::Enabled, "enabled"),
            (CacheMode::Disabled, "disabled"),
            (CacheMode::ReadOnly, "readOnly"),
            (CacheMode::Bypass, "bypass"),
        ];
        for (mode, expected) in cases {
            let json = serde_json::to_string(&mode).expect("serialize failed");
            assert_eq!(json, format!("\"{}\"", expected));
            let deserialized: CacheMode = serde_json::from_str(&json).expect("deserialize failed");
            assert_eq!(deserialized, mode);
        }
    }

    #[test]
    fn test_cache_mode_serde_rejects_write_only() {
        // WriteOnly 已删除，反序列化应失败
        let result: Result<CacheMode, _> = serde_json::from_str("\"writeOnly\"");
        assert!(
            result.is_err(),
            "writeOnly should be rejected after CRITICAL-1 fix"
        );
    }
}
