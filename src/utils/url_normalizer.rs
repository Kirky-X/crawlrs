// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL 归一化器（design.md §9，T050/R-frontier-001）
//!
//! 将等价 URL 归一为同一规范串，便于 Bloom/Interner/DB 三层去重命中。
//!
//! ## 归一规则
//!
//! - scheme 小写（`HTTPS` → `https`）
//! - host 小写（`Example.COM` → `example.com`）
//! - 去除 fragment（`#section`）
//! - 统一 trailing slash：根路径 `/` 保留；非根路径末尾的 `/` 去除
//! - query 按键排序（同键多值保持原相对顺序，稳定排序）
//! - `strip_query = true` 时直接去除 query 段
//!
//! ## Permutations
//!
//! 生成等价 URL 变体集合用于"全量去重检查"——任何变体命中 bloom/DB 都视为已爬。
//! 变体维度（笛卡尔积）：
//! - www/non-www：自动加/去 `www.` 前缀
//! - http/https：切换协议
//! - index.html：路径末尾追加/去除 `index.html`、`index.htm`、`index.php`
//!
//! ## 错误显性化（规则 12）
//!
//! `normalize` 解析失败返回 [`UrlError::InvalidUrl`]，不静默返回原串。

use crate::utils::url::UrlError;
use std::collections::BTreeMap;
use url::Url;

/// URL 归一化器
///
/// 状态仅 `strip_query` 一个布尔，无副作用，可 `Clone + Send + Sync`。
#[derive(Debug, Clone, Copy, Default)]
pub struct UrlNormalizer {
    /// 是否去除 query 段（默认保留）
    strip_query: bool,
}

impl UrlNormalizer {
    /// 创建归一化器
    ///
    /// # 参数
    ///
    /// - `strip_query`：`true` 时归一结果去除 query 段
    pub fn new(strip_query: bool) -> Self {
        Self { strip_query }
    }

    /// 默认配置（保留 query）
    pub fn with_default() -> Self {
        Self::new(false)
    }

    /// 归一化 URL
    ///
    /// # 参数
    ///
    /// - `url`：待归一化的 URL 字符串
    ///
    /// # 返回
    ///
    /// 成功返回归一化后的字符串；解析失败返回 [`UrlError::InvalidUrl`]。
    ///
    /// # 示例
    ///
    /// ```
    /// use crawlrs::utils::url_normalizer::UrlNormalizer;
    /// let n = UrlNormalizer::new(false);
    /// assert_eq!(
    ///     n.normalize("HTTPS://Example.COM/Path/?b=2&a=1#frag").unwrap(),
    ///     "https://example.com/Path?a=1&b=2"
    /// );
    /// ```
    pub fn normalize(&self, url: &str) -> Result<String, UrlError> {
        // 空串防御
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(UrlError::InvalidUrl("empty URL".to_string()));
        }

        // 解析（base = None，无法解析相对路径）
        let parsed =
            Url::parse(trimmed).map_err(|e| UrlError::InvalidUrl(format!("parse failed: {e}")))?;

        // 仅接受 http/https
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(UrlError::InvalidUrl(format!(
                "unsupported scheme: {} (only http/https)",
                parsed.scheme()
            )));
        }

        // 重组（克隆以拥有可变 Url）
        let mut normalized = parsed;
        normalized.set_fragment(None);

        // host 小写（scheme 已是小写，Url::parse 保证）
        // 注意：set_host 会重新解析，但能正确处理 IDN 等边界情况
        if let Some(host) = normalized.host_str() {
            let lower = host.to_lowercase();
            if lower != host {
                // set_host 失败时返回错误（规则 12：显性化）
                normalized
                    .set_host(Some(&lower))
                    .map_err(|e| UrlError::InvalidUrl(format!("set_host failed: {e}")))?;
            }
        }

        // 路径 trailing slash 统一：非根路径去除末尾 /
        let path = normalized.path().to_string();
        if path.len() > 1 && path.ends_with('/') {
            // set_path 不接受空串，根路径用 "/"
            let new_path = path.trim_end_matches('/');
            normalized.set_path(if new_path.is_empty() { "/" } else { new_path });
        }

        // query 处理：strip 或排序
        if self.strip_query {
            normalized.set_query(None);
        } else if let Some(q) = normalized.query() {
            let sorted = Self::sort_query(q);
            normalized.set_query(Some(&sorted));
        }

        Ok(normalized.to_string())
    }

    /// 排序 query 字符串
    ///
    /// 同键多值保持原相对顺序（稳定排序），仅按 key 排序。
    /// 例如 `?b=2&a=1&b=3` → `?a=1&b=2&b=3`。
    ///
    /// 规则 12：serde_urlencoded 解析失败时保留原始 query 串（不静默丢弃），
    /// 避免 query 不同的 URL 被误判为等价。
    fn sort_query(query: &str) -> String {
        // serde_urlencoded 解析为 Vec<(String, String)>，保持多值顺序
        // 解析失败时返回原始 query（规则 12：不吞错为空 Vec）
        let pairs: Vec<(String, String)> = match serde_urlencoded::from_str(query) {
            Ok(p) => p,
            Err(_) => return query.to_string(),
        };

        // 用 BTreeMap 按 key 分组（保持插入顺序）
        let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (k, v) in pairs {
            grouped.entry(k).or_default().push(v);
        }

        // 重新组装（预分配容量，避免多次 realloc）
        let total_pairs: usize = grouped.values().map(|v| v.len()).sum();
        let mut parts: Vec<String> = Vec::with_capacity(total_pairs);
        for (k, vs) in grouped {
            for v in vs {
                parts.push(format!("{}={}", k, v));
            }
        }
        parts.join("&")
    }

    /// 生成等价 URL 变体集合
    ///
    /// 用于"全量去重检查"——任何变体命中 bloom/DB 都视为已爬。
    ///
    /// # 参数
    ///
    /// - `url`：原始 URL 字符串（不要求已归一化）
    ///
    /// # 返回
    ///
    /// 返回所有等价变体（含原 URL 归一化形式）。解析失败时返回仅含原串的 Vec
    /// （permutations 是"尽力而为"的扩展，原串始终保留）。
    ///
    /// # 变体维度
    ///
    /// - www/non-www：自动加/去 `www.` 前缀
    /// - http/https：切换协议
    /// - index.html：路径末尾追加 `index.html`/`index.htm`/`index.php`
    ///
    /// # 示例
    ///
    /// ```
    /// use crawlrs::utils::url_normalizer::UrlNormalizer;
    /// let n = UrlNormalizer::new(false);
    /// let perms = n.permutations("https://example.com/path");
    /// // 包含 https://example.com/path, https://www.example.com/path,
    /// // http://example.com/path, http://www.example.com/path,
    /// // https://example.com/path/index.html, ...
    /// assert!(perms.len() >= 4);
    /// ```
    pub fn permutations(&self, url: &str) -> Vec<String> {
        // 解析失败时回退原串（不静默失败，但 permutations 是尽力而为）
        let parsed = match Url::parse(url.trim()) {
            Ok(u) => u,
            Err(_) => return vec![url.to_string()],
        };

        // 仅 http/https 才做变体；其他协议原样返回
        if !matches!(parsed.scheme(), "http" | "https") {
            return vec![url.to_string()];
        }

        // 笛卡尔积上界：2 schemes × 2 hosts × 4 path_variants = 16
        let mut variants: Vec<String> = Vec::with_capacity(16);
        let host = parsed.host_str().unwrap_or("");
        let path = parsed.path();
        let query = parsed.query();

        // 维度 1：www/non-www
        let host_variants: Vec<String> = if let Some(rest) = host.strip_prefix("www.") {
            vec![host.to_string(), rest.to_string()] // [www.example.com, example.com]
        } else {
            vec![host.to_string(), format!("www.{}", host)] // [example.com, www.example.com]
        };

        // 维度 2：http/https
        let scheme_variants = ["https", "http"];

        // 维度 3：index 变体
        let path_variants = Self::path_variants(path);

        // 笛卡尔积
        for scheme in scheme_variants {
            for host_v in &host_variants {
                for path_v in &path_variants {
                    let mut candidate = Url::parse(&format!("{}://{}{}", scheme, host_v, path_v))
                        .unwrap_or_else(|_| parsed.clone());
                    candidate.set_query(query);
                    candidate.set_fragment(None);
                    variants.push(candidate.to_string());
                }
            }
        }

        // 去重（无堆分配：sort + dedup 比 HashSet 更快）
        variants.sort_unstable();
        variants.dedup();

        variants
    }

    /// 路径变体：原路径 + 各 index 后缀
    fn path_variants(path: &str) -> Vec<String> {
        // 上界：1 原路径 + 3 index 后缀 = 4
        let mut variants: Vec<String> = Vec::with_capacity(4);
        variants.push(path.to_string());

        // 已是 /xxx/index.html 时，去除 index.html 也作为变体
        for suffix in ["/index.html", "/index.htm", "/index.php"] {
            if let Some(stripped) = path.strip_suffix(suffix) {
                let stripped = if stripped.is_empty() { "/" } else { stripped };
                variants.push(stripped.to_string());
            } else {
                variants.push(format!("{}{}", path, suffix));
            }
        }

        // 去重（无堆分配）
        variants.sort_unstable();
        variants.dedup();
        variants
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== normalize: scheme/host lowercase ==========

    #[test]
    fn test_normalize_lowercase_scheme_and_host() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("HTTPS://Example.COM/Path").unwrap(),
            "https://example.com/Path"
        );
    }

    #[test]
    fn test_normalize_preserves_path_case() {
        // 路径大小写保留（URL 路径区分大小写）
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/CamelCasePath").unwrap(),
            "https://example.com/CamelCasePath"
        );
    }

    // ========== normalize: fragment ==========

    #[test]
    fn test_normalize_strips_fragment() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/path#section").unwrap(),
            "https://example.com/path"
        );
    }

    #[test]
    fn test_normalize_strips_fragment_with_query() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/path?a=1#frag").unwrap(),
            "https://example.com/path?a=1"
        );
    }

    // ========== normalize: trailing slash ==========

    #[test]
    fn test_normalize_removes_trailing_slash() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/path/").unwrap(),
            "https://example.com/path"
        );
    }

    #[test]
    fn test_normalize_preserves_root_slash() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn test_normalize_no_path_implicit_root() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn test_normalize_multiple_trailing_slashes() {
        let n = UrlNormalizer::new(false);
        // 多个 trailing slash 全部去除
        assert_eq!(
            n.normalize("https://example.com/path//").unwrap(),
            "https://example.com/path"
        );
    }

    // ========== normalize: query sorting ==========

    #[test]
    fn test_normalize_sorts_query() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/path?b=2&a=1").unwrap(),
            "https://example.com/path?a=1&b=2"
        );
    }

    #[test]
    fn test_normalize_sorts_query_stable_multi_values() {
        let n = UrlNormalizer::new(false);
        // b=2 在 b=3 之前保持原相对顺序
        assert_eq!(
            n.normalize("https://example.com/path?b=2&a=1&b=3").unwrap(),
            "https://example.com/path?a=1&b=2&b=3"
        );
    }

    #[test]
    fn test_normalize_equivalent_urls_to_same_string() {
        let n = UrlNormalizer::new(false);
        let a = n
            .normalize("HTTPS://Example.COM/Path/?b=2&a=1#frag")
            .unwrap();
        let b = n.normalize("https://example.com/Path?a=1&b=2").unwrap();
        assert_eq!(a, b);
    }

    // ========== normalize: strip_query ==========

    #[test]
    fn test_normalize_strip_query_true() {
        let n = UrlNormalizer::new(true);
        assert_eq!(
            n.normalize("https://example.com/path?a=1&b=2").unwrap(),
            "https://example.com/path"
        );
    }

    #[test]
    fn test_normalize_strip_query_false_keeps_query() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com/path?a=1&b=2").unwrap(),
            "https://example.com/path?a=1&b=2"
        );
    }

    #[test]
    fn test_normalize_strip_query_equivalent_urls() {
        let n = UrlNormalizer::new(true);
        // strip_query 时，query 不同的 URL 视为等价
        let a = n.normalize("https://example.com/path?a=1").unwrap();
        let b = n.normalize("https://example.com/path?b=2").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "https://example.com/path");
    }

    // ========== normalize: error cases ==========

    #[test]
    fn test_normalize_empty_url_returns_err() {
        let n = UrlNormalizer::new(false);
        assert!(n.normalize("").is_err());
    }

    #[test]
    fn test_normalize_whitespace_only_returns_err() {
        let n = UrlNormalizer::new(false);
        assert!(n.normalize("   ").is_err());
    }

    #[test]
    fn test_normalize_invalid_url_returns_err() {
        let n = UrlNormalizer::new(false);
        // Url::parse 对 "not a url" 解析失败
        assert!(n.normalize("not a url").is_err());
    }

    #[test]
    fn test_normalize_unsupported_scheme_returns_err() {
        let n = UrlNormalizer::new(false);
        // ftp 不在 http/https 白名单
        let result = n.normalize("ftp://example.com/file");
        // Url 能解析 ftp，但我们拒绝
        assert!(result.is_err() || result.is_ok());
        // 如果解析成功且 scheme 是 ftp，应返回 Err
        if let Ok(s) = result {
            assert!(
                s.starts_with("http"),
                "non-http should be rejected, got: {s}"
            );
        }
    }

    // ========== normalize: port handling ==========

    #[test]
    fn test_normalize_preserves_port() {
        let n = UrlNormalizer::new(false);
        assert_eq!(
            n.normalize("https://example.com:8080/path").unwrap(),
            "https://example.com:8080/path"
        );
    }

    // ========== normalize: idempotency ==========

    #[test]
    fn test_normalize_idempotent() {
        let n = UrlNormalizer::new(false);
        let once = n
            .normalize("HTTPS://Example.COM/Path/?b=2&a=1#frag")
            .unwrap();
        let twice = n.normalize(&once).unwrap();
        assert_eq!(once, twice);
    }

    // ========== permutations ==========

    #[test]
    fn test_permutations_includes_original() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path");
        assert!(perms.iter().any(|u| u.contains("example.com/path")));
    }

    #[test]
    fn test_permutations_www_and_non_www() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path");
        // 应包含 example.com 和 www.example.com 两个 host
        assert!(
            perms.iter().any(|u| u.contains("://example.com")),
            "missing non-www"
        );
        assert!(
            perms.iter().any(|u| u.contains("://www.example.com")),
            "missing www"
        );
    }

    #[test]
    fn test_permutations_http_and_https() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path");
        let has_https = perms.iter().any(|u| u.starts_with("https://"));
        let has_http = perms.iter().any(|u| u.starts_with("http://"));
        assert!(has_https, "missing https variant");
        assert!(has_http, "missing http variant");
    }

    #[test]
    fn test_permutations_index_html() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path");
        // 应包含 path/index.html 变体
        assert!(
            perms.iter().any(|u| u.contains("/path/index.html")),
            "missing index.html variant"
        );
        assert!(
            perms.iter().any(|u| u.contains("/path/index.htm")),
            "missing index.htm variant"
        );
    }

    #[test]
    fn test_permutations_strips_index_html() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path/index.html");
        // 应包含去除 index.html 后的变体
        assert!(
            perms
                .iter()
                .any(|u| u == "https://example.com/path" || u == "https://example.com/path/"),
            "missing stripped index.html variant, got: {:?}",
            perms
        );
    }

    #[test]
    fn test_permutations_from_www_includes_non_www() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://www.example.com/path");
        // 应包含去 www 后的变体
        assert!(
            perms
                .iter()
                .any(|u| u.contains("://example.com/path") && !u.contains("www.")),
            "missing non-www variant"
        );
    }

    #[test]
    fn test_permutations_deduplicates() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/");
        // 不应有重复
        let mut sorted = perms.clone();
        sorted.sort();
        let original_len = perms.len();
        sorted.dedup();
        assert_eq!(sorted.len(), original_len, "duplicates found");
    }

    #[test]
    fn test_permutations_invalid_url_returns_original() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("not a url");
        assert_eq!(perms, vec!["not a url".to_string()]);
    }

    #[test]
    fn test_permutations_non_http_scheme_returns_original() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("ftp://example.com/file");
        assert_eq!(perms, vec!["ftp://example.com/file".to_string()]);
    }

    #[test]
    fn test_permutations_preserves_query() {
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path?a=1");
        // 所有变体都应保留 query
        for p in &perms {
            assert!(p.contains("?a=1"), "missing query in variant: {}", p);
        }
    }

    #[test]
    fn test_permutations_count_at_least_4() {
        // 至少 2 schemes × 2 hosts = 4 个基础变体
        let n = UrlNormalizer::new(false);
        let perms = n.permutations("https://example.com/path");
        assert!(
            perms.len() >= 4,
            "expected >= 4 variants, got: {}",
            perms.len()
        );
    }

    // ========== UrlNormalizer construction ==========

    #[test]
    fn test_new_default_keeps_query() {
        let n = UrlNormalizer::with_default();
        assert_eq!(
            n.normalize("https://example.com/path?a=1").unwrap(),
            "https://example.com/path?a=1"
        );
    }

    #[test]
    fn test_clone_copy() {
        let n1 = UrlNormalizer::new(true);
        let n2 = n1; // Copy
        assert_eq!(
            n1.normalize("https://example.com/path?a=1").unwrap(),
            n2.normalize("https://example.com/path?a=1").unwrap()
        );
    }
}
