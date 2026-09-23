// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Markdown 后处理器
//!
//! 从 [`crate::workers::scrape_worker::ScrapeWorker`] 抽取的 markdown 后处理逻辑，
//! 遵循 SRP：ScrapeWorker 专注任务调度，markdown 转换由本类型独立负责。
//!
//! # 职责
//!
//! 根据任务请求的 `formats` 字段判断是否需要生成 Markdown，
//! 命中 `"markdown"` 则通过注入的 `MarkdownServiceTrait` 转换 HTML → Markdown。
//!
//! # 依赖倒置（DIP）
//!
//! 本类型通过 `Arc<dyn MarkdownServiceTrait>` 注入转换服务，而非硬编码
//! `HtmdMarkdownService`，便于测试时 mock 替换，未来扩展其他实现时无需修改源码。
//!
//! # 特性门控
//!
//! 本模块整体 gated `markdown`（与 [`crate::domain::services::markdown_service`] 一致）。
//! 关闭 `markdown` 特性时本模块不编译，ScrapeWorker 也不持有本类型的字段。

#![cfg(feature = "content")]

use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::domain::services::markdown_service::{MarkdownError, MarkdownServiceTrait};
use log::{debug, warn};
use std::sync::Arc;
use uuid::Uuid;

/// Markdown 后处理错误（错误显性化）
///
/// 原实现 `generate()` 返回 `Option<String>`，将三类不同语义的 `None` 混淆：
/// 1. 未请求 markdown（非错误，应返回 `Ok(None)`）
/// 2. 转换失败（应返回 `Err(ConversionFailed)`）
/// 3. 转换结果为空（应返回 `Err(EmptyResult)`）
///
/// 现通过显式错误类型让调用方区分语义，符合“失败必须显性化”原则。
/// 调用方可根据业务策略决定是否阻断主流程（markdown 为增强字段，
/// 失败不阻断基础抓取结果，调用方应记录错误并继续）。
///
/// # 类型化错误传递（#7）
///
/// `ConversionFailed` 使用 `#[from] MarkdownError` 保留下层错误类型，
/// 调用方可通过 `downcast_ref::<MarkdownError>()` 拿到原始错误做精细处理，
/// 而非仅拿到字符串化后的错误消息。
#[derive(Debug, thiserror::Error)]
pub enum MarkdownPostProcessorError {
    /// HTML → Markdown 转换失败（底层 `MarkdownServiceTrait::to_markdown` 返回错误）
    ///
    /// 通过 `#[from]` 自动实现 `From<MarkdownError>`，保留下层错误类型信息。
    #[error("markdown conversion failed: {0}")]
    ConversionFailed(#[from] MarkdownError),
    /// 转换结果为空（HTML 解析后无可见文本，可能是空 HTML 或纯空白内容）
    #[error("markdown conversion returned empty result")]
    EmptyResult,
}

/// Markdown 后处理器
///
/// 持有注入的 `MarkdownServiceTrait` 实例（通过 `Arc` 共享），可安全在多处共享单例。
///
/// # 用法
///
/// ```no_run
/// # use crawlrs::application::dto::scrape_request::ScrapeRequestDto;
/// # use crawlrs::workers::markdown_post_processor::MarkdownPostProcessor;
/// # use crawlrs::domain::services::markdown_service::HtmdMarkdownService;
/// # use crawlrs::domain::services::content_extractor::ContentExtractionFacade;
/// # use serde_json::json;
/// # use std::sync::Arc;
/// # use uuid::Uuid;
/// let processor = MarkdownPostProcessor::new(
///     Arc::new(HtmdMarkdownService::new()),
///     Some(Arc::new(ContentExtractionFacade::new(None))),
/// );
/// let req: ScrapeRequestDto = serde_json::from_value(json!({
///     "url": "https://example.com",
///     "formats": ["markdown"]
/// })).unwrap();
/// // generate is async — use .await in async context
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// let md = processor.generate(Uuid::new_v4(), &req, "<html><body><h1>Hi</h1></body></html>").await;
/// assert!(md.unwrap().is_some());
/// # });
/// ```
#[derive(Clone)]
pub struct MarkdownPostProcessor {
    /// 注入的 Markdown 转换服务（DIP：依赖抽象而非具体实现）
    markdown_service: Arc<dyn MarkdownServiceTrait>,
    /// 可选的正文提取门面
    ///
    /// `only_main_content=true` 时先经此提取正文，再转 Markdown。
    /// `None` 时跳过正文提取，整页转 Markdown。
    content_extractor: Option<Arc<ContentExtractionFacade>>,
}

impl MarkdownPostProcessor {
    /// 创建新的 Markdown 后处理器
    ///
    /// # 参数
    ///
    /// - `markdown_service`：Markdown 转换服务（实现 [`MarkdownServiceTrait`] trait），
    ///   由调用方注入具体实现（如 [`crate::domain::services::markdown_service::HtmdMarkdownService`]）。
    ///   通过 `Arc` 共享，多个 worker 可共用同一实例。
    /// - `content_extractor`：可选的正文提取门面。
    ///   `only_main_content=true` 时先提取正文再转 Markdown；`None` 时跳过。
    #[must_use]
    pub fn new(
        markdown_service: Arc<dyn MarkdownServiceTrait>,
        content_extractor: Option<Arc<ContentExtractionFacade>>,
    ) -> Self {
        Self {
            markdown_service,
            content_extractor,
        }
    }

    /// 根据任务请求生成 Markdown（如 `formats` 含 `"markdown"`）
    ///
    /// # 参数
    ///
    /// - `task_id`：任务 ID（仅用于日志）
    /// - `req`：已解析的 [`ScrapeRequestDto`]，读取 `formats` 字段
    /// - `content`：待转换的 HTML 内容
    ///
    /// # 返回值（错误显性化）
    ///
    /// - `Ok(Some(md))`：成功生成非空 Markdown
    /// - `Ok(None)`：任务未请求 markdown（非错误，调用方应跳过）
    /// - `Err(ConversionFailed(e))`：底层转换服务返回错误
    /// - `Err(EmptyResult)`：转换结果为空（HTML 无可见文本）
    ///
    /// # 调用方策略
    ///
    /// markdown 为增强字段，失败不阻断基础抓取结果。调用方应：
    /// ```ignore
    /// match processor.generate(task_id, req, content) {
    ///     Ok(Some(md)) => { /* 存储 markdown */ }
    ///     Ok(None) => { /* 未请求，跳过 */ }
    ///     Err(e) => {
    ///         warn!("markdown post-processing failed: {e}");
    ///         /* 继续不阻断主流程 */
    ///     }
    /// }
    /// ```
    pub async fn generate(
        &self,
        task_id: Uuid,
        req: &ScrapeRequestDto,
        content: &str,
    ) -> Result<Option<String>, MarkdownPostProcessorError> {
        let want_markdown = req
            .formats
            .as_ref()
            .map(|fs| fs.iter().any(|f| f == "markdown"))
            .unwrap_or(false);

        if !want_markdown {
            return Ok(None);
        }

        // only_main_content=true 时先提取正文
        let only_main = req
            .options
            .as_ref()
            .and_then(|o| o.only_main_content)
            .unwrap_or(false);

        // P16：优先用保留结构的正文 HTML（表格/链接/代码块不丢）。
        // `ExtractedContent.text` 是纯文本——纯文本喂 Markdown 转换器
        // 会把结构信息全部抹平。
        enum MainContentSource {
            /// 结构化正文 HTML
            StructuredHtml(String),
            /// 纯文本（抽取器未产出正文 HTML，显性降级）
            PlainText(String),
            /// 整页 HTML
            FullHtml,
        }

        let content_source: MainContentSource = if only_main {
            if let Some(extractor) = &self.content_extractor {
                match extractor.extract(content, &req.url).await {
                    Ok(extracted) if !extracted.text.trim().is_empty() => {
                        debug!(
                            "task_id: {}, main content extracted (confidence={:.2}, {} bytes)",
                            task_id,
                            extracted.confidence,
                            extracted.text.len()
                        );
                        match extracted.html.filter(|h| !h.trim().is_empty()) {
                            Some(html) => MainContentSource::StructuredHtml(html),
                            None => {
                                // 降级必须显性：纯文本转 Markdown 无结构
                                warn!(
                                    "task_id: {}, extractor produced no main-content HTML; \
                                     Markdown will be structureless (plain text)",
                                    task_id
                                );
                                MainContentSource::PlainText(extracted.text)
                            }
                        }
                    }
                    Ok(_) => {
                        warn!(
                                "task_id: {}, content extraction returned empty, falling back to full HTML",
                                task_id
                            );
                        MainContentSource::FullHtml
                    }
                    Err(e) => {
                        warn!(
                            "task_id: {}, content extraction failed: {}, falling back to full HTML",
                            task_id, e
                        );
                        MainContentSource::FullHtml
                    }
                }
            } else {
                MainContentSource::FullHtml
            }
        } else {
            MainContentSource::FullHtml
        };

        // 结构化正文与整页 HTML 走 Markdown 转换；纯文本没有可转换的结构，
        // 直接作为 Markdown 正文（与修复前行为一致）。按值消费避免整文深拷贝。
        let markdown_result = match content_source {
            MainContentSource::PlainText(text) => Ok(text),
            MainContentSource::StructuredHtml(html) => {
                self.markdown_service.to_markdown(&html, false)
            }
            MainContentSource::FullHtml => self.markdown_service.to_markdown(content, false),
        };

        match markdown_result {
            Ok(md) if !md.trim().is_empty() => {
                debug!(
                    "task_id: {}, Markdown generated ({} bytes)",
                    task_id,
                    md.len()
                );
                Ok(Some(md))
            }
            Ok(_md) => {
                debug!(
                    "task_id: {}, Markdown conversion returned empty result",
                    task_id
                );
                Err(MarkdownPostProcessorError::EmptyResult)
            }
            Err(e) => {
                // #7：保留下层 MarkdownError 类型（通过 #[from] 自动转换），
                // 调用方可 downcast_ref 拿到原始错误做精细处理。
                warn!("task_id: {}, Markdown conversion failed: {}", task_id, e);
                Err(MarkdownPostProcessorError::ConversionFailed(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::services::markdown_service::HtmdMarkdownService;
    use serde_json::json;

    fn make_req(payload: serde_json::Value) -> ScrapeRequestDto {
        serde_json::from_value(payload).expect("valid ScrapeRequestDto")
    }

    fn make_processor() -> MarkdownPostProcessor {
        MarkdownPostProcessor::new(Arc::new(HtmdMarkdownService::new()), None)
    }

    /// formats 含 "markdown" 时应生成 Markdown（返回 Ok(Some)）
    #[tokio::test]
    async fn generates_markdown_when_formats_contains_markdown() {
        let p = make_processor();
        let req = make_req(json!({
            "url": "https://example.com",
            "formats": ["markdown"]
        }));
        let result = p
            .generate(
                Uuid::new_v4(),
                &req,
                "<html><body><h1>Title</h1></body></html>",
            )
            .await;
        let md = result.expect("expected Ok, got Err");
        assert!(md.is_some(), "expected Some(markdown)");
        let md = md.expect("some markdown");
        assert!(
            md.contains("Title"),
            "expected Title in markdown, got: {md}"
        );
    }

    /// formats 不含 "markdown" 时返回 Ok(None)（非错误）
    #[tokio::test]
    async fn returns_ok_none_when_formats_does_not_contain_markdown() {
        let p = make_processor();
        let req = make_req(json!({
            "url": "https://example.com",
            "formats": ["html"]
        }));
        let result = p
            .generate(Uuid::new_v4(), &req, "<html><body>Hi</body></html>")
            .await;
        assert!(result.is_ok(), "expected Ok, got Err: {:?}", result.err());
        assert!(
            result.unwrap().is_none(),
            "expected Ok(None) when markdown not requested"
        );
    }

    /// formats 为 None 时返回 Ok(None)（非错误）
    #[tokio::test]
    async fn returns_ok_none_when_formats_is_none() {
        let p = make_processor();
        let req = make_req(json!({"url": "https://example.com"}));
        let result = p
            .generate(Uuid::new_v4(), &req, "<html><body>Hi</body></html>")
            .await;
        assert!(result.is_ok(), "expected Ok, got Err: {:?}", result.err());
        assert!(
            result.unwrap().is_none(),
            "expected Ok(None) when formats absent"
        );
    }

    /// 空 HTML 在请求 markdown 时应返回 Err(EmptyResult)（错误显性化）
    ///
    /// 原实现：返回 `None`（与"未请求 markdown"混淆）
    /// 现实现：返回 `Err(EmptyResult)`（让调用方区分语义）
    #[tokio::test]
    async fn returns_err_empty_result_for_empty_html_when_markdown_requested() {
        let p = make_processor();
        let req = make_req(json!({
            "url": "https://example.com",
            "formats": ["markdown"]
        }));
        let result = p.generate(Uuid::new_v4(), &req, "").await;
        match result {
            Err(MarkdownPostProcessorError::EmptyResult) => {}
            other => panic!("expected Err(EmptyResult), got {other:?}"),
        }
    }

    /// 纯空白内容（仅空格/换行）在请求 markdown 时也应返回 Err(EmptyResult)
    #[tokio::test]
    async fn returns_err_empty_result_for_whitespace_only_html() {
        let p = make_processor();
        let req = make_req(json!({
            "url": "https://example.com",
            "formats": ["markdown"]
        }));
        let result = p.generate(Uuid::new_v4(), &req, "   \n\t  \n  ").await;
        match result {
            Err(MarkdownPostProcessorError::EmptyResult) => {}
            other => panic!("expected Err(EmptyResult), got {other:?}"),
        }
    }

    /// 确认 Ok(None) 与 Err(EmptyResult) 在调用方正确处理（模拟 scrape_worker 行为）
    ///
    /// 调用方策略：markdown 为增强字段，失败不阻断主流程
    #[tokio::test]
    async fn caller_strategy_continues_on_markdown_error() {
        let p = make_processor();

        // 未请求 markdown → Ok(None) → 继续无 markdown
        let req = make_req(json!({"url": "https://example.com"}));
        let result = p.generate(Uuid::new_v4(), &req, "<html></html>").await;
        let generated: Option<String> = match result {
            Ok(md) => md,
            Err(e) => {
                // 模拟 scrape_worker 调用方：记录告警并继续
                warn!("markdown post-processing failed: {}", e);
                None
            }
        };
        assert!(
            generated.is_none(),
            "expected no markdown when not requested"
        );

        // 请求 markdown 但空 HTML → Err(EmptyResult) → 继续无 markdown
        let req = make_req(json!({
            "url": "https://example.com",
            "formats": ["markdown"]
        }));
        let result = p.generate(Uuid::new_v4(), &req, "").await;
        let generated: Option<String> = match result {
            Ok(md) => md,
            Err(e) => {
                warn!("markdown post-processing failed: {}", e);
                None
            }
        };
        assert!(
            generated.is_none(),
            "expected no markdown on empty html error"
        );
    }

    /// P16 回归钉：only_main_content=true 且抽取器产出正文 HTML 时，
    /// Markdown 必须保留结构（表格/链接），而非退化为纯文本。
    ///
    /// 修复前：直接把 `ExtractedContent.text`（纯文本）喂给 Markdown 转换器，
    /// 表格/链接/代码块结构全部丢失。
    #[cfg(feature = "trafilatura")]
    #[tokio::test]
    async fn only_main_content_preserves_markdown_structure_from_extracted_html() {
        use crate::domain::services::content_extractor::ContentExtractionFacade;

        let p = MarkdownPostProcessor::new(
            Arc::new(HtmdMarkdownService::new()),
            Some(Arc::new(ContentExtractionFacade::new(None))),
        );
        let req = make_req(json!({
            "url": "https://example.com/article",
            "formats": ["markdown"],
            "options": { "only_main_content": true }
        }));
        let html = "<html><head><title>Engine Benchmarks Report</title></head><body>\
                    <nav>home | about | contact</nav>\
                    <article>\
                    <h1>Engine Benchmarks Report</h1>\
                    <p>This report covers the latest round of performance testing across \
                    all supported crawling engines. The methodology focuses on sustained \
                    request throughput under realistic network latency conditions.\
                    Read the <a href=\"https://example.com/docs\">full documentation</a> for setup details.</p>\
                    <h2>Methodology</h2>\
                    <p>Each engine ran for ten minutes against a controlled test cluster. \
                    We measured requests per second, error rates, and memory overhead. \
                    The test harness pinned CPU affinity to eliminate scheduler noise.</p>\
                    <h2>Results</h2>\
                    <ul><li>wreq reached 9000 rps</li><li>reqwest reached 6500 rps</li></ul>\
                    <p>For deeper analysis see the <a href=\"https://example.com/analysis\">analysis</a> page.</p>\
                    </article>\
                    </body></html>";
        let result = p.generate(Uuid::new_v4(), &req, html).await;
        let md = result
            .expect("expected Ok")
            .expect("expected Some(markdown)");
        assert!(
            md.contains("documentation") && md.contains("]("),
            "links must survive as Markdown links, got: {md}"
        );
        assert!(
            md.contains('#'),
            "heading structure must survive as Markdown heading, got: {md}"
        );
        // 注：rs-trafilatura 0.2.2 的 content_html 会把 <table> 扁平化为文本，
        // 表格结构无法经此链路保留（外部依赖能力边界，见 design.md Consequences）
    }

    /// P16 降级路径：only_main_content=true 但无抽取器注入时，
    /// 维持整页 HTML 转换行为（与修复前一致）
    #[tokio::test]
    async fn only_main_content_without_extractor_falls_back_to_full_html() {
        let p = make_processor();
        let req = make_req(json!({
            "url": "https://example.com/article",
            "formats": ["markdown"],
            "options": { "only_main_content": true }
        }));
        let result = p
            .generate(
                Uuid::new_v4(),
                &req,
                "<html><body><h1>Title</h1><p>Body text.</p></body></html>",
            )
            .await;
        let md = result
            .expect("expected Ok")
            .expect("expected Some(markdown)");
        assert!(
            md.contains("Title"),
            "full HTML conversion should still work, got: {md}"
        );
    }
}
