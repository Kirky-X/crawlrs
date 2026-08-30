// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `/v1/map` 业务用例：从站点 sitemap 发现 URL（bdd-acceptance-hardening R-map-002/003）。
//!
//! 语义（specs/map-endpoint）：
//! - 抓取 `{origin}/sitemap.xml`，解析 `<urlset>` 的 `<loc>` 保序返回
//! - `<sitemapindex>` 递归一层，子 sitemap 上限 5 个
//! - `<loc>` 去重保首次出现位置；sitemap 404 返回空 links（合法状态）
//! - include 白名单 → exclude 黑名单 → limit 截断
//! - 抓取经 `SitemapFetcher` 端口（生产实现 `EngineSitemapFetcher` 走 reqwest 引擎，
//!   XML 无需渲染），端口化使业务逻辑可脱离引擎单测

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;

use crate::application::dto::map_request::MapRequestDto;
use crate::engines::engine_client::EngineClient;
use crate::engines::types::ScrapeRequest;

/// 单次抓取结果
pub struct SitemapFetch {
    pub status: u16,
    pub body: String,
}

/// 用例级错误（handler 映射：TargetUnreachable → 502 MAP_TARGET_UNREACHABLE）
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("target unreachable: {0}")]
    TargetUnreachable(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// sitemap 抓取端口（测试用 mock 替换）
#[async_trait]
pub trait SitemapFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<SitemapFetch, MapError>;
}

/// 生产实现：经 EngineClient（reqwest 引擎）抓取
pub struct EngineSitemapFetcher {
    client: Arc<EngineClient>,
}

impl EngineSitemapFetcher {
    pub fn new(client: Arc<EngineClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SitemapFetcher for EngineSitemapFetcher {
    async fn fetch(&self, url: &str) -> Result<SitemapFetch, MapError> {
        let request =
            ScrapeRequest::new(url.to_string()).timeout(std::time::Duration::from_secs(30));
        let response = self
            .client
            .scrape(&request)
            .await
            .map_err(|e| MapError::TargetUnreachable(e.to_string()))?;
        Ok(SitemapFetch {
            status: response.status_code,
            body: response.content,
        })
    }
}

/// `/v1/map` 结果
pub struct MapResult {
    pub links: Vec<String>,
}

/// `/v1/map` 用例
pub struct MapUseCase {
    fetcher: Arc<dyn SitemapFetcher>,
}

/// 子 sitemap 递归上限（specs/map-endpoint R-map-002）
const MAX_SUB_SITEMAPS: usize = 5;

impl MapUseCase {
    pub fn new(fetcher: Arc<dyn SitemapFetcher>) -> Self {
        Self { fetcher }
    }

    pub async fn execute(&self, dto: &MapRequestDto) -> Result<MapResult, MapError> {
        let origin = extract_origin(&dto.url)
            .ok_or_else(|| MapError::Internal(format!("cannot parse origin from {}", dto.url)))?;

        let first = self.fetcher.fetch(&format!("{origin}/sitemap.xml")).await?;
        if first.status == 404 {
            // 站点无 sitemap 是合法状态：空结果而非错误
            return Ok(MapResult { links: Vec::new() });
        }
        if first.status >= 400 {
            return Err(MapError::TargetUnreachable(format!(
                "sitemap returned status {}",
                first.status
            )));
        }

        let mut locs = parse_locs(&first.body);
        if is_sitemap_index(&first.body) {
            // 递归一层：取前 MAX_SUB_SITEMAPS 个子 sitemap 展开
            let children: Vec<String> = locs.iter().take(MAX_SUB_SITEMAPS).cloned().collect();
            locs.clear();
            for child in children {
                let fetched = self.fetcher.fetch(&child).await?;
                if fetched.status == 200 {
                    locs.extend(parse_locs(&fetched.body));
                }
            }
        }

        // 去重（保首次出现位置）
        let mut seen: HashSet<String> = HashSet::new();
        let mut links: Vec<String> = Vec::with_capacity(locs.len());
        for loc in locs {
            if seen.insert(loc.clone()) {
                links.push(loc);
            }
        }

        // 先 include 白名单后 exclude 黑名单（R-map-003）
        if let Some(include) = &dto.include_patterns {
            links.retain(|u| include.iter().any(|p| matches_glob(p, u)));
        }
        if let Some(exclude) = &dto.exclude_patterns {
            links.retain(|u| !exclude.iter().any(|p| matches_glob(p, u)));
        }

        // limit 截断
        let limit = dto.limit.unwrap_or(1000) as usize;
        links.truncate(limit);

        Ok(MapResult { links })
    }
}

/// 从请求 URL 提取 `scheme://host[:port]` origin
fn extract_origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let scheme = parsed.scheme();
    match parsed.port() {
        Some(port) => Some(format!("{scheme}://{host}:{port}")),
        None => Some(format!("{scheme}://{host}")),
    }
}

/// 解析 sitemap XML 中全部 `<loc>` 文本（保序）
pub fn parse_locs(xml: &str) -> Vec<String> {
    let document = scraper::Html::parse_fragment(xml);
    document
        .select(&scraper::Selector::parse("loc").expect("valid selector"))
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 是否为 sitemap index（含子 sitemap 引用的根元素）
fn is_sitemap_index(xml: &str) -> bool {
    // 大小写不敏感地匹配根元素（某些站点输出 <sitemapindex> 大写变体）
    xml.to_ascii_lowercase().contains("<sitemapindex")
}

/// `*`（任意长度）/`?`（单字符）通配匹配（双指针 + 回溯，
/// 语义对齐 workers/crawl/filters.rs 的 match_pattern；application 层不依赖 workers 层故自实现）
pub fn matches_glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pos, mut text_mark) = (usize::MAX, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_pos = pi;
            text_mark = ti;
            pi += 1;
        } else if star_pos != usize::MAX {
            pi = star_pos + 1;
            text_mark += 1;
            ti = text_mark;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;

    // ========== Mock Fetcher ==========

    struct MockFetcher {
        /// url → (status, body)
        responses: HashMap<String, (u16, String)>,
    }

    #[async_trait]
    impl SitemapFetcher for MockFetcher {
        async fn fetch(&self, url: &str) -> Result<SitemapFetch, MapError> {
            match self.responses.get(url) {
                Some((status, body)) => Ok(SitemapFetch {
                    status: *status,
                    body: body.clone(),
                }),
                None => Err(MapError::TargetUnreachable(format!("no mock for {url}"))),
            }
        }
    }

    fn urlset(locs: &[&str]) -> String {
        let inner: String = locs
            .iter()
            .map(|l| format!("<url><loc>{l}</loc></url>"))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{inner}</urlset>"#
        )
    }

    fn sitemap_index(children: &[&str]) -> String {
        let inner: String = children
            .iter()
            .map(|l| format!("<sitemap><loc>{l}</loc></sitemap>"))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{inner}</sitemapindex>"#
        )
    }

    fn dto(url: &str) -> MapRequestDto {
        MapRequestDto {
            url: url.to_string(),
            include_patterns: None,
            exclude_patterns: None,
            limit: Some(1000),
        }
    }

    const ORIGIN: &str = "https://a.com";

    // ========== parse_locs ==========

    /// R-map-002：urlset 多 `<loc>` 解析保序。
    #[test]
    fn parse_locs_preserves_order() {
        let xml = urlset(&["https://a.com/1", "https://a.com/2", "https://a.com/3"]);
        assert_eq!(
            parse_locs(&xml),
            vec!["https://a.com/1", "https://a.com/2", "https://a.com/3"]
        );
    }

    /// R-map-002：空 sitemap / 无 loc 返回空。
    #[test]
    fn parse_locs_empty_sitemap() {
        assert!(parse_locs(&urlset(&[])).is_empty());
    }

    // ========== matches_glob ==========

    /// R-map-003：`*`/`?` 通配语义。
    #[test]
    fn matches_glob_semantics() {
        assert!(matches_glob("*/blog/*", "https://a.com/blog/post-1"));
        assert!(!matches_glob("*/blog/*", "https://a.com/page/1"));
        assert!(matches_glob("https://a.com/page/?", "https://a.com/page/1"));
        assert!(!matches_glob(
            "https://a.com/page/?",
            "https://a.com/page/12"
        ));
        assert!(matches_glob("*", "https://anything.example"));
        assert!(matches_glob("https://a.com/*", "https://a.com/"));
    }

    // ========== execute ==========

    /// R-map-002：3 个 loc 的 sitemap 保序返回 3 个 link。
    #[tokio::test]
    async fn execute_returns_locs_in_order() {
        let fetcher = MockFetcher {
            responses: HashMap::from([(
                format!("{ORIGIN}/sitemap.xml"),
                (
                    200u16,
                    urlset(&["https://a.com/1", "https://a.com/2", "https://a.com/3"]),
                ),
            )]),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto(ORIGIN))
            .await
            .expect("execute");
        assert_eq!(
            result.links,
            vec!["https://a.com/1", "https://a.com/2", "https://a.com/3"]
        );
    }

    /// R-map-002：重复 loc 去重保首次位置。
    #[tokio::test]
    async fn execute_dedups_keeping_first_position() {
        let fetcher = MockFetcher {
            responses: HashMap::from([(
                format!("{ORIGIN}/sitemap.xml"),
                (
                    200u16,
                    urlset(&["https://a.com/a", "https://a.com/b", "https://a.com/a"]),
                ),
            )]),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto(ORIGIN))
            .await
            .expect("execute");
        assert_eq!(result.links, vec!["https://a.com/a", "https://a.com/b"]);
    }

    /// R-map-002：sitemapindex 递归一层，两个子 sitemap 共 5 个 loc 全部返回。
    #[tokio::test]
    async fn execute_recurses_sitemap_index_one_level() {
        let fetcher = MockFetcher {
            responses: HashMap::from([
                (
                    format!("{ORIGIN}/sitemap.xml"),
                    (
                        200u16,
                        sitemap_index(&["https://a.com/s1.xml", "https://a.com/s2.xml"]),
                    ),
                ),
                (
                    "https://a.com/s1.xml".to_string(),
                    (
                        200u16,
                        urlset(&["https://a.com/1", "https://a.com/2", "https://a.com/3"]),
                    ),
                ),
                (
                    "https://a.com/s2.xml".to_string(),
                    (200u16, urlset(&["https://a.com/4", "https://a.com/5"])),
                ),
            ]),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto(ORIGIN))
            .await
            .expect("execute");
        assert_eq!(result.links.len(), 5);
        assert_eq!(result.links[0], "https://a.com/1");
        assert_eq!(result.links[4], "https://a.com/5");
    }

    /// R-map-002：子 sitemap 数超过 5 时只取前 5 个。
    #[tokio::test]
    async fn execute_caps_sub_sitemaps_at_five() {
        let children: Vec<String> = (0..7).map(|i| format!("https://a.com/s{i}.xml")).collect();
        let child_refs: Vec<&str> = children.iter().map(|s| s.as_str()).collect();
        let mut responses = HashMap::from([(
            format!("{ORIGIN}/sitemap.xml"),
            (200u16, sitemap_index(&child_refs)),
        )]);
        for i in 0..7 {
            responses.insert(
                format!("https://a.com/s{i}.xml"),
                (200u16, urlset(&[&format!("https://a.com/u{i}")])),
            );
        }
        let fetcher = MockFetcher { responses };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto(ORIGIN))
            .await
            .expect("execute");
        assert_eq!(
            result.links.len(),
            5,
            "only first 5 sub sitemaps must be fetched"
        );
        assert!(result.links.contains(&"https://a.com/u0".to_string()));
        assert!(!result.links.contains(&"https://a.com/u5".to_string()));
    }

    /// R-map-003：include 白名单、exclude 黑名单过滤；同时给时先 include 后 exclude。
    #[tokio::test]
    async fn execute_applies_include_then_exclude_patterns() {
        let fetcher = MockFetcher {
            responses: HashMap::from([(
                format!("{ORIGIN}/sitemap.xml"),
                (
                    200u16,
                    urlset(&[
                        "https://a.com/blog/1",
                        "https://a.com/blog/tag/x",
                        "https://a.com/page/1",
                    ]),
                ),
            )]),
        };
        let mut d = dto(ORIGIN);
        d.include_patterns = Some(vec!["*/blog/*".to_string()]);
        d.exclude_patterns = Some(vec!["*/tag/*".to_string()]);
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&d)
            .await
            .expect("execute");
        assert_eq!(result.links, vec!["https://a.com/blog/1"]);
    }

    /// R-map-003：limit 截断。
    #[tokio::test]
    async fn execute_truncates_to_limit() {
        let fetcher = MockFetcher {
            responses: HashMap::from([(
                format!("{ORIGIN}/sitemap.xml"),
                (
                    200u16,
                    urlset(&["https://a.com/1", "https://a.com/2", "https://a.com/3"]),
                ),
            )]),
        };
        let mut d = dto(ORIGIN);
        d.limit = Some(2);
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&d)
            .await
            .expect("execute");
        assert_eq!(result.links, vec!["https://a.com/1", "https://a.com/2"]);
    }

    /// R-map-002：sitemap 404 → 200 空 links（合法状态，非错误）。
    #[tokio::test]
    async fn execute_returns_empty_links_on_sitemap_404() {
        let fetcher = MockFetcher {
            responses: HashMap::from([(
                format!("{ORIGIN}/sitemap.xml"),
                (404u16, String::from("not found")),
            )]),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto(ORIGIN))
            .await
            .expect("execute");
        assert!(result.links.is_empty());
    }

    /// R-map-004：目标站 5xx → TargetUnreachable（handler 映射 502）。
    #[tokio::test]
    async fn execute_maps_5xx_to_target_unreachable() {
        let fetcher = MockFetcher {
            responses: HashMap::from([(
                format!("{ORIGIN}/sitemap.xml"),
                (500u16, String::from("boom")),
            )]),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto(ORIGIN))
            .await;
        assert!(matches!(result, Err(MapError::TargetUnreachable(_))));
    }

    /// R-map-001：无效 URL（无法解析 origin）→ Internal 错误（DTO 校验是第一道防线）。
    #[tokio::test]
    async fn execute_rejects_unparsable_url() {
        let fetcher = MockFetcher {
            responses: HashMap::new(),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto("not-a-url"))
            .await;
        assert!(matches!(result, Err(MapError::Internal(_))));
    }
}
