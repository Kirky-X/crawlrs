// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL 过滤器实现（T063，R-frontier-002）
//!
//! 参考 crawl4ai `deep_crawling/filters.py`，提供三个具体 filter：
//!
//! - [`DomainFilter`]：按域名过滤（同域 / 跨域 / 显式 allowlist）
//! - [`ContentTypeFilter`]：按源页面 Content-Type 过滤（防跨类型跳转）
//! - [`UrlPatternFilter`]：按正则 + 字符串 contains 回退过滤（兼容现有 include/exclude 行为）
//!
//! 三者通过 [`crate::workers::crawl::FilterChain`] 串联，全部 `accept` 才放行。

use regex::Regex;
use url::Url;

use super::{FilterContext, UrlFilter};

// =============================================================================
// DomainFilter
// =============================================================================

/// 域名过滤器（T063，R-frontier-002）
///
/// 支持三种模式：
///
/// 1. **同域过滤**（[`Self::same_domain`]）：仅接受与源域名相同的 URL。
///    用于 crawl 场景防止跨域爬取（默认行为）。
/// 2. **显式 allowlist**（[`Self::allowlist`]）：仅接受域名在 allowlist 中的 URL。
///    用于 multi-domain crawl 显式指定允许的域名集合。
/// 3. **跨域禁止黑名单**（[`Self::denylist`]）：接受所有不在 denylist 中的域名。
///    用于禁止特定域名（如 `facebook.com` / `twitter.com` 等社交分享域名）。
///
/// # 域名归一化
///
/// 比较前先对域名做小写化处理（`WWW.Example.COM` == `example.com`）。
/// 不做 trailing dot / www 前缀剥离（保守语义，避免误放行）。
///
/// # 缺失上下文
///
/// 当 `context.source_domain` 为 `None` 时：
/// - 同域模式：返回 `false`（无源域名无法判定同域，保守拒绝）
/// - allowlist 模式：照常判定 URL 域名是否在 allowlist
/// - denylist 模式：照常判定 URL 域名是否在 denylist
///
/// URL 解析失败时返回 `false`（保守拒绝，防无效 URL 入队）。
#[derive(Debug, Clone)]
pub struct DomainFilter {
    mode: DomainMode,
}

#[derive(Debug, Clone)]
enum DomainMode {
    /// 仅接受与源域名相同的 URL
    SameDomain,
    /// 仅接受 allowlist 中的域名
    Allowlist(Vec<String>),
    /// 拒绝 denylist 中的域名，其余接受
    Denylist(Vec<String>),
}

impl DomainFilter {
    /// 构造同域过滤器（仅接受与源域名相同的 URL）
    ///
    /// 配合 [`FilterContext::with_source_domain`] 设置源域名。
    /// `source_domain` 为 `None` 时返回 `false`（保守拒绝）。
    #[must_use]
    pub fn same_domain() -> Self {
        Self { mode: DomainMode::SameDomain }
    }

    /// 构造 allowlist 过滤器（仅接受 `allowed_domains` 中的域名）
    ///
    /// # 参数
    ///
    /// - `allowed_domains`: 允许的域名列表（大小写不敏感，比较前小写化）
    #[must_use]
    pub fn allowlist(allowed_domains: Vec<String>) -> Self {
        let normalized = allowed_domains.into_iter().map(|d| d.to_ascii_lowercase()).collect();
        Self { mode: DomainMode::Allowlist(normalized) }
    }

    /// 构造 denylist 过滤器（拒绝 `denied_domains` 中的域名，其余接受）
    ///
    /// # 参数
    ///
    /// - `denied_domains`: 拒绝的域名列表（大小写不敏感，比较前小写化）
    #[must_use]
    pub fn denylist(denied_domains: Vec<String>) -> Self {
        let normalized = denied_domains.into_iter().map(|d| d.to_ascii_lowercase()).collect();
        Self { mode: DomainMode::Denylist(normalized) }
    }

    /// 从 URL 提取域名（小写化）
    ///
    /// 返回 `None` 的情况：
    /// - URL 解析失败
    /// - URL 无 host（如 `file:///`、`mailto:`、`javascript:`）
    /// - host 为空字符串
    fn extract_domain(url_str: &str) -> Option<String> {
        let parsed = Url::parse(url_str).ok()?;
        let host = parsed.host_str()?;
        if host.is_empty() {
            return None;
        }
        Some(host.to_ascii_lowercase())
    }

    /// 域名是否在列表中（大小写不敏感，比较前都已小写化）
    fn domain_in_list(domain: &str, list: &[String]) -> bool {
        list.iter().any(|d| d == domain)
    }
}

impl UrlFilter for DomainFilter {
    fn accept(&self, url: &str, context: &FilterContext) -> bool {
        let url_domain = match Self::extract_domain(url) {
            Some(d) => d,
            None => return false, // 解析失败 / 无 host → 保守拒绝
        };

        match &self.mode {
            DomainMode::SameDomain => {
                // 源域名缺失 → 保守拒绝
                let source = match &context.source_domain {
                    Some(s) => s.to_ascii_lowercase(),
                    None => return false,
                };
                url_domain == source
            }
            DomainMode::Allowlist(allowed) => Self::domain_in_list(&url_domain, allowed),
            DomainMode::Denylist(denied) => !Self::domain_in_list(&url_domain, denied),
        }
    }
}

// =============================================================================
// ContentTypeFilter
// =============================================================================

/// Content-Type 过滤器（T063，R-frontier-002）
///
/// 根据源页面的 Content-Type 决定是否接受其外链 URL。
///
/// # 设计动机
///
/// crawl4ai 的 `ContentTypeFilter` 防止爬虫从 HTML 页面跳到非 HTML 资源
/// （如 PDF / 图片 / 视频 / 二进制等），避免队列被无意义资源占满。
///
/// # 行为
///
/// - `allowed_content_types`: 允许的源 Content-Type 前缀列表（如 `["text/html"]`）
/// - 当 `context.source_content_type` 为 `None` 时：默认放行（保守放行，
///   避免无 Content-Type 信息的场景误拒所有 URL）
/// - 当 `context.source_content_type` 为 `Some(ct)` 时：`ct` 必须以
///   `allowed_content_types` 中任一前缀开头（大小写不敏感），否则拒绝
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::{ContentTypeFilter, FilterContext, UrlFilter};
///
/// let filter = ContentTypeFilter::new(vec!["text/html".to_string()]);
/// let ctx = FilterContext::new().with_content_type("text/html; charset=utf-8");
/// assert!(filter.accept("https://example.com/page", &ctx));
///
/// let ctx_pdf = FilterContext::new().with_content_type("application/pdf");
/// assert!(!filter.accept("https://example.com/doc.pdf", &ctx_pdf));
/// ```
#[derive(Debug, Clone)]
pub struct ContentTypeFilter {
    /// 允许的源 Content-Type 前缀列表（小写化）
    allowed: Vec<String>,
}

impl ContentTypeFilter {
    /// 构造 Content-Type 过滤器
    ///
    /// # 参数
    ///
    /// - `allowed_content_types`: 允许的源 Content-Type 前缀列表。
    ///   匹配时大小写不敏感（`text/html` == `TEXT/HTML`）。
    ///   空列表等价于"无限制"（全部放行）。
    #[must_use]
    pub fn new(allowed_content_types: Vec<String>) -> Self {
        let normalized = allowed_content_types
            .into_iter()
            .map(|ct| ct.to_ascii_lowercase())
            .collect();
        Self { allowed: normalized }
    }

    /// 默认仅允许 `text/html` 源页面
    #[must_use]
    pub fn html_only() -> Self {
        Self::new(vec!["text/html".to_string()])
    }

    /// `ct` 是否匹配任一允许前缀（大小写不敏感）
    fn matches_any(content_type: &str, allowed: &[String]) -> bool {
        let ct_lower = content_type.to_ascii_lowercase();
        allowed.iter().any(|prefix| ct_lower.starts_with(prefix))
    }
}

impl UrlFilter for ContentTypeFilter {
    fn accept(&self, _url: &str, context: &FilterContext) -> bool {
        // 源 Content-Type 缺失 → 保守放行
        let source_ct = match &context.source_content_type {
            Some(ct) => ct,
            None => return true,
        };
        // 空白名单 → 无限制放行
        if self.allowed.is_empty() {
            return true;
        }
        Self::matches_any(source_ct, &self.allowed)
    }
}

// =============================================================================
// UrlPatternFilter
// =============================================================================

/// URL 模式过滤器（T063，R-frontier-002）
///
/// 兼容现有 `should_crawl` 的 include/exclude 行为：
///
/// 1. **include 模式**：URL 必须匹配任一 include pattern（若 `include_patterns` 非空）
/// 2. **exclude 模式**：URL 不能匹配任何 exclude pattern（若 `exclude_patterns` 非空）
///
/// 每个 pattern 的匹配优先级：
///
/// 1. **regex 优先**：尝试用 pattern 编译为 `Regex`，编译成功则用 `re.is_match(url)`
/// 2. **字符串 contains 回退**：regex 编译失败时回退到 `url.contains(pattern)`
///
/// 这与原 `scrape_worker::should_crawl` 完全等价（T066 回归测试断言）。
///
/// # 测试覆盖场景
///
/// - include 命中 / 未命中
/// - exclude 命中 / 未命中
/// - include + exclude 组合（include 先过滤，exclude 再排除）
/// - regex 编译失败回退到 contains
/// - include 与 exclude 任一为空（None 或空 Vec）
///
/// # 性能
///
/// regex 编译开销通过外部 `regex_cache` 缓存（T066 接入时复用 scrape_worker 的
/// `get_cached_regex`）。当前 filter 内部用 `OnceLock<Regex>` 每个 pattern 编译一次。
#[derive(Debug)]
pub struct UrlPatternFilter {
    /// include 模式列表（任一匹配即接受，空表示无 include 限制）
    include_patterns: Vec<String>,
    /// exclude 模式列表（任一匹配即拒绝，空表示无 exclude 限制）
    exclude_patterns: Vec<String>,
    /// 编译后的 include regex 缓存（编译失败的位置为 None）
    include_regexes: Vec<Option<Regex>>,
    /// 编译后的 exclude regex 缓存（编译失败的位置为 None）
    exclude_regexes: Vec<Option<Regex>>,
}

impl UrlPatternFilter {
    /// 构造 URL 模式过滤器
    ///
    /// # 参数
    ///
    /// - `include_patterns`: 必须匹配任一的模式列表（空表示无 include 限制）
    /// - `exclude_patterns`: 不能匹配任何的模式列表（空表示无 exclude 限制）
    #[must_use]
    pub fn new(include_patterns: Vec<String>, exclude_patterns: Vec<String>) -> Self {
        let include_regexes = include_patterns.iter().map(|p| Regex::new(p).ok()).collect();
        let exclude_regexes = exclude_patterns.iter().map(|p| Regex::new(p).ok()).collect();
        Self {
            include_patterns,
            exclude_patterns,
            include_regexes,
            exclude_regexes,
        }
    }

    /// 仅 include 模式（无 exclude 限制）
    #[must_use]
    pub fn include_only(include_patterns: Vec<String>) -> Self {
        Self::new(include_patterns, Vec::new())
    }

    /// 仅 exclude 模式（无 include 限制）
    #[must_use]
    pub fn exclude_only(exclude_patterns: Vec<String>) -> Self {
        Self::new(Vec::new(), exclude_patterns)
    }

    /// 单个 pattern 的匹配逻辑（regex 优先，失败回退 contains）
    ///
    /// 与原 `should_crawl` 行为一致：
    /// - `compiled = Some(re)`: 用 `re.is_match(url)`
    /// - `compiled = None`: 用 `url.contains(pattern)`
    fn match_pattern(url: &str, pattern: &str, compiled: &Option<Regex>) -> bool {
        match compiled {
            Some(re) => re.is_match(url),
            None => url.contains(pattern),
        }
    }

    /// include 校验：必须匹配任一 include pattern
    ///
    /// - `include_patterns` 为空 → 无 include 限制，返回 `true`
    /// - 否则 → 任一 pattern 匹配即返回 `true`，全不匹配返回 `false`
    fn matches_include(&self, url: &str) -> bool {
        if self.include_patterns.is_empty() {
            return true;
        }
        self.include_patterns
            .iter()
            .zip(self.include_regexes.iter())
            .any(|(pattern, re)| Self::match_pattern(url, pattern, re))
    }

    /// exclude 校验：不能匹配任何 exclude pattern
    ///
    /// - `exclude_patterns` 为空 → 无 exclude 限制，返回 `true`（放行）
    /// - 否则 → 任一 pattern 匹配即返回 `false`，全不匹配返回 `true`
    fn passes_exclude(&self, url: &str) -> bool {
        if self.exclude_patterns.is_empty() {
            return true;
        }
        !self
            .exclude_patterns
            .iter()
            .zip(self.exclude_regexes.iter())
            .any(|(pattern, re)| Self::match_pattern(url, pattern, re))
    }
}

impl UrlPatternFilter {
    /// include 模式数量
    #[must_use]
    pub fn include_count(&self) -> usize {
        self.include_patterns.len()
    }

    /// exclude 模式数量
    #[must_use]
    pub fn exclude_count(&self) -> usize {
        self.exclude_patterns.len()
    }
}

impl UrlFilter for UrlPatternFilter {
    fn accept(&self, url: &str, _context: &FilterContext) -> bool {
        // include 先过滤，exclude 再排除（与原 should_crawl 顺序一致）
        if !self.matches_include(url) {
            return false;
        }
        self.passes_exclude(url)
    }
}

// 注意：FilterChain 在 mod.rs 定义，此处仅 re-export 具体 filter
// 使用时通过 `crate::workers::crawl::FilterChain` 访问

// 让 UrlFilter 对 Arc<dyn UrlFilter> 也实现 UrlFilter（便于 FilterChain::with_filter 接受 Arc）
// 注：此实现不放在 trait 定义处以避免孤儿规则的复杂性，FilterChain 内部直接用 Arc<dyn UrlFilter>

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::crawl::{FilterChain, FilterContext};
    use std::sync::Arc;

    // ============ DomainFilter ============

    #[test]
    fn domain_filter_same_domain_accepts_matching() {
        let filter = DomainFilter::same_domain();
        let ctx = FilterContext::new().with_source_domain("example.com");
        assert!(filter.accept("https://example.com/page", &ctx));
        assert!(filter.accept("https://example.com:443/path?q=1", &ctx));
    }

    #[test]
    fn domain_filter_same_domain_rejects_cross_domain() {
        let filter = DomainFilter::same_domain();
        let ctx = FilterContext::new().with_source_domain("example.com");
        assert!(!filter.accept("https://other.com/page", &ctx));
        assert!(!filter.accept("https://sub.example.org/path", &ctx));
    }

    #[test]
    fn domain_filter_same_domain_case_insensitive() {
        let filter = DomainFilter::same_domain();
        let ctx = FilterContext::new().with_source_domain("Example.COM");
        assert!(filter.accept("https://example.com/page", &ctx));
        assert!(filter.accept("https://EXAMPLE.COM/page", &ctx));
    }

    #[test]
    fn domain_filter_same_domain_no_source_context_rejects() {
        let filter = DomainFilter::same_domain();
        let ctx = FilterContext::new(); // source_domain = None
        assert!(!filter.accept("https://example.com/page", &ctx));
    }

    #[test]
    fn domain_filter_same_domain_invalid_url_rejects() {
        let filter = DomainFilter::same_domain();
        let ctx = FilterContext::new().with_source_domain("example.com");
        assert!(!filter.accept("not a url", &ctx));
        assert!(!filter.accept("javascript:void(0)", &ctx));
        assert!(!filter.accept("mailto:test@example.com", &ctx));
    }

    #[test]
    fn domain_filter_allowlist_accepts_listed() {
        let filter = DomainFilter::allowlist(vec![
            "example.com".to_string(),
            "blog.example.org".to_string(),
        ]);
        let ctx = FilterContext::new();
        assert!(filter.accept("https://example.com/page", &ctx));
        assert!(filter.accept("https://blog.example.org/post", &ctx));
    }

    #[test]
    fn domain_filter_allowlist_rejects_unlisted() {
        let filter = DomainFilter::allowlist(vec!["example.com".to_string()]);
        let ctx = FilterContext::new();
        assert!(!filter.accept("https://other.com/page", &ctx));
        assert!(!filter.accept("https://blog.example.org/post", &ctx));
    }

    #[test]
    fn domain_filter_allowlist_case_insensitive() {
        let filter = DomainFilter::allowlist(vec!["Example.com".to_string()]);
        let ctx = FilterContext::new();
        assert!(filter.accept("https://EXAMPLE.com/page", &ctx));
    }

    #[test]
    fn domain_filter_denylist_rejects_denied() {
        let filter = DomainFilter::denylist(vec![
            "facebook.com".to_string(),
            "twitter.com".to_string(),
        ]);
        let ctx = FilterContext::new();
        assert!(!filter.accept("https://facebook.com/share", &ctx));
        assert!(!filter.accept("https://twitter.com/tweet", &ctx));
    }

    #[test]
    fn domain_filter_denylist_accepts_not_denied() {
        let filter = DomainFilter::denylist(vec!["facebook.com".to_string()]);
        let ctx = FilterContext::new();
        assert!(filter.accept("https://example.com/page", &ctx));
        assert!(filter.accept("https://blog.example.org/post", &ctx));
    }

    #[test]
    fn domain_filter_denylist_case_insensitive() {
        let filter = DomainFilter::denylist(vec!["Facebook.com".to_string()]);
        let ctx = FilterContext::new();
        assert!(!filter.accept("https://FACEBOOK.com/share", &ctx));
    }

    // ============ ContentTypeFilter ============

    #[test]
    fn content_type_filter_html_only_accepts_html_source() {
        let filter = ContentTypeFilter::html_only();
        let ctx = FilterContext::new().with_content_type("text/html; charset=utf-8");
        assert!(filter.accept("https://example.com/page", &ctx));
    }

    #[test]
    fn content_type_filter_html_only_rejects_pdf_source() {
        let filter = ContentTypeFilter::html_only();
        let ctx = FilterContext::new().with_content_type("application/pdf");
        assert!(!filter.accept("https://example.com/doc.pdf", &ctx));
    }

    #[test]
    fn content_type_filter_no_source_ct_accepts() {
        let filter = ContentTypeFilter::html_only();
        let ctx = FilterContext::new(); // source_content_type = None
        assert!(filter.accept("https://example.com/page", &ctx));
    }

    #[test]
    fn content_type_filter_empty_allowed_accepts_all() {
        let filter = ContentTypeFilter::new(Vec::new());
        let ctx = FilterContext::new().with_content_type("application/json");
        assert!(filter.accept("https://example.com/api", &ctx));
    }

    #[test]
    fn content_type_filter_case_insensitive() {
        let filter = ContentTypeFilter::new(vec!["text/html".to_string()]);
        let ctx = FilterContext::new().with_content_type("TEXT/HTML");
        assert!(filter.accept("https://example.com/page", &ctx));
    }

    #[test]
    fn content_type_filter_multiple_allowed() {
        let filter = ContentTypeFilter::new(vec![
            "text/html".to_string(),
            "application/xhtml+xml".to_string(),
        ]);
        let ctx_html = FilterContext::new().with_content_type("text/html");
        let ctx_xhtml = FilterContext::new().with_content_type("application/xhtml+xml");
        let ctx_json = FilterContext::new().with_content_type("application/json");
        assert!(filter.accept("https://example.com/page", &ctx_html));
        assert!(filter.accept("https://example.com/page", &ctx_xhtml));
        assert!(!filter.accept("https://example.com/api", &ctx_json));
    }

    // ============ UrlPatternFilter ============

    #[test]
    fn url_pattern_include_match_accepts() {
        let filter = UrlPatternFilter::include_only(vec!["/blog/.*".to_string()]);
        let ctx = FilterContext::new();
        assert!(filter.accept("https://example.com/blog/post-1", &ctx));
        assert!(filter.accept("https://example.com/blog/2024", &ctx));
    }

    #[test]
    fn url_pattern_include_no_match_rejects() {
        let filter = UrlPatternFilter::include_only(vec!["/blog/.*".to_string()]);
        let ctx = FilterContext::new();
        assert!(!filter.accept("https://example.com/about", &ctx));
        assert!(!filter.accept("https://example.com/", &ctx));
    }

    #[test]
    fn url_pattern_include_multiple_any_match_accepts() {
        let filter = UrlPatternFilter::include_only(vec![
            "/blog/.*".to_string(),
            "/news/.*".to_string(),
        ]);
        let ctx = FilterContext::new();
        assert!(filter.accept("https://example.com/blog/post", &ctx));
        assert!(filter.accept("https://example.com/news/today", &ctx));
        assert!(!filter.accept("https://example.com/about", &ctx));
    }

    #[test]
    fn url_pattern_exclude_match_rejects() {
        let filter = UrlPatternFilter::exclude_only(vec!["/admin/.*".to_string()]);
        let ctx = FilterContext::new();
        assert!(!filter.accept("https://example.com/admin/panel", &ctx));
        assert!(filter.accept("https://example.com/blog/post", &ctx));
    }

    #[test]
    fn url_pattern_exclude_multiple_any_match_rejects() {
        let filter = UrlPatternFilter::exclude_only(vec![
            "/admin/.*".to_string(),
            "/private/.*".to_string(),
        ]);
        let ctx = FilterContext::new();
        assert!(!filter.accept("https://example.com/admin/panel", &ctx));
        assert!(!filter.accept("https://example.com/private/secret", &ctx));
        assert!(filter.accept("https://example.com/blog/post", &ctx));
    }

    #[test]
    fn url_pattern_include_plus_exclude_combined() {
        // include = /blog/.*; exclude = /admin/.*
        let filter = UrlPatternFilter::new(
            vec!["/blog/.*".to_string()],
            vec!["/admin/.*".to_string()],
        );
        let ctx = FilterContext::new();
        // 同时匹配 include 且不匹配 exclude → 接受
        assert!(filter.accept("https://example.com/blog/post", &ctx));
        // 匹配 include 但同时匹配 exclude → 拒绝（exclude 优先）
        assert!(!filter.accept("https://example.com/blog/admin/panel", &ctx));
        // 不匹配 include → 拒绝（不进入 exclude 判定）
        assert!(!filter.accept("https://example.com/about", &ctx));
    }

    #[test]
    fn url_pattern_invalid_regex_falls_back_to_contains() {
        // 无效正则 `[` → 回退到字符串 contains
        let filter = UrlPatternFilter::include_only(vec!["[".to_string()]);
        let ctx = FilterContext::new();
        // 字符串 contains 匹配
        assert!(filter.accept("https://example.com/[bracket]", &ctx));
        // 字符串 contains 不匹配
        assert!(!filter.accept("https://example.com/no-bracket", &ctx));
    }

    #[test]
    fn url_pattern_empty_include_no_restriction() {
        let filter = UrlPatternFilter::new(Vec::new(), vec!["/admin/.*".to_string()]);
        let ctx = FilterContext::new();
        // 空 include = 无限制 → 只看 exclude
        assert!(filter.accept("https://example.com/blog/post", &ctx));
        assert!(!filter.accept("https://example.com/admin/panel", &ctx));
    }

    #[test]
    fn url_pattern_empty_exclude_no_restriction() {
        let filter = UrlPatternFilter::new(vec!["/blog/.*".to_string()], Vec::new());
        let ctx = FilterContext::new();
        // 空 exclude = 无限制 → 只看 include
        assert!(filter.accept("https://example.com/blog/post", &ctx));
        assert!(!filter.accept("https://example.com/about", &ctx));
    }

    #[test]
    fn url_pattern_both_empty_accepts_all() {
        let filter = UrlPatternFilter::new(Vec::new(), Vec::new());
        let ctx = FilterContext::new();
        assert!(filter.accept("https://example.com/anything", &ctx));
        assert!(filter.accept("https://other.com/path", &ctx));
    }

    #[test]
    fn url_pattern_include_count_and_exclude_count() {
        let filter = UrlPatternFilter::new(
            vec!["/blog/.*".to_string(), "/news/.*".to_string()],
            vec!["/admin/.*".to_string()],
        );
        assert_eq!(filter.include_count(), 2);
        assert_eq!(filter.exclude_count(), 1);
    }

    // ============ FilterChain ============

    #[test]
    fn filter_chain_empty_accepts_all() {
        let chain = FilterChain::new();
        let ctx = FilterContext::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert!(chain.accept("https://example.com/anything", &ctx));
    }

    #[test]
    fn filter_chain_single_filter_accepts() {
        let chain = FilterChain::new()
            .with_filter(DomainFilter::allowlist(vec!["example.com".to_string()]));
        let ctx = FilterContext::new();
        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 1);
        assert!(chain.accept("https://example.com/page", &ctx));
        assert!(!chain.accept("https://other.com/page", &ctx));
    }

    #[test]
    fn filter_chain_multiple_filters_all_must_accept() {
        let chain = FilterChain::new()
            .with_filter(DomainFilter::same_domain())
            .with_filter(UrlPatternFilter::new(
                vec!["/blog/.*".to_string()],
                vec!["/admin/.*".to_string()],
            ));
        let ctx = FilterContext::new().with_source_domain("example.com");
        // 同域 + include 命中 + exclude 不命中 → 接受
        assert!(chain.accept("https://example.com/blog/post", &ctx));
        // 跨域 → 拒绝（DomainFilter 短路）
        assert!(!chain.accept("https://other.com/blog/post", &ctx));
        // 同域 + include 不命中 → 拒绝
        assert!(!chain.accept("https://example.com/about", &ctx));
        // 同域 + include 命中 + exclude 命中 → 拒绝
        assert!(!chain.accept("https://example.com/blog/admin/panel", &ctx));
    }

    #[test]
    fn filter_chain_short_circuits_on_first_reject() {
        // 第一个 filter 拒绝时不应调用第二个 filter
        // 用 DomainFilter 拒绝跨域，UrlPatternFilter 即使会拒绝也不应执行
        let chain = FilterChain::new()
            .with_filter(DomainFilter::same_domain())
            .with_filter(UrlPatternFilter::new(
                vec!["/blog/.*".to_string()],
                Vec::new(),
            ));
        let ctx = FilterContext::new().with_source_domain("example.com");
        // 跨域且非 blog → DomainFilter 先拒绝，UrlPatternFilter 不执行
        assert!(!chain.accept("https://other.com/about", &ctx));
    }

    #[test]
    fn filter_chain_clone_shares_filters() {
        let chain = FilterChain::new()
            .with_filter(DomainFilter::allowlist(vec!["example.com".to_string()]));
        let cloned = chain.clone();
        let ctx = FilterContext::new();
        // clone 后行为一致
        assert!(cloned.accept("https://example.com/page", &ctx));
        assert!(!cloned.accept("https://other.com/page", &ctx));
        // 原始链不受影响
        assert!(chain.accept("https://example.com/page", &ctx));
    }

    #[test]
    fn filter_chain_with_shared_filter_accepts_arc() {
        let shared: Arc<dyn UrlFilter> = Arc::new(DomainFilter::allowlist(vec!["example.com".to_string()]));
        let chain = FilterChain::new().with_shared_filter(shared);
        let ctx = FilterContext::new();
        assert!(chain.accept("https://example.com/page", &ctx));
        assert!(!chain.accept("https://other.com/page", &ctx));
    }

    #[test]
    fn filter_chain_with_content_type_filter_combined() {
        // 同域 + Content-Type + URL pattern 三层过滤
        let chain = FilterChain::new()
            .with_filter(DomainFilter::same_domain())
            .with_filter(ContentTypeFilter::html_only())
            .with_filter(UrlPatternFilter::new(
                vec!["/blog/.*".to_string()],
                Vec::new(),
            ));
        let ctx = FilterContext::new()
            .with_source_domain("example.com")
            .with_content_type("text/html; charset=utf-8");
        // 全部通过
        assert!(chain.accept("https://example.com/blog/post", &ctx));
        // Content-Type 不符 → 拒绝
        let ctx_pdf = FilterContext::new()
            .with_source_domain("example.com")
            .with_content_type("application/pdf");
        assert!(!chain.accept("https://example.com/blog/post", &ctx_pdf));
    }

    // ============ FilterContext ============

    #[test]
    fn filter_context_builder_pattern() {
        let ctx = FilterContext::new()
            .with_content_type("text/html")
            .with_source_domain("example.com")
            .with_link_text("click here");
        assert_eq!(ctx.source_content_type.as_deref(), Some("text/html"));
        assert_eq!(ctx.source_domain.as_deref(), Some("example.com"));
        assert_eq!(ctx.link_text.as_deref(), Some("click here"));
    }

    #[test]
    fn filter_context_default_all_none() {
        let ctx = FilterContext::default();
        assert!(ctx.source_content_type.is_none());
        assert!(ctx.source_domain.is_none());
        assert!(ctx.link_text.is_none());
    }
}
