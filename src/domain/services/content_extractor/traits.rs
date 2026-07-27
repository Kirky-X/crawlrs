// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information

//! 正文提取 trait 与类型定义（design.md §11，T044/R-content-002）
//!
//! 提供 [`ContentExtractor`] trait 抽象 + [`ExtractedContent`] 输出结构 +
//! [`PageType`] 页面分类枚举 + [`ExtractError`] 类型化错误 + [`Result`] 别名。
//! 多实现按优先级由
//! [`crate::domain::services::content_extractor::facade::ContentExtractionFacade`] 路由。
//!
//! 实现说明（R-content-003）：
//! - `TrafilaturaExtractor`（gated `extractor-trafilatura`）：主路径，质量最高
//! - `DomSmoothieExtractor`（gated `extractor-dom-smoothie`）：性能回退，DOM 启发式
//! - `CssRuleExtractor`：兜底（无 feature 依赖），复用 `ExtractionService::get_clean_text`
//!
//! 三特性均关闭时 Facade 退化为 CssRule 兜底（编译通过，功能可用）。

use serde::{Deserialize, Serialize};

/// 提取页面类型（用于评估 confidence 与选择策略）
///
/// 来源：trafilatura `webpage_type` 与 dom_smoothie `Article` 启发式分类的统一抽象。
///
/// 注：错误状态不通过枚举变体表达（架构审查 H-1：PageType::Error 反模式），
/// 而是通过 `Result<ExtractedContent, ExtractError>` 显式返回（规则12：失败必须显性化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageType {
    /// 文章类（新闻、博客、长文）
    Article,
    /// 列表类（搜索引擎结果、聚合页）
    Listing,
    /// 其他 / 无法分类
    Unknown,
}

/// 正文提取结果（R-content-002）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContent {
    /// 提取的正文文本（非空）
    pub text: String,
    /// 标题（可选，来自 `<title>` / `<h1>` / 元数据）
    pub title: Option<String>,
    /// 作者（可选，来自元数据）
    pub author: Option<String>,
    /// 置信度 ∈ [0.0, 1.0]，表示提取结果的可信度
    ///
    /// 各实现约定：
    /// - `TrafilaturaExtractor`：算法内置 confidence
    /// - `DomSmoothieExtractor`：DOM 启发式打分
    /// - `CssRuleExtractor`：固定 0.5（兜底，最低可信度）
    /// - LLM 回退：0.8（结构化提取，高可信度）
    pub confidence: f32,
    /// 页面类型分类
    pub page_type: PageType,
}

impl ExtractedContent {
    /// 创建新实例（便于实现方构造）
    pub fn new(text: String) -> Self {
        Self {
            text,
            title: None,
            author: None,
            confidence: 0.0,
            page_type: PageType::Unknown,
        }
    }

    /// 是否应触发 LLM 回退（confidence < 0.7）
    ///
    /// 由 `ContentExtractionFacade` 调用，`LLMService` 可用时升级提取。
    pub fn should_fallback_to_llm(&self) -> bool {
        self.confidence < 0.7
    }
}

/// 正文提取服务 trait（design.md §11 / R-content-002）
///
/// 实现方需保证线程安全（`Send + Sync`）以便在 `ContentExtractionFacade` 中共享。
/// HTML 预处理复用现有 [`crate::domain::services::extraction_service::ExtractionService::get_clean_text`]，
/// 不引入第三套清理实现（spec constraint）。
pub trait ContentExtractor: Send + Sync {
    /// 提取正文
    ///
    /// # 参数
    ///
    /// - `html`: 待提取的 HTML 字符串
    /// - `url`: 文档 URL（用于相对链接解析与元数据补充）
    ///
    /// # 返回值
    ///
    /// 成功返回 [`ExtractedContent`]；提取失败（如无法解析、空内容）返回 [`ExtractError`]。
    fn extract(&self, html: &str, url: &str) -> Result<ExtractedContent>;

    /// 实现名称（用于日志与诊断）
    fn name(&self) -> &'static str;
}

/// 正文提取错误（类型化错误，禁止吞掉底层错误，规则 12）
///
/// 使用 `thiserror::Error` 派生（架构审查 M-1：恢复社区惯例，避免手写 Display+Error 的退化），
/// 便于上层 `?` 传播与日志结构化输出。
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// HTML 解析失败
    #[error("HTML parse error: {0}")]
    HtmlParseError(String),
    /// 提取结果为空（无有效正文）
    #[error("no content extracted")]
    NoContent,
    /// 底层 extractor 库错误（保留原错误信息）
    #[error("extractor failed: {0}")]
    ExtractorFailed(String),
}

/// 正文提取 Result 别名（简化签名）
pub type Result<T> = std::result::Result<T, ExtractError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// `ExtractedContent::new` 应初始化为最低可信度（0.0）+ Unknown 类型
    #[test]
    fn new_initializes_with_low_confidence_and_unknown_type() {
        let c = ExtractedContent::new("hello".to_string());
        assert_eq!(c.text, "hello");
        assert_eq!(c.confidence, 0.0);
        assert_eq!(c.page_type, PageType::Unknown);
        assert!(c.title.is_none() && c.author.is_none());
    }

    /// confidence < 0.7 时应触发 LLM 回退
    #[test]
    fn should_fallback_to_llm_when_confidence_below_threshold() {
        let mut c = ExtractedContent::new("text".to_string());
        c.confidence = 0.69;
        assert!(c.should_fallback_to_llm(), "0.69 should trigger fallback");
        c.confidence = 0.7;
        assert!(
            !c.should_fallback_to_llm(),
            "0.7 should NOT trigger fallback (boundary)"
        );
        c.confidence = 1.0;
        assert!(!c.should_fallback_to_llm());
    }

    /// PageType serde 应使用 snake_case
    #[test]
    fn page_type_serializes_as_snake_case() {
        let json = serde_json::to_string(&PageType::Article).unwrap();
        assert_eq!(json, "\"article\"");
        let parsed: PageType = serde_json::from_str("\"listing\"").unwrap();
        assert_eq!(parsed, PageType::Listing);
    }

    /// PageType 三个变体应可序列化往返
    #[test]
    fn page_type_all_variants_roundtrip() {
        for variant in [PageType::Article, PageType::Listing, PageType::Unknown] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: PageType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    /// ExtractedContent serde 应可往返
    #[test]
    fn extracted_content_roundtrips_through_serde() {
        let c = ExtractedContent {
            text: "body".to_string(),
            title: Some("Title".to_string()),
            author: Some("Author".to_string()),
            confidence: 0.85,
            page_type: PageType::Article,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: ExtractedContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, c.text);
        assert_eq!(back.title, c.title);
        assert_eq!(back.author, c.author);
        assert!((back.confidence - c.confidence).abs() < f32::EPSILON);
        assert_eq!(back.page_type, c.page_type);
    }

    /// ExtractError Display 应包含可读消息
    #[test]
    fn extract_error_display_includes_message() {
        let e = ExtractError::HtmlParseError("bad html".to_string());
        let s = format!("{e}");
        assert!(s.contains("HTML parse error"));
        assert!(s.contains("bad html"));

        let e = ExtractError::NoContent;
        assert_eq!(format!("{e}"), "no content extracted");

        let e = ExtractError::ExtractorFailed("boom".to_string());
        let s = format!("{e}");
        assert!(s.contains("extractor failed"));
        assert!(s.contains("boom"));
    }

    /// ExtractError 应实现 std::error::Error
    #[test]
    fn extract_error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        let e = ExtractError::NoContent;
        assert_error(&e);
    }

    /// ExtractError 应可作为 Box<dyn std::error::Error> 使用
    #[test]
    fn extract_error_can_be_boxed() {
        let e: Box<dyn std::error::Error> =
            Box::new(ExtractError::ExtractorFailed("oops".to_string()));
        assert!(e.to_string().contains("oops"));
    }

    /// Result 别名应等价于 std::result::Result<T, ExtractError>
    #[test]
    fn result_alias_is_correct() {
        let r: Result<()> = Err(ExtractError::NoContent);
        assert!(r.is_err());
        let r: Result<i32> = Ok(42);
        assert_eq!(r.unwrap(), 42);
    }
}
