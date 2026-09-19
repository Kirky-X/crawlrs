// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

/// AIMD 自适应并发控制
///
/// Additive Increase / Multiplicative Decrease 拥塞控制算法，动态调整并发上限。
pub mod adaptive_concurrency;
pub mod backoff;
/// 请求合并 Coalesce
///
/// 同 URL 并发请求只允许首个执行抓取，其余等待后从缓存/DB 读取。
pub mod coalesce;
pub mod crawl_text_integration;
/// URL 分层去重
///
/// UrlNormalizer + Bloom + UrlInterner 三层组合：
/// - L1: Bloom 预筛（mmap+MAP_HUGETLB，1M URLs ~1.2MB，假阳性 <1%）
/// - L2: HashSet 精确校验（hashbrown::HashSet<String>）
/// - L3: DB（scrape_worker.find_existing_urls）保权威
///
/// Bloom 阴性 → 绝对新（无假阴性）；Bloom 阳性 → 回落 DB 校验。
pub mod dedup;
pub mod error_helpers;
/// Hedge 请求副本控制器
///
/// EMA + 方差估 P84 延迟阈值，超阈值时建议发送副本请求降尾延迟。
pub mod hedge;
/// 工具模块
///
/// 提供通用的工具函数和辅助功能
/// 包括文本处理、URL工具、错误处理等功能
pub mod http_client;
pub mod port_sniffer;
/// 代理 URL 工具：校验 + 脱敏
///
/// 提供 `validate_proxy_url`（命令行参数注入防护）和 `redact_proxy_url`（日志凭证脱敏）。
/// 跨引擎共享（playwright / flare_solverr 等）。
pub mod proxy;
pub mod regex_cache;
pub mod retry;
pub mod retry_policy;
pub mod robots;
pub mod search_test;
pub mod telemetry;
pub mod text_processing;
/// UA Pool — 一致性 User-Agent / Header / Viewport 绑定池
pub mod ua_pool;
/// URL 处理工具（SafeUrl / resolve_url / UrlError）
pub mod url;
/// URL 归一化器
///
/// 等价 URL 归一为同一规范串，配合 dedup 模块做分层去重。
pub mod url_normalizer;

// 向后兼容的重新导出 - 已清理，只保留结构体
pub use crate::utils::text_processing::{
    CrawlProcessingError, CrawlTextProcessor, ProcessedCrawlContent, ProcessedWebContent,
    TextEncodingError, WebContentError, WebContentProcessor,
};

pub use crate::utils::url::{resolve_url, SafeUrl, UrlError};
