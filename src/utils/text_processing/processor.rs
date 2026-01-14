// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::utils::text_processing::encoding::{
    process_text_encoding, TextEncodingError, TextEncodingProcessor,
};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, error, info};

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

/// 网页内容处理器
pub struct WebContentProcessor {
    text_processor: &'static TextEncodingProcessor,
    html_cleaner: HtmlCleaner,
    encoding_patterns: HashMap<&'static str, Regex>,
}

static WEB_PROCESSOR: Lazy<WebContentProcessor> = Lazy::new(WebContentProcessor::new);

impl Default for WebContentProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl WebContentProcessor {
    pub fn new() -> Self {
        Self {
            text_processor: TextEncodingProcessor::global(),
            html_cleaner: HtmlCleaner::new(),
            encoding_patterns: Self::init_encoding_patterns()
                .expect("Failed to initialize encoding patterns"),
        }
    }

    pub fn global() -> &'static Self {
        &WEB_PROCESSOR
    }

    fn init_encoding_patterns() -> Result<HashMap<&'static str, Regex>, TextProcessingError> {
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

struct HtmlCleaner {
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

pub fn process_web_content(
    content: &[u8],
    content_type: Option<&str>,
) -> Result<ProcessedWebContent, WebContentError> {
    WebContentProcessor::global().process_web_content(content, content_type)
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
        thread::spawn(move || {
            let result = process_web_content(&content, content_type.as_deref());
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
        match process_text_encoding(text.as_bytes()) {
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

pub fn process_crawled_content(
    content: &[u8],
    url: &str,
    content_type: Option<&str>,
) -> Result<ProcessedCrawlContent, CrawlProcessingError> {
    CrawlTextProcessor::new().process_crawled_content(content, url, content_type)
}

pub fn process_crawled_batch(
    batch: Vec<(&[u8], &str, Option<&str>)>,
) -> Vec<Result<ProcessedCrawlContent, CrawlProcessingError>> {
    CrawlTextProcessor::new().process_batch(batch)
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
}
