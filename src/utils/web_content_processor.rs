// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::utils::text_encoding::{TextEncodingProcessor, TextEncodingError};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use tracing::{debug, info, error};

/// 网页内容处理器，专门处理爬虫抓取的内容
pub struct WebContentProcessor {
    text_processor: &'static TextEncodingProcessor,
    html_cleaner: HtmlCleaner,
    encoding_patterns: HashMap<String, Regex>,
}

/// HTML清理器
struct HtmlCleaner {
    script_regex: Regex,
    style_regex: Regex,
    comment_regex: Regex,
    tag_regex: Regex,
    whitespace_regex: Regex,
}

/// 全局网页内容处理器实例
static WEB_PROCESSOR: Lazy<WebContentProcessor> = Lazy::new(WebContentProcessor::new);

impl WebContentProcessor {
    /// 创建新的网页内容处理器
    pub fn new() -> Self {
        Self {
            text_processor: TextEncodingProcessor::global(),
            html_cleaner: HtmlCleaner::new(),
            encoding_patterns: Self::init_encoding_patterns(),
        }
    }

    /// 获取全局处理器实例
    pub fn global() -> &'static Self {
        &WEB_PROCESSOR
    }

    /// 初始化编码检测模式
    fn init_encoding_patterns() -> HashMap<String, Regex> {
        let mut patterns = HashMap::new();
        
        // HTML meta标签编码声明
        patterns.insert(
            "html_meta".to_string(),
            Regex::new(r#"(?i)<meta[^>]*charset\s*=\s*["']?([^"'>\s]+)["']?[^>]*>"#).unwrap()
        );
        
        // XML声明编码
        patterns.insert(
            "xml_declaration".to_string(),
            Regex::new(r#"(?i)<\?xml[^>]*encoding\s*=\s*["']?([^"'>\s]+)["']?[^>]*\?>"#).unwrap()
        );
        
        // HTTP Content-Type头编码
        patterns.insert(
            "http_content_type".to_string(),
            Regex::new(r#"(?i)charset\s*=\s*([^;\s]+)"#).unwrap()
        );
        
        patterns
    }

    /// 处理完整的网页内容
    pub fn process_web_content(&self, content: &[u8], content_type: Option<&str>) -> Result<ProcessedWebContent, WebContentError> {
        info!("开始处理网页内容，大小: {} 字节", content.len());
        
        // 1. 检测和转换编码
        let text_content = self.process_encoding(content)?;
        
        // 2. 检测HTML/XML结构
        let is_html = self.detect_html_structure(&text_content);
        let declared_encoding = self.extract_declared_encoding(&text_content);
        
        debug!("检测到HTML结构: {}, 声明编码: {:?}", is_html, declared_encoding);
        
        // 3. 提取文本内容
        let extracted_text = if is_html {
            self.extract_text_from_html(&text_content)?
        } else {
            text_content.clone()
        };
        
        // 4. 清理和规范化文本
        let cleaned_text = self.clean_text(&extracted_text);
        
        // 5. 检测语言
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

    /// 处理文本编码
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

    /// 检测HTML结构
    fn detect_html_structure(&self, content: &str) -> bool {
        // 检查是否包含HTML标签
        content.contains("<html") || 
        content.contains("<!DOCTYPE") || 
        content.contains("<head") || 
        content.contains("<body") ||
        content.contains("<div") ||
        content.contains("<p>") ||
        content.contains("<a ")
    }

    /// 提取声明的编码信息
    fn extract_declared_encoding(&self, content: &str) -> Option<String> {
        // 检查HTML meta标签
        if let Some(captures) = self.encoding_patterns.get("html_meta")?.captures(content) {
            if let Some(encoding) = captures.get(1) {
                return Some(encoding.as_str().to_lowercase());
            }
        }
        
        // 检查XML声明
        if let Some(captures) = self.encoding_patterns.get("xml_declaration")?.captures(content) {
            if let Some(encoding) = captures.get(1) {
                return Some(encoding.as_str().to_lowercase());
            }
        }
        
        None
    }

    /// 从HTML中提取文本内容
    fn extract_text_from_html(&self, html: &str) -> Result<String, WebContentError> {
        self.html_cleaner.extract_text(html)
    }

    /// 清理和规范化文本
    fn clean_text(&self, text: &str) -> String {
        let mut cleaned = text.to_string();
        
        // 解码HTML实体
        cleaned = html_escape::decode_html_entities(&cleaned).to_string();
        
        // 移除不可见字符（但保留空白字符）
        cleaned = cleaned.chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .collect();
        
        // 去除首位换行符
        cleaned = self.text_processor.trim_newlines(&cleaned);
        
        cleaned.trim().to_string()
    }

    /// 检测语言（简单实现，可以扩展为使用更复杂的库）
    fn detect_language(&self, text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }
        
        // 简单的语言检测逻辑
        let chinese_ratio = text.chars().filter(|c| {
            let code = *c as u32;
            // CJK Unified Ideographs, CJK Unified Ideographs Extension A, CJK Compatibility Ideographs
            (0x4E00 <= code && code <= 0x9FFF) || 
            (0x3400 <= code && code <= 0x4DBF) || 
            (0xF900 <= code && code <= 0xFAFF)
        }).count() as f32 / text.len() as f32;
        
        if chinese_ratio > 0.3 {
            Some("zh".to_string())
        } else if text.chars().filter(|c| c.is_ascii()).count() as f32 / text.len() as f32 > 0.8 {
            Some("en".to_string())
        } else {
            Some("unknown".to_string())
        }
    }

    /// 批量处理网页内容
    pub fn process_batch(&self, contents: Vec<(&[u8], Option<&str>)>) -> Vec<Result<ProcessedWebContent, WebContentError>> {
        contents.into_iter()
            .map(|(content, content_type)| self.process_web_content(content, content_type))
            .collect()
    }

    /// 获取处理器统计信息
    pub fn get_stats(&self) -> WebProcessorStats {
        WebProcessorStats {
            text_processor_stats: self.text_processor.get_stats(),
        }
    }
}

impl HtmlCleaner {
    /// 创建新的HTML清理器
    fn new() -> Self {
        Self {
            script_regex: Regex::new(r#"(?is)<script[^>]*>.*?</script>"#).unwrap(),
            style_regex: Regex::new(r#"(?is)<style[^>]*>.*?</style>"#).unwrap(),
            comment_regex: Regex::new(r#"(?is)<!--.*?-->"#).unwrap(),
            tag_regex: Regex::new(r#"(?is)<[^>]+>"#).unwrap(),
            whitespace_regex: Regex::new(r#"\s+"#).unwrap(),
        }
    }

    /// 从HTML中提取文本
    fn extract_text(&self, html: &str) -> Result<String, WebContentError> {
        let mut text = html.to_string();
        
        // 移除script标签
        text = self.script_regex.replace_all(&text, "").to_string();
        
        // 移除style标签
        text = self.style_regex.replace_all(&text, "").to_string();
        
        // 移除HTML注释
        text = self.comment_regex.replace_all(&text, "").to_string();
        
        // 替换HTML标签为空格
        text = self.tag_regex.replace_all(&text, " ").to_string();
        
        // 规范化空白字符
        text = self.normalize_whitespace(&text);
        
        Ok(text.trim().to_string())
    }

    /// 规范化空白字符
    fn normalize_whitespace(&self, text: &str) -> String {
        self.whitespace_regex.replace_all(text, " ").to_string()
    }
}

/// 处理后的网页内容
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

/// 网页内容处理错误
#[derive(Debug, thiserror::Error)]
pub enum WebContentError {
    #[error("编码处理错误: {0}")]
    EncodingError(#[from] TextEncodingError),
    
    #[error("HTML解析错误: {0}")]
    HtmlParseError(String),
    
    #[error("内容提取错误: {0}")]
    ContentExtractionError(String),
}

/// 处理器统计信息
#[derive(Debug, Clone)]
pub struct WebProcessorStats {
    pub text_processor_stats: crate::utils::text_encoding::TextProcessorStats,
}

/// 便捷函数：处理网页内容
pub fn process_web_content(content: &[u8], content_type: Option<&str>) -> Result<ProcessedWebContent, WebContentError> {
    WebContentProcessor::global().process_web_content(content, content_type)
}

/// 便捷函数：批量处理网页内容
pub fn process_web_content_batch(contents: Vec<(&[u8], Option<&str>)>) -> Vec<Result<ProcessedWebContent, WebContentError>> {
    WebContentProcessor::global().process_batch(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_detection() {
        let processor = WebContentProcessor::new();
        
        let html_content = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Test</title></head>
        <body><p>Hello World</p></body>
        </html>
        "#;
        
        assert!(processor.detect_html_structure(html_content));
        
        let plain_text = "This is plain text without any HTML tags.";
        assert!(!processor.detect_html_structure(plain_text));
    }

    #[test]
    fn test_encoding_extraction() {
        let processor = WebContentProcessor::new();
        
        let html_with_meta = r#"
        <html>
        <head>
            <meta charset="utf-8">
            <title>Test</title>
        </head>
        <body>Content</body>
        </html>
        "#;
        
        let encoding = processor.extract_declared_encoding(html_with_meta);
        assert_eq!(encoding, Some("utf-8".to_string()));
    }

    #[test]
    fn test_html_text_extraction() {
        let processor = WebContentProcessor::new();
        
        let html_content = r#"
        <html>
        <head><title>Test Page</title></head>
        <body>
            <h1>Main Title</h1>
            <p>This is a <strong>test</strong> paragraph.</p>
            <script>alert('test');</script>
            <style>body { color: red; }</style>
        </body>
        </html>
        "#;
        
        let result = processor.process_web_content(html_content.as_bytes(), Some("text/html")).unwrap();
        
        assert!(result.is_html);
        assert!(result.extracted_text.contains("Main Title"));
        assert!(result.extracted_text.contains("test paragraph"));
        assert!(!result.extracted_text.contains("script"));
        assert!(!result.extracted_text.contains("style"));
    }

    #[test]
    fn test_plain_text_processing() {
        let processor = WebContentProcessor::new();
        
        let plain_text = "This is plain text content.\nWith multiple lines.";
        let result = processor.process_web_content(plain_text.as_bytes(), Some("text/plain")).unwrap();
        
        assert!(!result.is_html);
        assert_eq!(result.extracted_text, plain_text);
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
    fn test_batch_processing() {
        let processor = WebContentProcessor::new();
        
        let contents = vec![
            ("Text 1".as_bytes(), Some("text/plain")),
            ("Text 2".as_bytes(), Some("text/plain")),
        ];
        
        let results = processor.process_batch(contents);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_newline_trimming_in_html() {
        let processor = WebContentProcessor::new();
        
        // 测试HTML内容中的首位换行符去除
        let html_with_newlines = r#"

<html>
<head><title>Test</title></head>
<body>
<p>Hello World</p>
</body>
</html>

"#;
        
        let result = processor.process_web_content(html_with_newlines.as_bytes(), Some("text/html")).unwrap();
        
        // 验证提取的文本没有首位换行符
        assert!(!result.extracted_text.starts_with('\n'));
        assert!(!result.extracted_text.ends_with('\n'));
        assert!(result.extracted_text.contains("Hello World"));
    }

    #[test]
    fn test_newline_trimming_in_plain_text() {
        let processor = WebContentProcessor::new();
        
        // 测试纯文本中的首位换行符去除
        let text_with_newlines = "\n\nThis is plain text.\nWith multiple lines.\n\n";
        let result = processor.process_web_content(text_with_newlines.as_bytes(), Some("text/plain")).unwrap();
        
        // 验证处理后的文本没有首位换行符
        assert!(!result.extracted_text.starts_with('\n'));
        assert!(!result.extracted_text.ends_with('\n'));
        assert!(result.extracted_text.contains("plain text"));
        assert!(result.extracted_text.contains("multiple lines"));
        
        // 验证中间换行符被保留
        assert!(result.extracted_text.contains("text.\nWith")); // 中间换行符被保留
    }
}