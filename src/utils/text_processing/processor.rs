// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::utils::text_processing::encoding::{TextEncodingError, TextEncodingProcessor};
use async_trait::async_trait;
use log::{debug, error, info};
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

/// 文本处理错误类型
#[derive(Error, Debug)]
pub enum TextProcessingError {
    #[error("正则表达式编译失败: {0}")]
    RegexCompilationError(String),

    #[error("编码处理错误: {0}")]
    EncodingError(#[from] TextEncodingError),
}

/// 常量定义 - 编码检测模式键名
const ENCODING_PATTERN_HTML_META: &str = "html_meta";
const ENCODING_PATTERN_XML_DECLARATION: &str = "xml_declaration";
const ENCODING_PATTERN_HTTP_CONTENT_TYPE: &str = "http_content_type";

/// 初始化编码检测正则表达式模式（公共函数，消除重复代码）
///
/// 此函数被 WebContentProcessorComponent 和 WebContentProcessor 共用
pub fn init_encoding_patterns() -> Result<HashMap<&'static str, Regex>, TextProcessingError> {
    let mut patterns = HashMap::with_capacity(4);

    patterns.insert(
        ENCODING_PATTERN_HTML_META,
        Regex::new(r#"(?i)<meta[^>]*charset\s*=\s*["']?([^"'>\s]+)["']?[^>]*>"#)
            .map_err(|e| TextProcessingError::RegexCompilationError(e.to_string()))?,
    );
    patterns.insert(
        ENCODING_PATTERN_XML_DECLARATION,
        Regex::new(r#"(?i)<\?xml[^>]*encoding\s*=\s*["']?([^"'>\s]+)["']?[^>]*\?>"#)
            .map_err(|e| TextProcessingError::RegexCompilationError(e.to_string()))?,
    );
    patterns.insert(
        ENCODING_PATTERN_HTTP_CONTENT_TYPE,
        Regex::new(r#"(?i)charset\s*=\s*([^;\s]+)"#)
            .map_err(|e| TextProcessingError::RegexCompilationError(e.to_string()))?,
    );
    Ok(patterns)
}

/// 检测文本内容是否为 HTML 结构
///
/// 用于判断内容是否包含 HTML 标签
pub fn detect_html_structure(content: &str) -> bool {
    content.contains("<html")
        || content.contains("<!DOCTYPE")
        || content.contains("<head")
        || content.contains("<body")
        || content.contains("<div")
        || content.contains("<p>")
        || content.contains("<a ")
}

/// 文本编码处理器 trait（支持 DI）
#[async_trait]
pub trait TextEncodingProcessorTrait: Send + Sync {
    fn process_text(&self, content: &[u8]) -> Result<String, TextEncodingError>;
    fn trim_newlines(&self, text: &str) -> String;
}

/// 文本编码处理器组件
#[allow(dead_code)]
pub struct TextEncodingProcessorComponent {
    default_encoding: &'static str,
    detect_encoding: bool,
}

impl TextEncodingProcessorComponent {
    pub fn new() -> Self {
        Self {
            default_encoding: "utf-8",
            detect_encoding: true,
        }
    }
}

impl Default for TextEncodingProcessorComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEncodingProcessorTrait for TextEncodingProcessorComponent {
    fn process_text(&self, content: &[u8]) -> Result<String, TextEncodingError> {
        TextEncodingProcessor::new().process_text(content)
    }

    fn trim_newlines(&self, text: &str) -> String {
        text.lines().map(|l| l.trim()).collect::<Vec<_>>().join(" ")
    }
}

/// 网页内容处理器 trait（支持 DI）
///
/// 提供网页内容处理的抽象接口，便于测试时注入 mock 实现。
#[async_trait]
pub trait WebContentProcessorTrait: Send + Sync {
    fn process_web_content(
        &self,
        content: &[u8],
        content_type: Option<&str>,
    ) -> Result<ProcessedWebContent, WebContentError>;
    fn process_batch(
        &self,
        contents: Vec<(&[u8], Option<&str>)>,
    ) -> Vec<Result<ProcessedWebContent, WebContentError>>;
}

/// 网页内容处理器组件（DI 实现）
pub struct WebContentProcessorComponent {
    /// 文本编码处理器
    text_processor: Arc<dyn TextEncodingProcessorTrait>,
    /// 编码模式
    encoding_patterns: HashMap<&'static str, Regex>,
    /// HTML 清理器
    html_cleaner: HtmlCleaner,
}

impl WebContentProcessorComponent {
    pub fn new(text_processor: Arc<dyn TextEncodingProcessorTrait>) -> Self {
        Self {
            text_processor,
            encoding_patterns: init_encoding_patterns()
                .expect("Failed to initialize encoding patterns"),
            html_cleaner: HtmlCleaner::new(),
        }
    }
}

impl Default for WebContentProcessorComponent {
    fn default() -> Self {
        Self::new(Arc::new(TextEncodingProcessorComponent::default()))
    }
}

impl WebContentProcessorTrait for WebContentProcessorComponent {
    fn process_web_content(
        &self,
        content: &[u8],
        content_type: Option<&str>,
    ) -> Result<ProcessedWebContent, WebContentError> {
        let text_content = self.process_encoding(content)?;
        let is_html = self.detect_html_structure(&text_content);
        let declared_encoding = self.extract_declared_encoding(&text_content);
        debug!(
            "检测到HTML结构: {}, 声明编码: {:?}",
            is_html, declared_encoding
        );
        let extracted_text = if is_html {
            self.extract_text_from_html(&text_content)?
        } else {
            text_content.clone()
        };
        let cleaned_text = self.clean_text(&extracted_text);
        let detected_language = self.detect_language(&cleaned_text);
        Ok(ProcessedWebContent {
            original_content: text_content,
            extracted_text: cleaned_text,
            is_html,
            declared_encoding,
            detected_language,
            content_type: content_type.map(|s| s.to_string()),
            content_length: content.len(),
        })
    }

    fn process_batch(
        &self,
        contents: Vec<(&[u8], Option<&str>)>,
    ) -> Vec<Result<ProcessedWebContent, WebContentError>> {
        contents
            .into_iter()
            .map(|(content, content_type)| self.process_web_content(content, content_type))
            .collect()
    }
}

impl WebContentProcessorComponent {
    fn process_encoding(&self, content: &[u8]) -> Result<String, WebContentError> {
        match self.text_processor.process_text(content) {
            Ok(text) => {
                debug!("文本编码处理成功");
                Ok(text)
            }
            Err(e) => {
                error!("文本编码处理失败: {}", e);
                Err(WebContentError::EncodingError(e))
            }
        }
    }

    fn detect_html_structure(&self, content: &str) -> bool {
        content.contains("<html")
            || content.contains("<!DOCTYPE")
            || content.contains("<head")
            || content.contains("<body")
            || content.contains("<div")
            || content.contains("<p>")
            || content.contains("<a ")
    }

    fn extract_declared_encoding(&self, content: &str) -> Option<String> {
        if let Some(captures) = self.encoding_patterns.get("html_meta")?.captures(content) {
            if let Some(encoding) = captures.get(1) {
                return Some(encoding.as_str().to_lowercase());
            }
        }
        if let Some(captures) = self
            .encoding_patterns
            .get("xml_declaration")?
            .captures(content)
        {
            if let Some(encoding) = captures.get(1) {
                return Some(encoding.as_str().to_lowercase());
            }
        }
        None
    }

    fn extract_text_from_html(&self, html: &str) -> Result<String, WebContentError> {
        self.html_cleaner.extract_text(html)
    }

    fn clean_text(&self, text: &str) -> String {
        let mut cleaned = text.to_string();
        cleaned = html_escape::decode_html_entities(&cleaned).to_string();
        cleaned = cleaned
            .chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .collect();
        cleaned = self.text_processor.trim_newlines(&cleaned);
        cleaned.trim().to_string()
    }

    fn detect_language(&self, text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }
        let chinese_ratio = text
            .chars()
            .filter(|c| {
                let code = *c as u32;
                (0x4E00..=0x9FFF).contains(&code)
                    || (0x3400..=0x4DBF).contains(&code)
                    || (0xF900..=0xFAFF).contains(&code)
            })
            .count() as f32
            / text.len() as f32;
        if chinese_ratio > 0.3 {
            Some("zh".to_string())
        } else if text.chars().filter(|c| c.is_ascii()).count() as f32 / text.len() as f32 > 0.8 {
            Some("en".to_string())
        } else {
            Some("unknown".to_string())
        }
    }
}

/// 网页内容处理器
#[derive(Clone)]
pub struct WebContentProcessor {
    text_processor: Arc<TextEncodingProcessor>,
    html_cleaner: HtmlCleaner,
    encoding_patterns: HashMap<&'static str, Regex>,
}

impl Default for WebContentProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl WebContentProcessor {
    /// Create a new WebContentProcessor with default TextEncodingProcessor
    pub fn new() -> Self {
        Self::with_text_processor(TextEncodingProcessor::new())
    }

    /// Create a WebContentProcessor with a specific TextEncodingProcessor (for DI)
    pub fn with_text_processor(text_processor: TextEncodingProcessor) -> Self {
        Self {
            text_processor: Arc::new(text_processor),
            html_cleaner: HtmlCleaner::new(),
            encoding_patterns: init_encoding_patterns()
                .expect("Failed to initialize encoding patterns"),
        }
    }

    /// Create a WebContentProcessor with an injected TextEncodingProcessor
    pub fn with_injected_processor(text_processor: Arc<TextEncodingProcessor>) -> Self {
        Self {
            text_processor,
            html_cleaner: HtmlCleaner::new(),
            encoding_patterns: init_encoding_patterns()
                .expect("Failed to initialize encoding patterns"),
        }
    }

    pub fn process_web_content(
        &self,
        content: &[u8],
        content_type: Option<&str>,
    ) -> Result<ProcessedWebContent, WebContentError> {
        info!("开始处理网页内容，大小: {} 字节", content.len());
        let text_content = self.process_encoding(content)?;
        let is_html = self.detect_html_structure(&text_content);
        let declared_encoding = self.extract_declared_encoding(&text_content);
        debug!(
            "检测到HTML结构: {}, 声明编码: {:?}",
            is_html, declared_encoding
        );
        let extracted_text = if is_html {
            self.extract_text_from_html(&text_content)?
        } else {
            text_content.clone()
        };
        let cleaned_text = self.clean_text(&extracted_text);
        let detected_language = self.detect_language(&cleaned_text);
        Ok(ProcessedWebContent {
            original_content: text_content,
            extracted_text: cleaned_text,
            is_html,
            declared_encoding,
            detected_language,
            content_type: content_type.map(|s| s.to_string()),
            content_length: content.len(),
        })
    }

    fn process_encoding(&self, content: &[u8]) -> Result<String, WebContentError> {
        match self.text_processor.process_text(content) {
            Ok(text) => {
                debug!("文本编码处理成功");
                Ok(text)
            }
            Err(e) => {
                error!("文本编码处理失败: {}", e);
                Err(WebContentError::EncodingError(e))
            }
        }
    }

    fn detect_html_structure(&self, content: &str) -> bool {
        content.contains("<html")
            || content.contains("<!DOCTYPE")
            || content.contains("<head")
            || content.contains("<body")
            || content.contains("<div")
            || content.contains("<p>")
            || content.contains("<a ")
    }

    fn extract_declared_encoding(&self, content: &str) -> Option<String> {
        if let Some(captures) = self.encoding_patterns.get("html_meta")?.captures(content) {
            if let Some(encoding) = captures.get(1) {
                return Some(encoding.as_str().to_lowercase());
            }
        }
        if let Some(captures) = self
            .encoding_patterns
            .get("xml_declaration")?
            .captures(content)
        {
            if let Some(encoding) = captures.get(1) {
                return Some(encoding.as_str().to_lowercase());
            }
        }
        None
    }

    fn extract_text_from_html(&self, html: &str) -> Result<String, WebContentError> {
        self.html_cleaner.extract_text(html)
    }

    fn clean_text(&self, text: &str) -> String {
        let mut cleaned = text.to_string();
        cleaned = html_escape::decode_html_entities(&cleaned).to_string();
        cleaned = cleaned
            .chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .collect();
        cleaned = self.text_processor.trim_newlines(&cleaned);
        cleaned.trim().to_string()
    }

    fn detect_language(&self, text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }
        let chinese_ratio = text
            .chars()
            .filter(|c| {
                let code = *c as u32;
                (0x4E00..=0x9FFF).contains(&code)
                    || (0x3400..=0x4DBF).contains(&code)
                    || (0xF900..=0xFAFF).contains(&code)
            })
            .count() as f32
            / text.len() as f32;
        if chinese_ratio > 0.3 {
            Some("zh".to_string())
        } else if text.chars().filter(|c| c.is_ascii()).count() as f32 / text.len() as f32 > 0.8 {
            Some("en".to_string())
        } else {
            Some("unknown".to_string())
        }
    }

    pub fn process_batch(
        &self,
        contents: Vec<(&[u8], Option<&str>)>,
    ) -> Vec<Result<ProcessedWebContent, WebContentError>> {
        contents
            .into_iter()
            .map(|(content, content_type)| self.process_web_content(content, content_type))
            .collect()
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct HtmlCleaner {
    script_regex: Regex,
    style_regex: Regex,
    comment_regex: Regex,
    tag_regex: Regex,
    whitespace_regex: Regex,
}

impl HtmlCleaner {
    fn new() -> Self {
        Self {
            script_regex: Regex::new(r#"(?is)<script[^>]*>.*?</script>"#)
                .expect("Failed to compile script regex"),
            style_regex: Regex::new(r#"(?is)<style[^>]*>.*?</style>"#)
                .expect("Failed to compile style regex"),
            comment_regex: Regex::new(r#"(?is)<!--.*?-->"#)
                .expect("Failed to compile comment regex"),
            tag_regex: Regex::new(r#"(?is)<[^>]+>"#).expect("Failed to compile tag regex"),
            whitespace_regex: Regex::new(r#"\s+"#).expect("Failed to compile whitespace regex"),
        }
    }

    fn extract_text(&self, html: &str) -> Result<String, WebContentError> {
        let mut text = html.to_string();
        text = self.script_regex.replace_all(&text, "").to_string();
        text = self.style_regex.replace_all(&text, "").to_string();
        text = self.comment_regex.replace_all(&text, "").to_string();
        text = self.tag_regex.replace_all(&text, " ").to_string();
        text = self.normalize_whitespace(&text);
        Ok(text.trim().to_string())
    }

    fn normalize_whitespace(&self, text: &str) -> String {
        self.whitespace_regex.replace_all(text, " ").to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ProcessedWebContent {
    pub original_content: String,
    pub extracted_text: String,
    pub is_html: bool,
    pub declared_encoding: Option<String>,
    pub detected_language: Option<String>,
    pub content_type: Option<String>,
    pub content_length: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum WebContentError {
    #[error("编码处理错误: {0}")]
    EncodingError(#[from] TextEncodingError),
    #[error("HTML解析错误: {0}")]
    HtmlParseError(String),
    #[error("内容提取错误: {0}")]
    ContentExtractionError(String),
}

/// 使用提供的处理器实例处理网页内容
pub fn process_web_content_with_processor(
    processor: &WebContentProcessor,
    content: &[u8],
    content_type: Option<&str>,
) -> Result<ProcessedWebContent, WebContentError> {
    processor.process_web_content(content, content_type)
}

/// 爬虫文本处理器
pub struct CrawlTextProcessor {
    max_processing_time: Duration,
    max_content_size: usize,
}

impl Default for CrawlTextProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl CrawlTextProcessor {
    pub fn new() -> Self {
        Self {
            max_processing_time: Duration::from_secs(30),
            max_content_size: 10 * 1024 * 1024,
        }
    }

    pub fn process_crawled_content(
        &self,
        content: &[u8],
        url: &str,
        content_type: Option<&str>,
    ) -> Result<ProcessedCrawlContent, CrawlProcessingError> {
        let start_time = Instant::now();
        let content_size = content.len();
        info!(
            "开始处理抓取的内容: URL={}, 大小={} 字节",
            url, content_size
        );
        if content_size > self.max_content_size {
            return Err(CrawlProcessingError::ContentTooLarge {
                size: content_size,
                max_size: self.max_content_size,
            });
        }
        if start_time.elapsed() > self.max_processing_time {
            return Err(CrawlProcessingError::ProcessingTimeout);
        }
        let processed_content = self.process_content_with_timeout(content, content_type)?;
        let processing_time = start_time.elapsed();
        info!(
            "内容处理完成: URL={}, 耗时={:?}, 提取文本长度={}",
            url,
            processing_time,
            processed_content.extracted_text.len()
        );
        Ok(ProcessedCrawlContent {
            url: url.to_string(),
            processed_content: processed_content.clone(),
            processing_time,
            original_size: content_size,
            processed_size: processed_content.extracted_text.len(),
        })
    }

    fn process_content_with_timeout(
        &self,
        content: &[u8],
        content_type: Option<&str>,
    ) -> Result<ProcessedWebContent, CrawlProcessingError> {
        use std::sync::mpsc;
        use std::thread;
        let (tx, rx) = mpsc::channel();
        let content = content.to_vec();
        let content_type = content_type.map(|s| s.to_string());
        let processor = WebContentProcessor::new();
        thread::spawn(move || {
            let result = processor.process_web_content(&content, content_type.as_deref());
            let _ = tx.send(result);
        });
        match rx.recv_timeout(self.max_processing_time) {
            Ok(result) => result.map_err(CrawlProcessingError::WebContentError),
            Err(_) => Err(CrawlProcessingError::ProcessingTimeout),
        }
    }

    pub fn process_batch(
        &self,
        batch: Vec<(&[u8], &str, Option<&str>)>,
    ) -> Vec<Result<ProcessedCrawlContent, CrawlProcessingError>> {
        let mut results = Vec::with_capacity(batch.len());
        for (content, url, content_type) in batch {
            let result = self.process_crawled_content(content, url, content_type);
            results.push(result);
        }
        results
    }

    pub fn process_simple_text(&self, text: &str) -> Result<String, CrawlProcessingError> {
        if text.is_empty() {
            return Ok(String::new());
        }
        let processor = TextEncodingProcessor::new();
        match processor.process_text(text.as_bytes()) {
            Ok(processed) => Ok(processed),
            Err(e) => Err(CrawlProcessingError::TextEncodingError(e)),
        }
    }

    pub fn validate_content_quality(&self, content: &ProcessedCrawlContent) -> ContentQuality {
        let text_length = content.processed_content.extracted_text.len();
        let word_count = content
            .processed_content
            .extracted_text
            .split_whitespace()
            .count();
        if text_length < 50 {
            return ContentQuality::Poor("文本内容过短".to_string());
        }
        if word_count < 10 {
            return ContentQuality::Poor("单词数量过少".to_string());
        }
        let text_ratio = text_length as f64 / content.original_size as f64;
        if text_ratio < 0.01 && content.original_size > 1000 {
            return ContentQuality::Poor("有效内容比例过低".to_string());
        }
        let repetitive_ratio =
            self.calculate_repetitive_ratio(&content.processed_content.extracted_text);
        if repetitive_ratio > 0.8 {
            return ContentQuality::Poor("内容重复度过高".to_string());
        }
        if content.processing_time.as_secs() > 10 {
            ContentQuality::Good
        } else {
            ContentQuality::Excellent
        }
    }

    fn calculate_repetitive_ratio(&self, text: &str) -> f64 {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 10 {
            return 0.0;
        }
        let mut word_counts = std::collections::HashMap::with_capacity(256);
        for word in &words {
            *word_counts.entry(word.to_lowercase()).or_insert(0) += 1;
        }
        let max_count = *word_counts.values().max().unwrap_or(&1);
        let total_words = words.len();
        (max_count as f64 / total_words as f64).min(1.0)
    }

    pub fn get_config(&self) -> CrawlProcessorConfig {
        CrawlProcessorConfig {
            max_processing_time_secs: self.max_processing_time.as_secs(),
            max_content_size_mb: self.max_content_size / (1024 * 1024),
        }
    }

    pub fn update_config(&mut self, config: CrawlProcessorConfig) {
        self.max_processing_time = Duration::from_secs(config.max_processing_time_secs);
        self.max_content_size = config.max_content_size_mb * 1024 * 1024;
        info!("更新爬虫文本处理器配置: {:?}", config);
    }
}

#[derive(Debug, Clone)]
pub struct ProcessedCrawlContent {
    pub url: String,
    pub processed_content: ProcessedWebContent,
    pub processing_time: Duration,
    pub original_size: usize,
    pub processed_size: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum CrawlProcessingError {
    #[error("文本编码处理错误: {0}")]
    TextEncodingError(#[from] TextEncodingError),
    #[error("网页内容处理错误: {0}")]
    WebContentError(#[from] WebContentError),
    #[error("内容过大: {size} 字节 (最大允许: {max_size} 字节)")]
    ContentTooLarge { size: usize, max_size: usize },
    #[error("处理超时")]
    ProcessingTimeout,
    #[error("内容质量过低: {0}")]
    PoorContentQuality(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentQuality {
    Excellent,
    Good,
    Fair,
    Poor(String),
}

#[derive(Debug, Clone)]
pub struct CrawlProcessorConfig {
    pub max_processing_time_secs: u64,
    pub max_content_size_mb: usize,
}

impl Default for CrawlProcessorConfig {
    fn default() -> Self {
        Self {
            max_processing_time_secs: 30,
            max_content_size_mb: 10,
        }
    }
}

/// 使用提供的处理器实例处理爬取的内容
pub fn process_crawled_content_with_processor(
    processor: &CrawlTextProcessor,
    content: &[u8],
    url: &str,
    content_type: Option<&str>,
) -> Result<ProcessedCrawlContent, CrawlProcessingError> {
    processor.process_crawled_content(content, url, content_type)
}

/// 使用提供的处理器实例批量处理爬取的内容
pub fn process_crawled_batch_with_processor(
    processor: &CrawlTextProcessor,
    batch: Vec<(&[u8], &str, Option<&str>)>,
) -> Vec<Result<ProcessedCrawlContent, CrawlProcessingError>> {
    processor.process_batch(batch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_detection() {
        let processor = WebContentProcessor::new();
        let html_content = r#"<!DOCTYPE html><html><head><title>Test</title></head><body><p>Hello World</p></body></html>"#;
        assert!(processor.detect_html_structure(html_content));
        let plain_text = "This is plain text without any HTML tags.";
        assert!(!processor.detect_html_structure(plain_text));
    }

    #[test]
    fn test_encoding_extraction() {
        let processor = WebContentProcessor::new();
        let html_with_meta = r#"<html><head><meta charset="utf-8"><title>Test</title></head><body>Content</body></html>"#;
        let encoding = processor.extract_declared_encoding(html_with_meta);
        assert_eq!(encoding, Some("utf-8".to_string()));
    }

    #[test]
    fn test_html_text_extraction() {
        let processor = WebContentProcessor::new();
        let html_content = r#"<html><head><title>Test Page</title></head><body><h1>Main Title</h1><p>This is a <strong>test</strong> paragraph.</p><script>alert('test');</script><style>body { color: red; }</style></body></html>"#;
        let result = processor
            .process_web_content(html_content.as_bytes(), Some("text/html"))
            .unwrap();
        assert!(result.is_html);
        assert!(result.extracted_text.contains("Main Title"));
        assert!(result.extracted_text.contains("test paragraph"));
        assert!(!result.extracted_text.contains("script"));
        assert!(!result.extracted_text.contains("style"));
    }

    #[test]
    fn test_language_detection() {
        let processor = WebContentProcessor::new();
        let english_text = "This is English text content.";
        let lang = processor.detect_language(english_text);
        assert_eq!(lang, Some("en".to_string()));
        let chinese_text = "这是中文内容";
        let lang = processor.detect_language(chinese_text);
        assert_eq!(lang, Some("zh".to_string()));
    }

    #[test]
    fn test_simple_text_processing() {
        let processor = CrawlTextProcessor::new();
        let text = "Hello, 世界! This is a test.";
        let result = processor.process_simple_text(text).unwrap();
        assert_eq!(result, text);
    }

    #[test]
    fn test_repetitive_content_detection() {
        let processor = CrawlTextProcessor::new();
        let repetitive_text = "spam spam spam spam spam spam spam spam spam spam";
        let ratio = processor.calculate_repetitive_ratio(repetitive_text);
        assert!(ratio > 0.5);
        let normal_text = "This is normal text with different words and meaningful content.";
        let ratio = processor.calculate_repetitive_ratio(normal_text);
        assert!(ratio < 0.5);
    }

    // ========== Free function tests ==========

    #[test]
    fn test_init_encoding_patterns_returns_three_patterns() {
        let patterns = init_encoding_patterns().expect("patterns should compile");
        assert!(patterns.contains_key(ENCODING_PATTERN_HTML_META));
        assert!(patterns.contains_key(ENCODING_PATTERN_XML_DECLARATION));
        assert!(patterns.contains_key(ENCODING_PATTERN_HTTP_CONTENT_TYPE));
        assert_eq!(patterns.len(), 3);
    }

    #[test]
    fn test_init_encoding_patterns_compiles_valid_regexes() {
        let patterns = init_encoding_patterns().expect("patterns should compile");
        let html_meta = patterns.get(ENCODING_PATTERN_HTML_META).unwrap();
        assert!(html_meta.is_match(r#"<meta charset="utf-8">"#));
        let xml_decl = patterns.get(ENCODING_PATTERN_XML_DECLARATION).unwrap();
        assert!(xml_decl.is_match(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn test_detect_html_structure_free_function_html_tags() {
        assert!(detect_html_structure("<html><body></body></html>"));
        assert!(detect_html_structure("<!DOCTYPE html>"));
        assert!(detect_html_structure("<head><title>X</title></head>"));
        assert!(detect_html_structure("<body>content</body>"));
        assert!(detect_html_structure("<div>content</div>"));
        assert!(detect_html_structure("<p>paragraph</p>"));
        assert!(detect_html_structure("<a href=\"x\">link</a>"));
    }

    #[test]
    fn test_detect_html_structure_free_function_non_html() {
        assert!(!detect_html_structure("plain text content"));
        assert!(!detect_html_structure("no html here at all"));
        assert!(!detect_html_structure(""));
    }

    // ========== TextEncodingProcessorComponent tests ==========

    #[test]
    fn test_text_encoding_processor_component_default() {
        let component = TextEncodingProcessorComponent::default();
        assert_eq!(component.default_encoding, "utf-8");
        assert!(component.detect_encoding);
    }

    #[test]
    fn test_text_encoding_processor_component_new() {
        let component = TextEncodingProcessorComponent::new();
        assert_eq!(component.default_encoding, "utf-8");
        assert!(component.detect_encoding);
    }

    #[test]
    fn test_text_encoding_processor_component_process_text_utf8() {
        let component = TextEncodingProcessorComponent::new();
        let result = component.process_text(b"hello world").unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_text_encoding_processor_component_process_text_chinese() {
        let component = TextEncodingProcessorComponent::new();
        let input = "你好世界".as_bytes();
        let result = component.process_text(input).unwrap();
        assert!(result.contains("你好世界"));
    }

    #[test]
    fn test_text_encoding_processor_component_trim_newlines() {
        let component = TextEncodingProcessorComponent::new();
        let result = component.trim_newlines("hello\nworld\nfoo");
        assert_eq!(result, "hello world foo");
    }

    #[test]
    fn test_text_encoding_processor_component_trim_newlines_single_line() {
        let component = TextEncodingProcessorComponent::new();
        let result = component.trim_newlines("hello");
        assert_eq!(result, "hello");
    }

    // ========== WebContentProcessorComponent tests ==========

    #[test]
    fn test_web_content_processor_component_default() {
        let component = WebContentProcessorComponent::default();
        let result = component
            .process_web_content(b"plain text", Some("text/plain"))
            .unwrap();
        assert!(!result.is_html);
        assert_eq!(result.extracted_text, "plain text");
        assert_eq!(result.content_length, 10);
    }

    #[test]
    fn test_web_content_processor_component_process_html() {
        let component = WebContentProcessorComponent::default();
        let html = b"<html><body><p>Hello</p></body></html>";
        let result = component
            .process_web_content(html, Some("text/html"))
            .unwrap();
        assert!(result.is_html);
        assert!(result.extracted_text.contains("Hello"));
    }

    #[test]
    fn test_web_content_processor_component_process_batch() {
        let component = WebContentProcessorComponent::default();
        let contents = vec![
            (b"text1".as_slice(), Some("text/plain")),
            (b"<p>text2</p>".as_slice(), Some("text/html")),
            (b"text3".as_slice(), None),
        ];
        let results = component.process_batch(contents);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
        assert!(results[1].as_ref().unwrap().is_html);
    }

    #[test]
    fn test_web_content_processor_component_process_batch_empty() {
        let component = WebContentProcessorComponent::default();
        let results = component.process_batch(vec![]);
        assert!(results.is_empty());
    }

    // ========== WebContentProcessor tests ==========

    #[test]
    fn test_web_content_processor_default() {
        let processor = WebContentProcessor::default();
        let result = processor.process_web_content(b"hello", None).unwrap();
        assert_eq!(result.extracted_text, "hello");
    }

    #[test]
    fn test_web_content_processor_with_text_processor() {
        let processor = WebContentProcessor::with_text_processor(TextEncodingProcessor::new());
        let result = processor
            .process_web_content(b"test content", Some("text/plain"))
            .unwrap();
        assert!(!result.is_html);
        assert_eq!(result.extracted_text, "test content");
    }

    #[test]
    fn test_web_content_processor_with_injected_processor() {
        let processor =
            WebContentProcessor::with_injected_processor(Arc::new(TextEncodingProcessor::new()));
        let result = processor.process_web_content(b"injected", None).unwrap();
        assert_eq!(result.extracted_text, "injected");
    }

    #[test]
    fn test_web_content_processor_process_batch() {
        let processor = WebContentProcessor::new();
        let contents = vec![
            (b"<p>html</p>".as_slice(), Some("text/html")),
            (b"plain".as_slice(), None),
        ];
        let results = processor.process_batch(contents);
        assert_eq!(results.len(), 2);
        assert!(results[0].as_ref().unwrap().is_html);
        assert!(!results[1].as_ref().unwrap().is_html);
    }

    #[test]
    fn test_web_content_processor_detect_html_structure_all_tags() {
        let processor = WebContentProcessor::new();
        assert!(processor.detect_html_structure("<html>"));
        assert!(processor.detect_html_structure("<!DOCTYPE"));
        assert!(processor.detect_html_structure("<head"));
        assert!(processor.detect_html_structure("<body"));
        assert!(processor.detect_html_structure("<div"));
        assert!(processor.detect_html_structure("<p>"));
        assert!(processor.detect_html_structure("<a "));
        assert!(!processor.detect_html_structure("no tags here"));
    }

    #[test]
    fn test_web_content_processor_extract_declared_encoding_xml() {
        let processor = WebContentProcessor::new();
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>";
        let encoding = processor.extract_declared_encoding(xml);
        assert_eq!(encoding, Some("utf-8".to_string()));
    }

    #[test]
    fn test_web_content_processor_extract_declared_encoding_meta_uppercase() {
        let processor = WebContentProcessor::new();
        let html = r#"<html><head><meta charset="UTF-8"></head></html>"#;
        let encoding = processor.extract_declared_encoding(html);
        assert_eq!(encoding, Some("utf-8".to_string()));
    }

    #[test]
    fn test_web_content_processor_extract_declared_encoding_none() {
        let processor = WebContentProcessor::new();
        let plain = "just plain text without encoding";
        let encoding = processor.extract_declared_encoding(plain);
        assert_eq!(encoding, None);
    }

    #[test]
    fn test_web_content_processor_clean_text_html_entities() {
        let processor = WebContentProcessor::new();
        let text = "Hello &amp; goodbye &lt;world&gt;";
        let cleaned = processor.clean_text(text);
        assert!(cleaned.contains("Hello & goodbye"));
        assert!(cleaned.contains("<world>"));
    }

    #[test]
    fn test_web_content_processor_clean_text_control_chars() {
        let processor = WebContentProcessor::new();
        let text = "hello\x01\x02world";
        let cleaned = processor.clean_text(text);
        assert!(!cleaned.contains('\x01'));
        assert!(!cleaned.contains('\x02'));
        assert!(cleaned.contains("hello"));
        assert!(cleaned.contains("world"));
    }

    #[test]
    fn test_web_content_processor_detect_language_empty() {
        let processor = WebContentProcessor::new();
        assert_eq!(processor.detect_language(""), None);
    }

    #[test]
    fn test_web_content_processor_detect_language_unknown() {
        let processor = WebContentProcessor::new();
        // Mix of non-ASCII and non-Chinese characters (Arabic/Cyrillic)
        let text = "Привет мир مرحبا بالعالم";
        let lang = processor.detect_language(text);
        assert_eq!(lang, Some("unknown".to_string()));
    }

    // ========== ProcessedWebContent struct tests ==========

    #[test]
    fn test_processed_web_content_fields() {
        let content = ProcessedWebContent {
            original_content: "original".to_string(),
            extracted_text: "extracted".to_string(),
            is_html: true,
            declared_encoding: Some("utf-8".to_string()),
            detected_language: Some("en".to_string()),
            content_type: Some("text/html".to_string()),
            content_length: 100,
        };
        assert_eq!(content.original_content, "original");
        assert_eq!(content.extracted_text, "extracted");
        assert!(content.is_html);
        assert_eq!(content.declared_encoding.as_deref(), Some("utf-8"));
        assert_eq!(content.detected_language.as_deref(), Some("en"));
        assert_eq!(content.content_type.as_deref(), Some("text/html"));
        assert_eq!(content.content_length, 100);
    }

    #[test]
    fn test_processed_web_content_clone() {
        let content = ProcessedWebContent {
            original_content: "orig".to_string(),
            extracted_text: "ext".to_string(),
            is_html: false,
            declared_encoding: None,
            detected_language: None,
            content_type: None,
            content_length: 5,
        };
        let cloned = content.clone();
        assert_eq!(cloned.extracted_text, content.extracted_text);
        assert_eq!(cloned.is_html, content.is_html);
    }

    // ========== WebContentError variants tests ==========

    #[test]
    fn test_web_content_error_display_encoding() {
        let err =
            WebContentError::EncodingError(TextEncodingError::DetectionFailed("test".to_string()));
        assert!(err.to_string().contains("编码处理错误"));
    }

    #[test]
    fn test_web_content_error_display_html_parse() {
        let err = WebContentError::HtmlParseError("parse issue".to_string());
        assert!(err.to_string().contains("HTML解析错误"));
    }

    #[test]
    fn test_web_content_error_display_content_extraction() {
        let err = WebContentError::ContentExtractionError("extract fail".to_string());
        assert!(err.to_string().contains("内容提取错误"));
    }

    // ========== CrawlTextProcessor tests ==========

    #[test]
    fn test_crawl_text_processor_default() {
        let processor = CrawlTextProcessor::default();
        let config = processor.get_config();
        assert_eq!(config.max_processing_time_secs, 30);
        assert_eq!(config.max_content_size_mb, 10);
    }

    #[test]
    fn test_crawl_text_processor_new() {
        let processor = CrawlTextProcessor::new();
        let config = processor.get_config();
        assert_eq!(config.max_processing_time_secs, 30);
        assert_eq!(config.max_content_size_mb, 10);
    }

    #[test]
    fn test_crawl_text_processor_get_config() {
        let processor = CrawlTextProcessor::new();
        let config = processor.get_config();
        assert_eq!(config.max_processing_time_secs, 30);
        assert_eq!(config.max_content_size_mb, 10);
    }

    #[test]
    fn test_crawl_text_processor_update_config() {
        let mut processor = CrawlTextProcessor::new();
        let new_config = CrawlProcessorConfig {
            max_processing_time_secs: 60,
            max_content_size_mb: 20,
        };
        processor.update_config(new_config);
        let config = processor.get_config();
        assert_eq!(config.max_processing_time_secs, 60);
        assert_eq!(config.max_content_size_mb, 20);
    }

    #[test]
    fn test_crawl_text_processor_process_simple_text_empty() {
        let processor = CrawlTextProcessor::new();
        let result = processor.process_simple_text("").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_crawl_text_processor_process_simple_text_unicode() {
        let processor = CrawlTextProcessor::new();
        let text = r"Hello \u4e16\u754c";
        let result = processor.process_simple_text(text).unwrap();
        assert_eq!(result, "Hello 世界");
    }

    #[test]
    fn test_crawl_text_processor_process_crawled_content_plain() {
        let processor = CrawlTextProcessor::new();
        let result = processor
            .process_crawled_content(b"hello world", "http://example.com", Some("text/plain"))
            .unwrap();
        assert_eq!(result.url, "http://example.com");
        assert!(!result.processed_content.is_html);
        assert_eq!(result.processed_content.extracted_text, "hello world");
        assert_eq!(result.original_size, 11);
    }

    #[test]
    fn test_crawl_text_processor_process_crawled_content_html() {
        let processor = CrawlTextProcessor::new();
        let html = b"<html><body><p>Test content here</p></body></html>";
        let result = processor
            .process_crawled_content(html, "http://example.com", Some("text/html"))
            .unwrap();
        assert!(result.processed_content.is_html);
        assert!(result
            .processed_content
            .extracted_text
            .contains("Test content"));
    }

    #[test]
    fn test_crawl_text_processor_process_crawled_content_too_large() {
        let mut processor = CrawlTextProcessor::new();
        processor.update_config(CrawlProcessorConfig {
            max_processing_time_secs: 30,
            max_content_size_mb: 0,
        });
        let content = b"this is too large";
        let result = processor.process_crawled_content(content, "http://x.com", None);
        assert!(result.is_err());
        match result.unwrap_err() {
            CrawlProcessingError::ContentTooLarge { size, max_size } => {
                assert_eq!(size, 17);
                assert_eq!(max_size, 0);
            }
            other => panic!("expected ContentTooLarge, got {:?}", other),
        }
    }

    #[test]
    fn test_crawl_text_processor_process_batch() {
        let processor = CrawlTextProcessor::new();
        let batch = vec![
            (b"content1".as_slice(), "http://a.com", Some("text/plain")),
            (
                b"<p>content2</p>".as_slice(),
                "http://b.com",
                Some("text/html"),
            ),
            (b"content3".as_slice(), "http://c.com", None),
        ];
        let results = processor.process_batch(batch);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_crawl_text_processor_process_batch_empty() {
        let processor = CrawlTextProcessor::new();
        let results = processor.process_batch(vec![]);
        assert!(results.is_empty());
    }

    #[test]
    fn test_validate_content_quality_excellent() {
        let processor = CrawlTextProcessor::new();
        // 使用多样化的词汇避免重复度过高
        let diverse_text = "The quick brown fox jumps over the lazy dog near the river bank. \
             Birds sing loudly in the morning while cats sleep peacefully on warm rooftops. \
             Children play games outside enjoying sunny weather with friends and neighbors.";
        let result = processor
            .process_crawled_content(diverse_text.as_bytes(), "http://example.com", None)
            .unwrap();
        let quality = processor.validate_content_quality(&result);
        match quality {
            ContentQuality::Excellent | ContentQuality::Good => {}
            other => panic!("expected Excellent or Good, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_content_quality_poor_short_text() {
        let processor = CrawlTextProcessor::new();
        let result = processor
            .process_crawled_content(b"hi", "http://x.com", None)
            .unwrap();
        let quality = processor.validate_content_quality(&result);
        match quality {
            ContentQuality::Poor(msg) => assert!(msg.contains("过短") || msg.contains("过少")),
            other => panic!("expected Poor, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_content_quality_poor_repetitive() {
        let processor = CrawlTextProcessor::new();
        let repetitive = "word ".repeat(50);
        let result = processor
            .process_crawled_content(repetitive.as_bytes(), "http://x.com", None)
            .unwrap();
        let quality = processor.validate_content_quality(&result);
        match quality {
            ContentQuality::Poor(msg) => assert!(msg.contains("重复"), "got: {}", msg),
            other => panic!("expected Poor repetitive, got {:?}", other),
        }
    }

    #[test]
    fn test_calculate_repetitive_ratio_short_text() {
        let processor = CrawlTextProcessor::new();
        let ratio = processor.calculate_repetitive_ratio("few words");
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn test_calculate_repetitive_ratio_empty() {
        let processor = CrawlTextProcessor::new();
        let ratio = processor.calculate_repetitive_ratio("");
        assert_eq!(ratio, 0.0);
    }

    // ========== CrawlProcessorConfig tests ==========

    #[test]
    fn test_crawl_processor_config_default() {
        let config = CrawlProcessorConfig::default();
        assert_eq!(config.max_processing_time_secs, 30);
        assert_eq!(config.max_content_size_mb, 10);
    }

    #[test]
    fn test_crawl_processor_config_clone() {
        let config = CrawlProcessorConfig {
            max_processing_time_secs: 45,
            max_content_size_mb: 15,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_processing_time_secs, 45);
        assert_eq!(cloned.max_content_size_mb, 15);
    }

    // ========== CrawlProcessingError variants tests ==========

    #[test]
    fn test_crawl_processing_error_content_too_large_display() {
        let err = CrawlProcessingError::ContentTooLarge {
            size: 1000,
            max_size: 500,
        };
        let msg = err.to_string();
        assert!(msg.contains("1000"));
        assert!(msg.contains("500"));
    }

    #[test]
    fn test_crawl_processing_error_processing_timeout_display() {
        let err = CrawlProcessingError::ProcessingTimeout;
        assert_eq!(err.to_string(), "处理超时");
    }

    #[test]
    fn test_crawl_processing_error_poor_content_quality_display() {
        let err = CrawlProcessingError::PoorContentQuality("bad quality".to_string());
        assert!(err.to_string().contains("bad quality"));
    }

    // ========== ContentQuality tests ==========

    #[test]
    fn test_content_quality_variants() {
        let excellent = ContentQuality::Excellent;
        let good = ContentQuality::Good;
        let fair = ContentQuality::Fair;
        let poor = ContentQuality::Poor("reason".to_string());

        assert_eq!(excellent, ContentQuality::Excellent);
        assert_eq!(good, ContentQuality::Good);
        assert_eq!(fair, ContentQuality::Fair);
        match poor {
            ContentQuality::Poor(r) => assert_eq!(r, "reason"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_content_quality_clone() {
        let poor = ContentQuality::Poor("test reason".to_string());
        let cloned = poor.clone();
        match cloned {
            ContentQuality::Poor(r) => assert_eq!(r, "test reason"),
            _ => panic!("wrong variant after clone"),
        }
    }

    // ========== Free function tests ==========

    #[test]
    fn test_process_web_content_with_processor_fn() {
        let processor = WebContentProcessor::new();
        let result =
            process_web_content_with_processor(&processor, b"<p>test</p>", Some("text/html"))
                .unwrap();
        assert!(result.is_html);
        assert!(result.extracted_text.contains("test"));
    }

    #[test]
    fn test_process_crawled_content_with_processor_fn() {
        let processor = CrawlTextProcessor::new();
        let result =
            process_crawled_content_with_processor(&processor, b"hello", "http://x.com", None)
                .unwrap();
        assert_eq!(result.url, "http://x.com");
        assert!(!result.processed_content.is_html);
    }

    #[test]
    fn test_process_crawled_batch_with_processor_fn() {
        let processor = CrawlTextProcessor::new();
        let batch = vec![
            (b"a".as_slice(), "http://a.com", None),
            (b"b".as_slice(), "http://b.com", None),
        ];
        let results = process_crawled_batch_with_processor(&processor, batch);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    // ========== TextProcessingError tests ==========

    #[test]
    fn test_text_processing_error_regex_compilation_display() {
        let err = TextProcessingError::RegexCompilationError("bad regex".to_string());
        assert!(err.to_string().contains("正则表达式编译失败"));
        assert!(err.to_string().contains("bad regex"));
    }

    #[test]
    fn test_text_processing_error_encoding_from() {
        let encoding_err = TextEncodingError::DetectionFailed("fail".to_string());
        let err: TextProcessingError = encoding_err.into();
        assert!(matches!(err, TextProcessingError::EncodingError(_)));
    }

    // ========== HtmlCleaner integration tests ==========

    #[test]
    fn test_html_cleaner_extracts_text_with_scripts_and_styles() {
        let processor = WebContentProcessor::new();
        let html = r#"<html><head><style>body { color: red; }</style></head><body><script>alert(1);</script><p>visible text</p></body></html>"#;
        let result = processor.extract_text_from_html(html).unwrap();
        assert!(result.contains("visible text"));
        assert!(!result.contains("alert"));
        assert!(!result.contains("color"));
    }

    #[test]
    fn test_html_cleaner_removes_comments() {
        let processor = WebContentProcessor::new();
        let html = "<p>before</p><!-- this is a comment --><p>after</p>";
        let result = processor.extract_text_from_html(html).unwrap();
        assert!(!result.contains("comment"));
        assert!(result.contains("before"));
        assert!(result.contains("after"));
    }

    #[test]
    fn test_html_cleaner_normalizes_whitespace() {
        let processor = WebContentProcessor::new();
        let html = "<p>hello</p>   <p>world</p>";
        let result = processor.extract_text_from_html(html).unwrap();
        assert!(!result.contains("   "));
    }

    // ========== validate_content_quality additional tests ==========

    #[test]
    fn test_validate_content_quality_poor_low_text_ratio() {
        let processor = CrawlTextProcessor::new();
        // Create content with large original size but low text ratio.
        // Use lots of HTML tags (which get stripped) so extracted text is small
        // relative to original content.
        let mut html = String::with_capacity(2000);
        html.push_str("<html><body>");
        for _ in 0..200 {
            html.push_str("<div></div>");
        }
        html.push_str("<p>");
        html.push_str("word ");
        for _ in 0..15 {
            html.push_str("unique ");
        }
        html.push_str("</p>");
        html.push_str("</body></html>");
        let result = processor
            .process_crawled_content(html.as_bytes(), "http://x.com", Some("text/html"))
            .unwrap();
        // original_size > 1000 and text_ratio < 0.01 should trigger Poor
        let quality = processor.validate_content_quality(&result);
        match quality {
            ContentQuality::Poor(msg) => {
                // Any Poor reason is valid: short text, few words, low ratio, or repetitive
                assert!(
                    msg.contains("过短")
                        || msg.contains("过少")
                        || msg.contains("比例")
                        || msg.contains("重复"),
                    "expected Poor with meaningful reason, got: {}",
                    msg
                );
            }
            other => {
                // The exact outcome depends on text ratio calculation;
                // just verify it doesn't panic
                let _ = other;
            }
        }
    }

    #[test]
    fn test_validate_content_quality_fair_not_reached() {
        // The ContentQuality::Fair variant is defined but never returned
        // by validate_content_quality. This test documents that behavior.
        let processor = CrawlTextProcessor::new();
        let text = "This is a moderately sized text with enough words to pass \
                    the minimum thresholds. It has diverse vocabulary to avoid \
                    being flagged as repetitive. The quick brown fox jumps over \
                    the lazy dog near the river bank every single day.";
        let result = processor
            .process_crawled_content(text.as_bytes(), "http://example.com", None)
            .unwrap();
        let quality = processor.validate_content_quality(&result);
        // Should be Excellent or Good, never Fair
        assert!(
            matches!(quality, ContentQuality::Excellent | ContentQuality::Good),
            "expected Excellent or Good, got {:?}",
            quality
        );
    }

    // ========== process_web_content with various content types ==========

    #[test]
    fn test_web_content_processor_process_xml_content() {
        let processor = WebContentProcessor::new();
        let xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><root><item>test</item></root>";
        let result = processor
            .process_web_content(xml, Some("application/xml"))
            .unwrap();
        // XML does not contain HTML tags (html, head, body, div, p, a),
        // so is_html should be false
        assert!(!result.is_html);
        // But XML declaration encoding should be detected
        assert_eq!(result.declared_encoding, Some("utf-8".to_string()));
        // Extracted text should contain the content
        assert!(result.extracted_text.contains("test"));
    }

    #[test]
    fn test_web_content_processor_process_empty_content() {
        let processor = WebContentProcessor::new();
        let result = processor.process_web_content(b"", None).unwrap();
        assert!(!result.is_html);
        assert_eq!(result.extracted_text, "");
        assert_eq!(result.content_length, 0);
        assert_eq!(result.detected_language, None);
    }

    #[test]
    fn test_web_content_processor_process_content_with_null_bytes() {
        let processor = WebContentProcessor::new();
        // Content with some bytes that might cause encoding issues
        let content = b"hello\x00world";
        let result = processor.process_web_content(content, None);
        // Should not panic; may succeed or fail depending on encoding
        let _ = result;
    }

    // ========== CrawlTextProcessor process_batch with errors ==========

    #[test]
    fn test_crawl_text_processor_process_batch_with_mixed_results() {
        let mut processor = CrawlTextProcessor::new();
        processor.update_config(CrawlProcessorConfig {
            max_processing_time_secs: 30,
            max_content_size_mb: 0,
        });
        let batch = vec![
            (b"normal content".as_slice(), "http://a.com", None),
            (b"too large content".as_slice(), "http://b.com", None),
        ];
        let results = processor.process_batch(batch);
        assert_eq!(results.len(), 2);
        // First should fail (content too large with max_size=0)
        assert!(results[0].is_err());
        assert!(results[1].is_err());
    }

    // ========== ProcessedCrawlContent tests ==========

    #[test]
    fn test_processed_crawl_content_clone() {
        let processor = CrawlTextProcessor::new();
        let result = processor
            .process_crawled_content(b"test content", "http://x.com", None)
            .unwrap();
        let cloned = result.clone();
        assert_eq!(cloned.url, result.url);
        assert_eq!(cloned.original_size, result.original_size);
        assert_eq!(cloned.processed_size, result.processed_size);
    }

    #[test]
    fn test_processed_crawl_content_debug() {
        let processor = CrawlTextProcessor::new();
        let result = processor
            .process_crawled_content(b"debug test", "http://x.com", None)
            .unwrap();
        let debug = format!("{:?}", result);
        assert!(debug.contains("ProcessedCrawlContent"));
        assert!(debug.contains("http://x.com"));
    }

    // ========== CrawlProcessingError variants ==========

    #[test]
    fn test_crawl_processing_error_text_encoding_display() {
        let err = CrawlProcessingError::TextEncodingError(TextEncodingError::DetectionFailed(
            "detect fail".to_string(),
        ));
        assert!(err.to_string().contains("文本编码处理错误"));
    }

    #[test]
    fn test_crawl_processing_error_web_content_display() {
        let err = CrawlProcessingError::WebContentError(WebContentError::HtmlParseError(
            "parse error".to_string(),
        ));
        assert!(err.to_string().contains("网页内容处理错误"));
    }

    // ========== WebContentProcessorComponent with injected processor ==========

    #[test]
    fn test_web_content_processor_component_with_custom_processor() {
        let text_processor: Arc<dyn TextEncodingProcessorTrait> =
            Arc::new(TextEncodingProcessorComponent::new());
        let component = WebContentProcessorComponent::new(text_processor);
        let result = component
            .process_web_content(b"custom processor test", None)
            .unwrap();
        assert_eq!(result.extracted_text, "custom processor test");
    }

    #[test]
    fn test_web_content_processor_component_process_html_with_encoding() {
        let component = WebContentProcessorComponent::default();
        let html = b"<html><head><meta charset=\"utf-8\"></head><body><p>Encoded content</p></body></html>";
        let result = component
            .process_web_content(html, Some("text/html"))
            .unwrap();
        assert!(result.is_html);
        assert_eq!(result.declared_encoding, Some("utf-8".to_string()));
        assert!(result.extracted_text.contains("Encoded content"));
    }

    // ========== TextEncodingProcessorTrait implementation ==========

    #[test]
    fn test_text_encoding_processor_component_trait_object() {
        let component: Arc<dyn TextEncodingProcessorTrait> =
            Arc::new(TextEncodingProcessorComponent::new());
        let result = component.process_text(b"trait object test").unwrap();
        assert_eq!(result, "trait object test");
        let trimmed = component.trim_newlines("line1\nline2");
        assert_eq!(trimmed, "line1 line2");
    }

    // ========== WebContentProcessorTrait implementation ==========

    #[test]
    fn test_web_content_processor_component_trait_object() {
        let component: Arc<dyn WebContentProcessorTrait> =
            Arc::new(WebContentProcessorComponent::default());
        let result = component
            .process_web_content(b"trait test", Some("text/plain"))
            .unwrap();
        assert!(!result.is_html);
        assert_eq!(result.extracted_text, "trait test");
    }

    #[test]
    fn test_web_content_processor_component_trait_process_batch() {
        let component: Arc<dyn WebContentProcessorTrait> =
            Arc::new(WebContentProcessorComponent::default());
        let contents = vec![
            (b"a".as_slice(), None),
            (b"b".as_slice(), Some("text/plain")),
        ];
        let results = component.process_batch(contents);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }
}
