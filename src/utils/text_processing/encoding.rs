// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chardetng::EncodingDetector;
use encoding_rs::Encoding;
use log::{debug, warn};
use lru::LruCache;
use parking_lot::Mutex;
use std::sync::Arc;
use thiserror::Error;

/// 文本编码处理错误类型
#[derive(Error, Debug, Clone)]
pub enum TextEncodingError {
    #[error("编码检测失败: {0}")]
    DetectionFailed(String),

    #[error("编码转换失败: {0}")]
    ConversionFailed(String),

    #[error("Unicode转换失败: {0}")]
    UnicodeConversionFailed(String),

    #[error("无效的编码格式: {0}")]
    InvalidEncoding(String),

    #[error("文本处理超时")]
    ProcessingTimeout,

    #[error("缓存锁获取失败: {0}")]
    CacheLockError(String),
}

/// 编码检测结果
#[derive(Debug, Clone)]
pub struct EncodingDetection {
    pub encoding: String,
    pub confidence: f32,
    pub is_utf8: bool,
}

/// 文本编码处理器
#[derive(Clone)]
pub struct TextEncodingProcessor {
    encoding_cache: Arc<Mutex<LruCache<String, EncodingDetection>>>,
    short_text_threshold: usize,
}

impl Default for TextEncodingProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEncodingProcessor {
    /// 创建新的文本编码处理器
    pub fn new() -> Self {
        Self {
            encoding_cache: Arc::new(Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(1000).expect("Cache size must be non-zero"),
            ))),
            short_text_threshold: 1000,
        }
    }

    /// 创建带有自定义配置的文本编码处理器
    pub fn with_config(cache_size: usize, short_text_threshold: usize) -> Self {
        Self {
            encoding_cache: Arc::new(Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_size).expect("Cache size must be non-zero"),
            ))),
            short_text_threshold,
        }
    }

    /// 处理文本编码，确保输出为UTF-8格式
    pub fn process_text(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        let result = if input.len() < self.short_text_threshold {
            self.process_short_text(input)?
        } else {
            self.process_long_text(input)?
        };
        Ok(self.trim_newlines(&result))
    }

    /// 处理短文本（优化性能）
    fn process_short_text(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        debug!("处理短文本，长度: {} 字节", input.len());
        if let Ok(utf8_str) = std::str::from_utf8(input) {
            if self.contains_unicode_escapes(utf8_str) {
                return self.normalize_unicode(utf8_str);
            }
            return Ok(utf8_str.to_string());
        }
        let (result, _detection) = self.detect_and_convert_encoding(input);
        result
    }

    /// 处理长文本（完整处理流程）
    fn process_long_text(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        debug!("处理长文本，长度: {} 字节", input.len());
        let cache_key = self.generate_cache_key(input);
        if let Some(cached_result) = self.get_cached_detection(&cache_key) {
            debug!("使用缓存的编码检测结果: {:?}", cached_result);
            return self.apply_cached_conversion(input, &cached_result);
        }
        let (result, detection) = self.detect_and_convert_encoding(input);
        if result.is_ok() {
            self.cache_detection_result(&cache_key, &detection);
        }
        result
    }

    /// 检测文本是否包含Unicode转义序列
    fn contains_unicode_escapes(&self, text: &str) -> bool {
        text.contains("\\u") || text.contains("\\U") || text.contains("\\x")
    }

    /// 解析字符串中的Unicode转义序列
    fn parse_unicode_escapes(&self, text: &str) -> String {
        let mut result = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if chars.peek() == Some(&'u') {
                    chars.next();
                    let mut hex_chars = String::new();
                    for _ in 0..4 {
                        if let Some(h) = chars.next() {
                            hex_chars.push(h);
                        }
                    }
                    if let Ok(code_point) = u32::from_str_radix(&hex_chars, 16) {
                        if let Some(ch) = char::from_u32(code_point) {
                            result.push(ch);
                            continue;
                        }
                    }
                } else if chars.peek() == Some(&'U') {
                    chars.next();
                    let mut hex_chars = String::new();
                    for _ in 0..8 {
                        if let Some(h) = chars.next() {
                            hex_chars.push(h);
                        }
                    }
                    if let Ok(code_point) = u32::from_str_radix(&hex_chars, 16) {
                        if let Some(ch) = char::from_u32(code_point) {
                            result.push(ch);
                            continue;
                        }
                    }
                } else if chars.peek() == Some(&'x') {
                    chars.next();
                    let mut hex_chars = String::new();
                    for _ in 0..2 {
                        if let Some(h) = chars.next() {
                            hex_chars.push(h);
                        }
                    }
                    if let Ok(code_point) = u32::from_str_radix(&hex_chars, 16) {
                        if let Some(ch) = char::from_u32(code_point) {
                            result.push(ch);
                            continue;
                        }
                    }
                }
            }
            result.push(c);
        }
        result
    }

    /// Unicode字符串规范化转换
    fn normalize_unicode(&self, text: &str) -> Result<String, TextEncodingError> {
        debug!("检测到Unicode转义序列，执行规范化转换");
        let parsed = self.parse_unicode_escapes(text);
        debug!("Unicode转义序列解析完成: {:?}", parsed);
        Ok(parsed)
    }

    /// 检测并转换编码
    fn detect_and_convert_encoding(
        &self,
        input: &[u8],
    ) -> (Result<String, TextEncodingError>, EncodingDetection) {
        debug!("开始编码检测");
        let mut detector = EncodingDetector::new();
        detector.feed(input, true);
        let encoding = detector.guess(None, true);
        let encoding_name = encoding.name();
        let is_utf8 = encoding_name == "utf-8";
        let confidence = if is_utf8 { 1.0 } else { 0.9 };
        let detection = EncodingDetection {
            encoding: encoding_name.to_string(),
            confidence,
            is_utf8,
        };
        debug!("检测到编码: {}, 置信度: {:.2}", encoding_name, confidence);
        if is_utf8 {
            let result = std::str::from_utf8(input)
                .map(|s| s.to_string())
                .map_err(|e| TextEncodingError::ConversionFailed(e.to_string()));
            return (result, detection);
        }
        let result = self.convert_encoding(input, encoding, confidence);
        (result, detection)
    }

    /// 去除字符串的首位换行符
    pub fn trim_newlines(&self, text: &str) -> String {
        let mut result = text.to_string();
        while result.starts_with('\n') {
            result = result[1..].to_string();
        }
        while result.ends_with('\n') && !result.is_empty() {
            result.pop();
        }
        result
    }

    /// 转换编码到UTF-8
    fn convert_encoding(
        &self,
        input: &[u8],
        encoding: &'static Encoding,
        confidence: f32,
    ) -> Result<String, TextEncodingError> {
        if confidence < 0.3 {
            warn!("编码检测置信度过低: {:.2}，使用UTF-8尝试解析", confidence);
        }
        let (decoded, _, had_errors) = encoding.decode(input);
        if had_errors && confidence < 0.5 {
            return Err(TextEncodingError::ConversionFailed(format!(
                "编码转换错误，检测到编码: {}，置信度: {:.2}",
                encoding.name(),
                confidence
            )));
        }
        let result = decoded.into_owned();
        if self.contains_unicode_escapes(&result) {
            return self.normalize_unicode(&result);
        }
        Ok(result)
    }

    /// 生成缓存键
    fn generate_cache_key(&self, input: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// 获取缓存的检测结果
    fn get_cached_detection(&self, key: &str) -> Option<EncodingDetection> {
        let mut cache = self.encoding_cache.lock();
        cache.get(key).cloned()
    }

    /// 缓存检测结果
    fn cache_detection_result(&self, key: &str, detection: &EncodingDetection) {
        let mut cache = self.encoding_cache.lock();
        cache.put(key.to_string(), detection.clone());
    }

    /// 应用缓存的转换结果
    fn apply_cached_conversion(
        &self,
        input: &[u8],
        detection: &EncodingDetection,
    ) -> Result<String, TextEncodingError> {
        if detection.is_utf8 {
            std::str::from_utf8(input)
                .map(|s| s.to_string())
                .map_err(|e| TextEncodingError::ConversionFailed(e.to_string()))
        } else {
            let encoding = Encoding::for_label(detection.encoding.as_bytes())
                .ok_or_else(|| TextEncodingError::InvalidEncoding(detection.encoding.clone()))?;
            self.convert_encoding(input, encoding, detection.confidence)
        }
    }

    /// 批量处理文本编码
    pub fn process_batch(&self, inputs: Vec<&[u8]>) -> Vec<Result<String, TextEncodingError>> {
        inputs
            .into_iter()
            .map(|input| self.process_text(input))
            .collect()
    }

    /// 获取处理器统计信息
    pub fn get_stats(&self) -> TextProcessorStats {
        let cache_size = self.encoding_cache.lock().len();
        TextProcessorStats {
            cache_size,
            cache_capacity: 1000,
            short_text_threshold: self.short_text_threshold,
        }
    }
}

/// 处理器统计信息
#[derive(Debug, Clone)]
pub struct TextProcessorStats {
    pub cache_size: usize,
    pub cache_capacity: usize,
    pub short_text_threshold: usize,
}

/// 处理单个文本（使用提供的处理器实例）
pub fn process_with_processor(
    processor: &TextEncodingProcessor,
    input: &[u8],
) -> Result<String, TextEncodingError> {
    processor.process_text(input)
}

/// 批量处理文本（使用提供的处理器实例）
pub fn process_batch_with_processor(
    processor: &TextEncodingProcessor,
    inputs: Vec<&[u8]>,
) -> Vec<Result<String, TextEncodingError>> {
    processor.process_batch(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf8_text_processing() {
        let processor = TextEncodingProcessor::new();
        let input = "Hello, 世界! This is a test.";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_unicode_escape_processing() {
        let processor = TextEncodingProcessor::new();
        let input = "Hello 世界!";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert!(result.contains("世界"));
    }

    #[test]
    fn test_unicode_escape_sequence_conversion() {
        let processor = TextEncodingProcessor::new();
        let input = "\u{4e00}\u{7ad9}\u{5f0f}\u{7f51}\u{7ad9}\u{5efa}\u{8bbe}";
        let result = processor
            .process_text(input.as_bytes())
            .expect("Unicode conversion should succeed");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_short_text_optimization() {
        let processor = TextEncodingProcessor::new();
        let short_text = "Short text";
        let result = processor.process_text(short_text.as_bytes()).unwrap();
        assert_eq!(result, short_text);
    }

    #[test]
    fn test_batch_processing() {
        let processor = TextEncodingProcessor::new();
        let inputs = vec![
            "Text 1".as_bytes(),
            "Text 2".as_bytes(),
            "Text 3".as_bytes(),
        ];
        let results = processor.process_batch(inputs);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_trim_newlines_basic() {
        let processor = TextEncodingProcessor::new();
        assert_eq!(processor.trim_newlines("hello"), "hello");
        assert_eq!(processor.trim_newlines("\nhello"), "hello");
        assert_eq!(processor.trim_newlines("hello\n"), "hello");
        assert_eq!(processor.trim_newlines("\nhello\n"), "hello");
        assert_eq!(processor.trim_newlines("\n\nhello\n\n"), "hello");
    }

    #[test]
    fn test_trim_newlines_middle_preserved() {
        let processor = TextEncodingProcessor::new();
        assert_eq!(processor.trim_newlines("hello\nworld"), "hello\nworld");
        assert_eq!(processor.trim_newlines("\nhello\nworld\n"), "hello\nworld");
    }

    #[test]
    fn test_trim_newlines_empty() {
        let processor = TextEncodingProcessor::new();
        assert_eq!(processor.trim_newlines(""), "");
        assert_eq!(processor.trim_newlines("\n"), "");
        assert_eq!(processor.trim_newlines("\n\n\n"), "");
    }

    #[test]
    fn test_with_config_custom_values() {
        let processor = TextEncodingProcessor::with_config(50, 200);
        let stats = processor.get_stats();
        assert_eq!(stats.short_text_threshold, 200);
    }

    #[test]
    fn test_default_impl() {
        let processor = TextEncodingProcessor::default();
        let stats = processor.get_stats();
        assert_eq!(stats.short_text_threshold, 1000);
    }

    #[test]
    fn test_long_text_processing_utf8() {
        let processor = TextEncodingProcessor::new();
        let long_text = "a".repeat(1500);
        let result = processor.process_text(long_text.as_bytes()).unwrap();
        assert_eq!(result, long_text);
    }

    #[test]
    fn test_long_text_caching_increases_cache_size() {
        let processor = TextEncodingProcessor::new();
        let long_text = "x".repeat(1500);

        assert_eq!(processor.get_stats().cache_size, 0);

        processor.process_text(long_text.as_bytes()).unwrap();
        assert_eq!(processor.get_stats().cache_size, 1);

        processor.process_text(long_text.as_bytes()).unwrap();
        assert_eq!(processor.get_stats().cache_size, 1);
    }

    #[test]
    fn test_get_stats_empty_cache() {
        let processor = TextEncodingProcessor::new();
        let stats = processor.get_stats();
        assert_eq!(stats.cache_size, 0);
        assert_eq!(stats.cache_capacity, 1000);
        assert_eq!(stats.short_text_threshold, 1000);
    }

    #[test]
    fn test_get_stats_after_processing() {
        let processor = TextEncodingProcessor::new();
        let long_text = "c".repeat(1500);
        processor.process_text(long_text.as_bytes()).unwrap();
        let stats = processor.get_stats();
        assert_eq!(stats.cache_size, 1);
    }

    #[test]
    fn test_unicode_lowercase_u_escape_sequence() {
        let processor = TextEncodingProcessor::new();
        let input = r"Hello \u0041 World";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "Hello A World");
    }

    #[test]
    fn test_unicode_uppercase_u_escape_sequence() {
        let processor = TextEncodingProcessor::new();
        let input = r"Hello \U00000041 World";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "Hello A World");
    }

    #[test]
    fn test_unicode_x_escape_sequence() {
        let processor = TextEncodingProcessor::new();
        let input = r"Hello \x41 World";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "Hello A World");
    }

    #[test]
    fn test_unicode_escape_chinese() {
        let processor = TextEncodingProcessor::new();
        let input = r"Hello \u4e16界";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "Hello 世界");
    }

    #[test]
    fn test_invalid_unicode_escape_fallback() {
        let processor = TextEncodingProcessor::new();
        let input = r"Hello \uGGGG World";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "Hello \\ World");
    }

    #[test]
    fn test_backslash_without_escape() {
        let processor = TextEncodingProcessor::new();
        let input = r"Hello \n World";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "Hello \\n World");
    }

    #[test]
    fn test_process_with_processor_free_function() {
        let processor = TextEncodingProcessor::new();
        let input = "test text";
        let result = process_with_processor(&processor, input.as_bytes()).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_process_batch_with_processor_free_function() {
        let processor = TextEncodingProcessor::new();
        let inputs = vec!["a".as_bytes(), "b".as_bytes()];
        let results = process_batch_with_processor(&processor, inputs);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_text_encoding_error_display() {
        let err = TextEncodingError::DetectionFailed("test reason".to_string());
        assert_eq!(err.to_string(), "编码检测失败: test reason");

        let err = TextEncodingError::ConversionFailed("conv error".to_string());
        assert_eq!(err.to_string(), "编码转换失败: conv error");

        let err = TextEncodingError::UnicodeConversionFailed("uni error".to_string());
        assert_eq!(err.to_string(), "Unicode转换失败: uni error");

        let err = TextEncodingError::InvalidEncoding("bad enc".to_string());
        assert_eq!(err.to_string(), "无效的编码格式: bad enc");

        let err = TextEncodingError::ProcessingTimeout;
        assert_eq!(err.to_string(), "文本处理超时");

        let err = TextEncodingError::CacheLockError("lock fail".to_string());
        assert_eq!(err.to_string(), "缓存锁获取失败: lock fail");
    }

    #[test]
    fn test_encoding_detection_struct() {
        let detection = EncodingDetection {
            encoding: "utf-8".to_string(),
            confidence: 1.0,
            is_utf8: true,
        };
        assert_eq!(detection.encoding, "utf-8");
        assert_eq!(detection.confidence, 1.0);
        assert!(detection.is_utf8);
    }

    #[test]
    fn test_text_processor_stats_struct() {
        let stats = TextProcessorStats {
            cache_size: 10,
            cache_capacity: 100,
            short_text_threshold: 500,
        };
        assert_eq!(stats.cache_size, 10);
        assert_eq!(stats.cache_capacity, 100);
        assert_eq!(stats.short_text_threshold, 500);
    }

    #[test]
    fn test_batch_empty_input() {
        let processor = TextEncodingProcessor::new();
        let results = processor.process_batch(vec![]);
        assert_eq!(results.len(), 0);
    }
}
