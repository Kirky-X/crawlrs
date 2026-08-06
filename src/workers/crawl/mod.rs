// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 深度爬取模块（design.md §15/§16，Stage4）
//!
//! 参考 crawl4ai `deep_crawling/{filters,scorers,frontier}.py` 与
//! `adaptive_crawler.py`，提供 URL 过滤、评分、优先级队列与自适应停止条件。
//!
//! # 模块组成
//!
//! - [`filters`]：URL 过滤器 trait + FilterChain + 三个具体 filter（T063）
//! - [`scorers`]：URL 评分器 trait + CompositeScorer + 两个具体 scorer（T064）
//! - [`frontier`]：优先级队列 ScoredUrl + 域名 round-robin（T065）
//! - [`adaptive`]：自适应策略 + 停止条件（T067）

pub mod adaptive;
/// DRL 自适应爬取策略（T083-T087）
///
/// ONNX 模型推理 + 启发式退化。
pub mod drl_policy;
pub mod filters;
pub mod frontier;
/// 知识图谱覆盖感知爬取（T077-T082）
///
/// 爬取过程中构建 KG，用 Chao1 估计覆盖率，
/// 结构空洞检测指导 URL 优先级。
pub mod knowledge_graph;
pub mod scorers;

use std::sync::Arc;

/// URL 过滤器 trait（T063，R-frontier-002）
///
/// 实现者定义 URL 是否被接受。[`FilterChain`] 串联多个 filter，
/// 全部 `accept` 才放行（AND 语义）。
///
/// # 实现要求
///
/// - 必须线程安全（`Send + Sync`），FilterChain 可被多 worker 共享
/// - 决策幂等：同一 `(url, context)` 输入应返回相同结果
/// - 不修改输入：`url` 与 `context` 以只读引用传入
pub trait UrlFilter: Send + Sync {
    /// 判定 URL 是否被接受
    ///
    /// # 参数
    ///
    /// - `url`: 待判定的 URL（已归一化）
    /// - `context`: 过滤上下文（源页面 content-type / 域名等辅助信息）
    ///
    /// # 返回
    ///
    /// - `true`: 接受该 URL（继续后续 filter 或最终放行）
    /// - `false`: 拒绝该 URL（FilterChain 立即短路返回 false）
    fn accept(&self, url: &str, context: &FilterContext) -> bool;
}

/// URL 评分器 trait（T064，R-frontier-003）
///
/// 实现者定义 URL 的相关性分数，[`scorers::CompositeScorer`] 加权聚合多个 scorer。
/// 分数归一化到 `[0.0, 1.0]`，越高表示越相关，应优先出队。
///
/// # 实现要求
///
/// - 必须线程安全（`Send + Sync`），CompositeScorer 可被多 worker 共享
/// - 决策幂等：同一 `(url, context)` 输入应返回相同分数
/// - 分数归一化：返回值必须落在 `[0.0, 1.0]` 区间
///   - `0.0`: 完全不相关
///   - `0.5`: 中性（如空关键词列表无法判定相关性）
///   - `1.0`: 完全相关
pub trait UrlScorer: Send + Sync {
    /// 计算 URL 的相关性分数
    ///
    /// # 参数
    ///
    /// - `url`: 待评分的 URL（已归一化）
    /// - `context`: 评分上下文（关键词列表等辅助信息）
    ///
    /// # 返回
    ///
    /// 归一化分数 `[0.0, 1.0]`，高=更相关
    fn score(&self, url: &str, context: &ScoringContext) -> f32;
}

/// 过滤上下文（T063）
///
/// 提供 filter 决策所需的辅助信息，避免每个 filter 自行解析 URL 或重复查询。
///
/// # 字段说明
///
/// - `source_content_type`: 源页面（即发现该 URL 的页面）的 Content-Type。
///   [`filters::ContentTypeFilter`] 用此判定是否跨类型跳转（如从 text/html 跳到 application/pdf）。
/// - `source_domain`: 源页面域名。[`filters::DomainFilter`] 用此判定是否跨域。
/// - `link_text`: 链接文本（如 `<a href="...">link_text</a>`）。
///   [`filters::UrlPatternFilter`] 在 regex 失败回退时可用作辅助判定（当前未使用，预留扩展）。
#[derive(Debug, Clone, Default)]
pub struct FilterContext {
    /// 源页面的 Content-Type（如 `text/html; charset=utf-8`）
    pub source_content_type: Option<String>,
    /// 源页面的域名（如 `example.com`）
    pub source_domain: Option<String>,
    /// 链接文本（`<a>` 标签的文本内容）
    pub link_text: Option<String>,
}

impl FilterContext {
    /// 构造空上下文（所有字段为 None）
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置源页面 Content-Type
    #[must_use]
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.source_content_type = Some(content_type.into());
        self
    }

    /// 设置源页面域名
    #[must_use]
    pub fn with_source_domain(mut self, domain: impl Into<String>) -> Self {
        self.source_domain = Some(domain.into());
        self
    }

    /// 设置链接文本
    #[must_use]
    pub fn with_link_text(mut self, text: impl Into<String>) -> Self {
        self.link_text = Some(text.into());
        self
    }
}

/// 评分上下文（T064）
///
/// 提供 scorer 决策所需的辅助信息。
///
/// # 字段说明
///
/// - `keywords`: 关键词列表（用于 [`scorers::KeywordRelevanceScorer`]）。
///   通常由 CrawlConfigDto 的 `keywords` 字段或 LLM 查询扩展生成。
/// - `source_url`: 源页面 URL（预留扩展，用于上下文相关评分，如 source 同 path 前缀加分）。
#[derive(Debug, Clone, Default)]
pub struct ScoringContext {
    /// 关键词列表
    pub keywords: Vec<String>,
    /// 源页面 URL（预留扩展）
    pub source_url: Option<String>,
}

impl ScoringContext {
    /// 构造空上下文（无关键词，无源 URL）
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置关键词列表
    #[must_use]
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    /// 设置源页面 URL
    #[must_use]
    pub fn with_source_url(mut self, url: impl Into<String>) -> Self {
        self.source_url = Some(url.into());
        self
    }
}

/// 过滤器链（T063，R-frontier-002）
///
/// 串联多个 [`UrlFilter`]，全部 `accept` 才放行（AND 语义）。
/// 任一 filter 返回 `false` 立即短路返回 `false`，不调用后续 filter。
///
/// # 线程安全
///
/// 内部用 `Vec<Arc<dyn UrlFilter>>` 共享 filter 实例，`FilterChain` 自身是 `Clone`
/// （仅 clone Arc 指针），可被多 worker 共享。
///
/// # 示例
///
/// ```ignore
/// use crate::workers::crawl::{FilterChain, FilterContext, filters::{DomainFilter, UrlPatternFilter}};
///
/// let chain = FilterChain::new()
///     .with_filter(DomainFilter::same_domain("example.com"))
///     .with_filter(UrlPatternFilter::new(vec!["/blog/.*".to_string()], vec!["/admin/.*".to_string()]));
///
/// let ctx = FilterContext::new().with_source_domain("example.com");
/// assert!(chain.accept("https://example.com/blog/post-1", &ctx));
/// assert!(!chain.accept("https://example.com/admin/panel", &ctx));
/// assert!(!chain.accept("https://other.com/blog/post-1", &ctx));
/// ```
#[derive(Clone, Default)]
pub struct FilterChain {
    filters: Vec<Arc<dyn UrlFilter>>,
}

impl FilterChain {
    /// 构造空链（无 filter 时默认放行所有 URL）
    #[must_use]
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// 追加一个 filter，返回 `self`（builder 模式）
    #[must_use]
    pub fn with_filter(mut self, filter: impl UrlFilter + 'static) -> Self {
        self.filters.push(Arc::new(filter));
        self
    }

    /// 追加一个已擦除类型的 filter（用于动态拼装）
    #[must_use]
    pub fn with_shared_filter(mut self, filter: Arc<dyn UrlFilter>) -> Self {
        self.filters.push(filter);
        self
    }

    /// 当前链是否为空（空链等价于"放行所有"）
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// 链中 filter 数量
    #[must_use]
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// 依次调用链中 filter，全部 `accept` 才放行（AND 语义，短路求值）
    ///
    /// 空链返回 `true`（默认放行）。
    pub fn accept(&self, url: &str, context: &FilterContext) -> bool {
        self.filters.iter().all(|f| f.accept(url, context))
    }
}

pub use filters::{ContentTypeFilter, DomainFilter, UrlPatternFilter};
pub use frontier::{Frontier, FrontierError, ScoredUrl};
pub use scorers::{CompositeScorer, KeywordRelevanceScorer, PathDepthScorer};
