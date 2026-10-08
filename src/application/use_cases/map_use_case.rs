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

/// 用例级错误（handler 映射：TargetUnreachable / SitemapStatus →
/// 502 MAP_TARGET_UNREACHABLE；OriginUnparsable / Internal → 400）
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("target unreachable: {0}")]
    TargetUnreachable(String),
    /// sitemap 源返回非 2xx 状态（结构化携带状态码，供 handler 按请求 locale 翻译）
    #[error("sitemap returned status {0}")]
    SitemapStatus(u16),
    /// URL 无法解析出 origin（结构化携带原始 URL，供 handler 翻译）
    #[error("cannot parse origin from {0}")]
    OriginUnparsable(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// sitemap 文档根元素类型
#[cfg(feature = "sitemap")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SitemapKind {
    /// `<urlset>`：`<loc>` 为页面 URL
    Urlset,
    /// `<sitemapindex>`：`<loc>` 为子 sitemap URL（递归一层展开）
    SitemapIndex,
}

/// 流式 sitemap 流：文档类型 + 逐 `<loc>` 产出的有界通道
///
/// 解析端在通道满时暂停（背压），消费端按解析顺序逐条 `recv`。
#[cfg(feature = "sitemap")]
pub struct SitemapStream {
    /// 根元素类型（由首个 Start 事件判定；空文档按 Urlset 处理）
    pub kind: SitemapKind,
    /// `<loc>` 文本流（保序）
    pub rx: tokio::sync::mpsc::Receiver<String>,
}

#[cfg(feature = "sitemap")]
impl SitemapStream {
    /// 立即耗尽的空流（sitemap 404 合法状态）
    pub fn empty() -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx);
        Self {
            kind: SitemapKind::Urlset,
            rx,
        }
    }
}

/// [`MapUseCase::execute_streaming`] 的运行参数（来自 `settings.sitemap`）
#[cfg(feature = "sitemap")]
#[derive(Debug, Clone, Copy)]
pub struct SitemapStreamOptions {
    /// mpsc 背压边界（`sitemap.channel_capacity`）
    pub channel_capacity: usize,
    /// 单次 map 消费上限（`sitemap.max_urls`，防无限站点图）
    pub max_urls: usize,
    /// 子 sitemap 并发展开上限（`sitemap.max_concurrent_sub_sitemaps`）
    pub max_concurrent_sub_sitemaps: usize,
}

/// sitemap 抓取端口（测试用 mock 替换）
#[async_trait]
pub trait SitemapFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<SitemapFetch, MapError>;

    /// 流式拉取：解析端遇 `<loc>` 即送入容量 `channel_capacity` 的有界通道，
    /// 通道满即 await（背压）。
    ///
    /// 语义：404 → `Ok(空流)`（无 sitemap 是合法状态）；其余 ≥400 / 传输失败
    /// → `Err(TargetUnreachable)`。
    ///
    /// 默认实现为整串 fetch + 一次解析的回退路径；生产实现
    /// （[`EngineSitemapFetcher`]）覆盖为 quick-xml 增量解析。
    #[cfg(feature = "sitemap")]
    async fn fetch_streaming(
        &self,
        url: &str,
        channel_capacity: usize,
    ) -> Result<SitemapStream, MapError> {
        let fetched = self.fetch(url).await?;
        if fetched.status == 404 {
            return Ok(SitemapStream::empty());
        }
        if fetched.status >= 400 {
            return Err(MapError::SitemapStatus(fetched.status));
        }
        let kind = if is_sitemap_index(&fetched.body) {
            SitemapKind::SitemapIndex
        } else {
            SitemapKind::Urlset
        };
        let (tx, rx) = tokio::sync::mpsc::channel(channel_capacity.max(1));
        tokio::spawn(async move {
            for loc in parse_locs(&fetched.body) {
                if tx.send(loc).await.is_err() {
                    break;
                }
            }
        });
        Ok(SitemapStream { kind, rx })
    }
}

/// 生产实现：经 EngineClient（reqwest 引擎）抓取
///
/// `timeout` 来自 `settings.sitemap.fetch_timeout_secs`（单文档拉取超时）。
pub struct EngineSitemapFetcher {
    client: Arc<EngineClient>,
    timeout: std::time::Duration,
}

impl EngineSitemapFetcher {
    pub fn new(client: Arc<EngineClient>, timeout: std::time::Duration) -> Self {
        Self { client, timeout }
    }
}

#[async_trait]
impl SitemapFetcher for EngineSitemapFetcher {
    async fn fetch(&self, url: &str) -> Result<SitemapFetch, MapError> {
        let request = ScrapeRequest::new(url.to_string()).timeout(self.timeout);
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

    /// quick-xml 增量解析：遇 `<loc>` 即 `send`，通道满即 await（背压）。
    /// XML 中途解析失败时保留已产出的部分结果（`warn!` 显性化后终止流），
    /// 不因尾部畸形标签丢弃整份 sitemap。
    #[cfg(feature = "sitemap")]
    async fn fetch_streaming(
        &self,
        url: &str,
        channel_capacity: usize,
    ) -> Result<SitemapStream, MapError> {
        use quick_xml::events::Event;

        let request = ScrapeRequest::new(url.to_string()).timeout(self.timeout);
        let response = self
            .client
            .scrape(&request)
            .await
            .map_err(|e| MapError::TargetUnreachable(e.to_string()))?;
        if response.status_code == 404 {
            return Ok(SitemapStream::empty());
        }
        if response.status_code >= 400 {
            return Err(MapError::SitemapStatus(response.status_code));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(channel_capacity.max(1));
        let mut reader = quick_xml::Reader::from_str(&response.content);
        let mut kind: Option<SitemapKind> = None;
        let mut in_loc = false;
        let mut loc_text = String::new();
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = e.name().local_name();
                    if kind.is_none() {
                        kind = Some(if name.as_ref().eq_ignore_ascii_case("sitemapindex") {
                            SitemapKind::SitemapIndex
                        } else {
                            SitemapKind::Urlset
                        });
                    }
                    if name.as_ref().eq_ignore_ascii_case("loc") {
                        in_loc = true;
                        loc_text.clear();
                    }
                }
                Ok(Event::Text(t)) if in_loc => {
                    match quick_xml::escape::unescape(&t.xml10_content()) {
                        Ok(unescaped) => loc_text.push_str(&unescaped),
                        Err(e) => {
                            log::warn!("sitemap loc entity unescape failed: {e}");
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    if in_loc && e.name().local_name().as_ref().eq_ignore_ascii_case("loc") {
                        in_loc = false;
                        let loc = loc_text.trim();
                        if !loc.is_empty() && tx.send(loc.to_string()).await.is_err() {
                            // 消费端已关闭：停止解析
                            break;
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => {
                    log::warn!(
                        "sitemap XML parse error: {e}; keeping partial results already produced"
                    );
                    break;
                }
            }
        }

        Ok(SitemapStream {
            kind: kind.unwrap_or(SitemapKind::Urlset),
            rx,
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
        let origin =
            extract_origin(&dto.url).ok_or_else(|| MapError::OriginUnparsable(dto.url.clone()))?;

        let first = self.fetcher.fetch(&format!("{origin}/sitemap.xml")).await?;
        if first.status == 404 {
            // 站点无 sitemap 是合法状态：空结果而非错误
            return Ok(MapResult { links: Vec::new() });
        }
        if first.status >= 400 {
            return Err(MapError::SitemapStatus(first.status));
        }

        let mut locs = parse_locs(&first.body);
        if is_sitemap_index(&first.body) {
            // 递归一层：取前 MAX_SUB_SITEMAPS 个子 sitemap 展开
            let children: Vec<String> = locs.iter().take(MAX_SUB_SITEMAPS).cloned().collect();
            locs.clear();
            for child in children {
                // 连接级瞬态错误（如 keep-alive 连接被对端关闭）重试一次，
                // 仍失败则按既有语义放弃该子 sitemap 的内容（不计为整体失败）
                let fetched = match self.fetcher.fetch(&child).await {
                    Ok(f) => Some(f),
                    // 连接级瞬态错误重试一次（ok() 语义：仍失败则放弃该子 sitemap）
                    Err(MapError::TargetUnreachable(_)) | Err(MapError::SitemapStatus(_)) => {
                        self.fetcher.fetch(&child).await.ok()
                    }
                    Err(e) => return Err(e),
                };
                if let Some(fetched) = fetched {
                    if fetched.status == 200 {
                        locs.extend(parse_locs(&fetched.body));
                    }
                }
            }
        }

        Ok(MapResult {
            links: finalize_links(locs, dto),
        })
    }

    /// 流式执行（F11：sitemap 一等请求源的产出口）
    ///
    /// 从 `{origin}/sitemap.xml` 流式解析并经有界通道逐条产出 `<loc>`：
    /// - `Urlset`：loc 直接转发，`max_urls` 为消费上限护栏
    /// - `SitemapIndex`：递归一层，取前 [`MAX_SUB_SITEMAPS`] 个子 sitemap，
    ///   并发展开 ≤ `options.max_concurrent_sub_sitemaps`；子 sitemap 瞬态
    ///   失败重试一次仍失败则放弃该子 sitemap（对齐 [`Self::execute`] 语义）
    ///
    /// 返回的 [`SitemapStream`] 携带根元素类型（消费方的断点 seek 决策依赖：
    /// Urlset 流保序可精确 seek，Index 流并发展开顺序不定只能靠去重兜底），
    /// `rx` 供消费方逐条处理（HTTP handler 聚合为 [`MapResult`]；请求源路径
    /// 可直接消费做 dedup/过滤/入队）。过滤（dedup/include/exclude/limit）
    /// 由消费方按需应用（HTTP 路径用 [`finalize_links`]）。
    #[cfg(feature = "sitemap")]
    pub async fn execute_streaming(
        &self,
        dto: &MapRequestDto,
        options: SitemapStreamOptions,
    ) -> Result<SitemapStream, MapError> {
        use tokio::sync::mpsc;
        use tokio::task::JoinSet;

        let origin =
            extract_origin(&dto.url).ok_or_else(|| MapError::OriginUnparsable(dto.url.clone()))?;

        let mut first = self
            .fetcher
            .fetch_streaming(&format!("{origin}/sitemap.xml"), options.channel_capacity)
            .await?;

        match first.kind {
            SitemapKind::Urlset => Ok(SitemapStream {
                kind: SitemapKind::Urlset,
                rx: forward_capped(first.rx, options),
            }),
            SitemapKind::SitemapIndex => {
                // 递归一层：取前 MAX_SUB_SITEMAPS 个子 sitemap
                let mut children: Vec<String> = Vec::new();
                while children.len() < MAX_SUB_SITEMAPS {
                    match first.rx.recv().await {
                        Some(child) => children.push(child),
                        None => break,
                    }
                }

                let (tx, rx) = mpsc::channel(options.channel_capacity.max(1));
                let semaphore = Arc::new(tokio::sync::Semaphore::new(
                    options.max_concurrent_sub_sitemaps.max(1),
                ));
                let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let mut set: JoinSet<()> = JoinSet::new();
                for child in children {
                    let fetcher = self.fetcher.clone();
                    let tx = tx.clone();
                    let semaphore = semaphore.clone();
                    let sent = sent.clone();
                    set.spawn(async move {
                        let _permit = semaphore
                            .acquire_owned()
                            .await
                            .expect("semaphore has capacity");
                        // 连接级瞬态错误重试一次，仍失败放弃该子 sitemap
                        // （ok() 语义，对齐 execute 的子 sitemap 处理）
                        let stream = match fetcher
                            .fetch_streaming(&child, options.channel_capacity)
                            .await
                        {
                            Ok(s) => Some(s),
                            Err(MapError::TargetUnreachable(_))
                            | Err(MapError::SitemapStatus(_)) => fetcher
                                .fetch_streaming(&child, options.channel_capacity)
                                .await
                                .ok(),
                            Err(_) => None,
                        };
                        if let Some(stream) = stream {
                            let mut rx = stream.rx;
                            while let Some(loc) = rx.recv().await {
                                // 消费上限护栏：check 与 increment 非原子，
                                // 并发下最多超发 ≤ 并发数条——护栏安全方向
                                if sent.load(std::sync::atomic::Ordering::Relaxed)
                                    >= options.max_urls
                                {
                                    return;
                                }
                                if tx.send(loc).await.is_err() {
                                    return;
                                }
                                sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    });
                }
                drop(tx);
                // 全部子任务结束后 tx 归零，rx 端 recv 返回 None
                tokio::spawn(async move { while set.join_next().await.is_some() {} });
                Ok(SitemapStream {
                    kind: SitemapKind::SitemapIndex,
                    rx,
                })
            }
        }
    }
}

/// Urlset 流的消费上限转发：最多转发 `max_urls` 条（防无限站点图）
#[cfg(feature = "sitemap")]
fn forward_capped(
    mut rx: tokio::sync::mpsc::Receiver<String>,
    options: SitemapStreamOptions,
) -> tokio::sync::mpsc::Receiver<String> {
    let (tx, out) = tokio::sync::mpsc::channel(options.channel_capacity.max(1));
    tokio::spawn(async move {
        let mut sent = 0usize;
        while sent < options.max_urls {
            match rx.recv().await {
                Some(loc) => {
                    if tx.send(loc).await.is_err() {
                        break;
                    }
                    sent += 1;
                }
                None => break,
            }
        }
    });
    out
}

/// 流式 loc 序列的聚合后处理：include 白名单 → exclude 黑名单 → 去重（保首次
/// 出现位置）→ limit 截断（R-map-003）
///
/// include/exclude 是纯谓词，先过滤后去重输出语义不变，但去重集合从全量
/// locs（max_urls 默认 5 万）缩小到命中白名单的子集。[`MapUseCase::execute`]
/// 与 map_handler 聚合路径共用，保证两条路径的响应语义一致。
pub fn finalize_links(locs: Vec<String>, dto: &MapRequestDto) -> Vec<String> {
    // 先 include 白名单后 exclude 黑名单（R-map-003）
    let mut links = locs;
    if let Some(include) = &dto.include_patterns {
        links.retain(|u| include.iter().any(|p| matches_glob(p, u)));
    }
    if let Some(exclude) = &dto.exclude_patterns {
        links.retain(|u| !exclude.iter().any(|p| matches_glob(p, u)));
    }

    // 去重（保首次出现位置）
    let mut seen: HashSet<String> = HashSet::with_capacity(links.len());
    links.retain(|u| seen.insert(u.clone()));

    // limit 截断
    let limit = dto.limit.unwrap_or(1000) as usize;
    links.truncate(limit);
    links
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

/// `*`（任意长度）/`?`（单字符）通配匹配（复用 [`crate::utils::glob`] 单实现）
///
/// 注意语义差异：`workers/crawl/filters.rs` 的 `match_pattern` 是
/// regex 优先 + contains 回退，非 glob——两者不可混用。
pub fn matches_glob(pattern: &str, text: &str) -> bool {
    crate::utils::glob::glob_match(pattern, text)
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

    /// R-map-004：目标站 5xx → SitemapStatus（handler 映射 502 MAP_TARGET_UNREACHABLE）。
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
        assert!(matches!(result, Err(MapError::SitemapStatus(_))));
    }

    /// R-map-001：无效 URL（无法解析 origin）→ OriginUnparsable 错误（DTO 校验是第一道防线）。
    #[tokio::test]
    async fn execute_rejects_unparsable_url() {
        let fetcher = MockFetcher {
            responses: HashMap::new(),
        };
        let result = MapUseCase::new(Arc::new(fetcher))
            .execute(&dto("not-a-url"))
            .await;
        assert!(matches!(result, Err(MapError::OriginUnparsable(_))));
    }

    // ========== execute_streaming（F11 流式路径，sitemap feature） ==========

    #[cfg(feature = "sitemap")]
    mod streaming_tests {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn stream_options(
            capacity: usize,
            max_urls: usize,
            concurrency: usize,
        ) -> SitemapStreamOptions {
            SitemapStreamOptions {
                channel_capacity: capacity,
                max_urls,
                max_concurrent_sub_sitemaps: concurrency,
            }
        }

        async fn collect(rx: tokio::sync::mpsc::Receiver<String>) -> Vec<String> {
            let mut rx = rx;
            let mut out = Vec::new();
            while let Some(loc) = rx.recv().await {
                out.push(loc);
            }
            out
        }

        /// Urlset 流经 execute_streaming 逐条产出且保序（万级 loc）
        #[tokio::test]
        async fn streaming_urlset_yields_ten_thousand_locs_in_order() {
            let locs: Vec<String> = (0..10_000).map(|i| format!("https://a.com/{i}")).collect();
            let refs: Vec<&str> = locs.iter().map(String::as_str).collect();
            let fetcher = MockFetcher {
                responses: HashMap::from([(
                    format!("{ORIGIN}/sitemap.xml"),
                    (200u16, urlset(&refs)),
                )]),
            };
            let rx = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(64, 50_000, 4))
                .await
                .expect("execute_streaming");
            assert_eq!(collect(rx.rx).await, locs, "万级 loc 必须逐条且保序产出");
        }

        /// sitemap 404 → 立即耗尽的空流（合法状态，非错误）
        #[tokio::test]
        async fn streaming_sitemap_404_yields_empty_stream() {
            let fetcher = MockFetcher {
                responses: HashMap::from([(
                    format!("{ORIGIN}/sitemap.xml"),
                    (404u16, String::from("not found")),
                )]),
            };
            let rx = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(4, 100, 2))
                .await
                .expect("404 must be Ok (empty stream)");
            assert!(collect(rx.rx).await.is_empty());
        }

        /// 目标站 5xx / 拉取超时 → SitemapStatus（错误显性化）
        #[tokio::test]
        async fn streaming_maps_5xx_to_target_unreachable() {
            let fetcher = MockFetcher {
                responses: HashMap::from([(
                    format!("{ORIGIN}/sitemap.xml"),
                    (500u16, String::from("boom")),
                )]),
            };
            let result = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(4, 100, 2))
                .await;
            assert!(matches!(result, Err(MapError::SitemapStatus(_))));
        }

        /// max_urls 消费上限：100 条 loc 只产出前 10 条
        #[tokio::test]
        async fn streaming_respects_max_urls_cap() {
            let locs: Vec<String> = (0..100).map(|i| format!("https://a.com/{i}")).collect();
            let refs: Vec<&str> = locs.iter().map(String::as_str).collect();
            let fetcher = MockFetcher {
                responses: HashMap::from([(
                    format!("{ORIGIN}/sitemap.xml"),
                    (200u16, urlset(&refs)),
                )]),
            };
            let rx = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(4, 10, 2))
                .await
                .expect("execute_streaming");
            let got = collect(rx.rx).await;
            assert_eq!(got.len(), 10, "max_urls=10 must cap the stream at 10");
            assert_eq!(got[0], "https://a.com/0");
            assert_eq!(got[9], "https://a.com/9");
        }

        /// 背压：消费端暂停时，生产端完成量受通道容量约束
        /// （两跳通道的在途上界 = 生产通道 capacity + 转发通道 capacity + 1 手持）
        struct BackpressureFetcher {
            total: usize,
            sent: Arc<AtomicUsize>,
        }

        #[async_trait]
        impl SitemapFetcher for BackpressureFetcher {
            async fn fetch(&self, _url: &str) -> Result<SitemapFetch, MapError> {
                Err(MapError::Internal("unused".to_string()))
            }

            async fn fetch_streaming(
                &self,
                _url: &str,
                channel_capacity: usize,
            ) -> Result<SitemapStream, MapError> {
                let (tx, rx) = tokio::sync::mpsc::channel(channel_capacity.max(1));
                let sent = self.sent.clone();
                let total = self.total;
                tokio::spawn(async move {
                    for i in 0..total {
                        if tx.send(format!("https://a.com/{i}")).await.is_err() {
                            break;
                        }
                        sent.fetch_add(1, Ordering::SeqCst);
                    }
                });
                Ok(SitemapStream {
                    kind: SitemapKind::Urlset,
                    rx,
                })
            }
        }

        #[tokio::test]
        async fn streaming_backpressure_bounds_producer_progress() {
            const TOTAL: usize = 10_000;
            const CAPACITY: usize = 4;
            let sent = Arc::new(AtomicUsize::new(0));
            let fetcher = BackpressureFetcher {
                total: TOTAL,
                sent: sent.clone(),
            };
            let rx = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(CAPACITY, TOTAL, 2))
                .await
                .expect("execute_streaming");

            // 消费端暂停：生产端被背压阻塞，完成量远小于总量
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let stalled = sent.load(Ordering::SeqCst);
            assert!(
                stalled <= 2 * CAPACITY + 1,
                "consumer paused: producer must be bounded by channel capacity, \
                 sent={stalled} (cap={CAPACITY})"
            );

            // 恢复消费：全部 10_000 条按序产出
            let got = collect(rx.rx).await;
            assert_eq!(got.len(), TOTAL, "drain must yield all locs");
            assert_eq!(
                sent.load(Ordering::SeqCst),
                TOTAL,
                "producer must complete after drain"
            );
        }

        /// sitemapindex：并发展开 ≤ max_concurrent_sub_sitemaps、递归一层
        /// 只取前 MAX_SUB_SITEMAPS 个子 sitemap、子 sitemap 失败放弃不计整体失败
        struct IndexMockFetcher {
            /// 子 sitemap url → (status, body)
            children: HashMap<String, (u16, String)>,
            /// 根 index 列出的子 sitemap（有序）
            root_children: Vec<String>,
            in_flight: Arc<AtomicUsize>,
            max_in_flight: Arc<AtomicUsize>,
            child_delay_ms: u64,
        }

        impl IndexMockFetcher {
            fn record_enter(&self) {
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            }
        }

        #[async_trait]
        impl SitemapFetcher for IndexMockFetcher {
            async fn fetch(&self, _url: &str) -> Result<SitemapFetch, MapError> {
                Err(MapError::Internal("unused".to_string()))
            }

            async fn fetch_streaming(
                &self,
                url: &str,
                channel_capacity: usize,
            ) -> Result<SitemapStream, MapError> {
                const ROOT: &str = "https://a.com/sitemap.xml";
                if url == ROOT {
                    let (tx, rx) = tokio::sync::mpsc::channel(channel_capacity.max(1));
                    let children = self.root_children.clone();
                    tokio::spawn(async move {
                        for child in children {
                            if tx.send(child).await.is_err() {
                                break;
                            }
                        }
                    });
                    return Ok(SitemapStream {
                        kind: SitemapKind::SitemapIndex,
                        rx,
                    });
                }

                let (status, body) = match self.children.get(url) {
                    Some((s, b)) => (*s, b.clone()),
                    None => return Err(MapError::TargetUnreachable(format!("no mock for {url}"))),
                };
                if status == 404 {
                    return Ok(SitemapStream::empty());
                }
                if status >= 400 {
                    return Err(MapError::TargetUnreachable(format!(
                        "child returned status {status}"
                    )));
                }
                let (tx, rx) = tokio::sync::mpsc::channel(channel_capacity.max(1));
                self.record_enter();
                let in_flight = self.in_flight.clone();
                let delay = self.child_delay_ms;
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    for loc in parse_locs(&body) {
                        if tx.send(loc).await.is_err() {
                            break;
                        }
                    }
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                });
                Ok(SitemapStream {
                    kind: SitemapKind::Urlset,
                    rx,
                })
            }
        }

        #[tokio::test]
        async fn streaming_index_recursion_caps_concurrency_and_children() {
            // 7 个子 sitemap（超出 MAX_SUB_SITEMAPS=5 只取前 5），
            // 其中第 2 个（index 1）500 且重试仍 500 → 放弃
            let mut children = HashMap::new();
            let mut root_children = Vec::new();
            for i in 0..7 {
                let url = format!("https://a.com/s{i}.xml");
                let status = if i == 1 { 500u16 } else { 200u16 };
                let body = urlset(&[&format!("https://a.com/u{i}")]);
                children.insert(url.clone(), (status, body));
                root_children.push(url);
            }
            let in_flight = Arc::new(AtomicUsize::new(0));
            let max_in_flight = Arc::new(AtomicUsize::new(0));
            let fetcher = IndexMockFetcher {
                children,
                root_children,
                in_flight: in_flight.clone(),
                max_in_flight: max_in_flight.clone(),
                child_delay_ms: 50,
            };
            let rx = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(4, 100, 2))
                .await
                .expect("execute_streaming");

            let got = collect(rx.rx).await;
            // 子 sitemap 0、2、3、4 命中（1 失败放弃，5、6 超出上限不取）
            assert_eq!(
                got,
                vec![
                    "https://a.com/u0",
                    "https://a.com/u2",
                    "https://a.com/u3",
                    "https://a.com/u4",
                ],
                "first 5 children expanded, failed child skipped"
            );
            let peak = max_in_flight.load(Ordering::SeqCst);
            assert!(
                peak >= 2,
                "children must be fetched concurrently (observed peak {peak})"
            );
            assert!(
                peak <= 2,
                "child fetch concurrency must not exceed max_concurrent_sub_sitemaps=2, got {peak}"
            );
        }

        /// 空 sitemapindex（无子 sitemap）→ 立即耗尽的空流
        #[tokio::test]
        async fn streaming_empty_index_yields_empty_stream() {
            let fetcher = MockFetcher {
                responses: HashMap::from([(
                    format!("{ORIGIN}/sitemap.xml"),
                    (200u16, sitemap_index(&[])),
                )]),
            };
            let rx = MapUseCase::new(Arc::new(fetcher))
                .execute_streaming(&dto(ORIGIN), stream_options(4, 100, 2))
                .await
                .expect("execute_streaming");
            assert!(collect(rx.rx).await.is_empty());
        }

        /// finalize_links：流式 loc 聚合语义与 execute() 一致
        /// （去重保首现位置 → include → exclude → limit）
        #[test]
        fn finalize_links_matches_execute_semantics() {
            let mut d = dto(ORIGIN);
            d.include_patterns = Some(vec!["*/blog/*".to_string()]);
            d.exclude_patterns = Some(vec!["*/tag/*".to_string()]);
            d.limit = Some(2);
            let links = finalize_links(
                vec![
                    "https://a.com/blog/1".to_string(),
                    "https://a.com/blog/1".to_string(),     // 重复
                    "https://a.com/blog/tag/x".to_string(), // exclude
                    "https://a.com/page/1".to_string(),     // include 不命中
                    "https://a.com/blog/2".to_string(),
                    "https://a.com/blog/3".to_string(),
                ],
                &d,
            );
            assert_eq!(links, vec!["https://a.com/blog/1", "https://a.com/blog/2"]);
        }
    }
}
