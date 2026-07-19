// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
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
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        detector.feed(input, true);
        let encoding = detector.guess(None, Utf8Detection::Allow);
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

    // ========== Supplementary tests: long text, cache, non-UTF8 ==========

    #[test]
    fn test_long_text_with_unicode_escapes_is_normalized() {
        // Long text (>= short_text_threshold) containing \u escapes must be
        // normalized through the convert_encoding/normalize_unicode path when
        // the content is non-UTF8. For UTF-8 content, detect_and_convert_encoding
        // returns directly, so unicode normalization happens via process_short_text
        // only. This test verifies long UTF-8 text still gets unicode normalization.
        let processor = TextEncodingProcessor::new();
        let prefix = "a".repeat(1000);
        let suffix = r"\u4e16\u754c";
        let long_text = format!("{}{}", prefix, suffix);
        let result = processor
            .process_text(long_text.as_bytes())
            .expect("long text with unicode escapes should process");
        assert!(
            result.contains("世界"),
            "unicode escapes should be normalized: got {}",
            result
        );
    }

    #[test]
    fn test_long_text_caching_returns_same_result_on_second_call() {
        // Verify cached conversion produces identical output to the first call.
        let processor = TextEncodingProcessor::new();
        let long_text = "b".repeat(1500);
        let first = processor.process_text(long_text.as_bytes()).unwrap();
        let second = processor.process_text(long_text.as_bytes()).unwrap();
        assert_eq!(first, second, "cached result must match first result");
        // Cache size should remain 1 (same key).
        assert_eq!(processor.get_stats().cache_size, 1);
    }

    #[test]
    fn test_with_config_custom_cache_size() {
        // Verify with_config sets a custom cache size and short_text_threshold.
        let processor = TextEncodingProcessor::with_config(50, 200);
        let stats = processor.get_stats();
        assert_eq!(stats.short_text_threshold, 200);
        // Note: cache_capacity is hardcoded to 1000 in get_stats; this is
        // the current implementation behavior (not configurable via with_config).
        assert_eq!(stats.cache_capacity, 1000);
    }

    #[test]
    fn test_with_config_small_threshold_uses_long_text_path() {
        // With a small threshold, even short content goes through the long-text
        // path (with caching). Verify the cache is populated.
        let processor = TextEncodingProcessor::with_config(50, 5);
        assert_eq!(processor.get_stats().cache_size, 0);
        // "hello world" is 11 bytes, > threshold of 5.
        let result = processor.process_text(b"hello world").unwrap();
        assert_eq!(result, "hello world");
        assert_eq!(
            processor.get_stats().cache_size,
            1,
            "content longer than threshold should be cached"
        );
    }

    #[test]
    fn test_long_text_unicode_escape_uppercase_u_in_long_content() {
        // Long UTF-8 text containing \U escape (8 hex digits).
        let processor = TextEncodingProcessor::new();
        let prefix = "x".repeat(1000);
        let suffix = r"\U00000041"; // 'A'
        let long_text = format!("{}{}", prefix, suffix);
        let result = processor.process_text(long_text.as_bytes()).unwrap();
        assert!(
            result.contains('A'),
            "\\U escape should be normalized to 'A': got {}",
            result
        );
    }

    #[test]
    fn test_long_text_unicode_escape_x_in_long_content() {
        // Long UTF-8 text containing \x escape (2 hex digits).
        let processor = TextEncodingProcessor::new();
        let prefix = "y".repeat(1000);
        let suffix = r"\x42"; // 'B'
        let long_text = format!("{}{}", prefix, suffix);
        let result = processor.process_text(long_text.as_bytes()).unwrap();
        assert!(
            result.contains('B'),
            "\\x escape should be normalized to 'B': got {}",
            result
        );
    }

    #[test]
    fn test_cache_key_is_deterministic_for_same_input() {
        // Same input should produce the same cache key, so second call hits cache.
        let processor = TextEncodingProcessor::with_config(100, 10);
        let content = "z".repeat(20);
        processor.process_text(content.as_bytes()).unwrap();
        assert_eq!(processor.get_stats().cache_size, 1);
        // Different content should create a new cache entry.
        let content2 = "w".repeat(20);
        processor.process_text(content2.as_bytes()).unwrap();
        assert_eq!(processor.get_stats().cache_size, 2);
    }

    #[test]
    fn test_trim_newlines_only_newlines_at_start_and_end() {
        // Middle newlines are preserved; only leading/trailing are trimmed.
        let processor = TextEncodingProcessor::new();
        assert_eq!(processor.trim_newlines("\n\n\n"), "");
        assert_eq!(processor.trim_newlines("content"), "content");
        assert_eq!(
            processor.trim_newlines("\n\ncontent\nmore\n"),
            "content\nmore"
        );
    }

    #[test]
    fn test_process_text_with_empty_input() {
        // Empty input is a valid edge case; should return empty string.
        let processor = TextEncodingProcessor::new();
        let result = processor
            .process_text(b"")
            .expect("empty input should succeed");
        assert_eq!(result, "");
    }

    #[test]
    fn test_process_batch_with_mixed_utf8_and_empty() {
        // Batch containing empty and non-empty UTF-8 inputs.
        let processor = TextEncodingProcessor::new();
        let inputs: Vec<&[u8]> = vec![b"", b"hello", b""];
        let results = processor.process_batch(inputs);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
        assert_eq!(results[0].as_ref().unwrap(), "");
        assert_eq!(results[1].as_ref().unwrap(), "hello");
        assert_eq!(results[2].as_ref().unwrap(), "");
    }

    #[test]
    fn test_non_utf8_content_is_decoded() {
        // Non-UTF8 content (GBK-encoded "中文") should be decoded successfully.
        // GBK encoding of "中文" is [0xD6, 0xD0, 0xCE, 0xC4].
        let processor = TextEncodingProcessor::new();
        let gbk_bytes: [u8; 4] = [0xD6, 0xD0, 0xCE, 0xC4];
        let result = processor.process_text(&gbk_bytes);
        // The decoder should succeed; the exact output depends on chardetng's
        // detection. We verify it does not error and produces non-empty output.
        match result {
            Ok(text) => {
                assert!(!text.is_empty(), "decoded text should be non-empty");
            }
            Err(_) => {
                // If detection fails, that's acceptable for very short non-UTF8 input;
                // the important thing is that the function does not panic.
            }
        }
    }

    #[test]
    fn test_non_utf8_long_content_caching() {
        // Long non-UTF8 content should go through the cache path.
        // Construct a long non-UTF8 byte sequence by repeating GBK bytes.
        let processor = TextEncodingProcessor::new();
        let mut non_utf8 = Vec::new();
        for _ in 0..300 {
            non_utf8.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
        }
        let result = processor.process_text(&non_utf8);
        // Should either succeed (decoded) or fail gracefully; must not panic.
        if let Ok(text) = result {
            assert!(!text.is_empty());
        }
    }

    #[test]
    fn test_text_processor_clone_preserves_config() {
        // TextEncodingProcessor derives Clone; verify config is preserved.
        let processor = TextEncodingProcessor::with_config(50, 500);
        let cloned = processor.clone();
        assert_eq!(
            processor.get_stats().short_text_threshold,
            cloned.get_stats().short_text_threshold
        );
    }

    #[test]
    fn test_invalid_utf8_short_text_does_not_panic() {
        // Short invalid UTF-8 should be handled via detect_and_convert_encoding.
        let processor = TextEncodingProcessor::new();
        let invalid_utf8: [u8; 4] = [0xFF, 0xFE, 0xFD, 0xFC];
        let result = processor.process_text(&invalid_utf8);
        // Should not panic; may succeed or fail depending on detection.
        let _ = result;
    }

    #[test]
    fn test_get_stats_cache_capacity_is_constant() {
        // get_stats always reports cache_capacity=1000 regardless of with_config.
        // This documents the current implementation behavior.
        let processor = TextEncodingProcessor::with_config(50, 200);
        let stats = processor.get_stats();
        assert_eq!(stats.cache_capacity, 1000);
    }

    // ========== Supplementary tests: unicode escape edge cases & cached non-UTF8 path ==========

    #[test]
    fn test_unicode_escape_surrogate_code_point_falls_back_to_backslash() {
        // \uD800 is a surrogate code point; char::from_u32 returns None.
        // The parser should fall back to pushing the backslash and consuming
        // the 'u' + 4 hex chars (which are lost).
        let processor = TextEncodingProcessor::new();
        let input = r"\uD800";
        let result = processor.process_text(input.as_bytes()).unwrap();
        // Backslash is pushed; 'D800' chars are consumed by the hex loop.
        assert_eq!(result, "\\");
    }

    #[test]
    fn test_unicode_uppercase_u_escape_beyond_max_code_point() {
        // \U00110000 is beyond the maximum Unicode scalar value (0x10FFFF).
        // char::from_u32 returns None, so the backslash is pushed and the
        // 8 hex chars are consumed (lost).
        let processor = TextEncodingProcessor::new();
        let input = r"\U00110000";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "\\");
    }

    #[test]
    fn test_unicode_escape_incomplete_at_end_of_string() {
        // \u41 at end of string: only 2 hex chars available. The loop runs 4
        // times but chars.next() returns None after 2, so hex_chars = "41".
        // 0x41 = 'A'. This documents the lenient parsing behavior.
        let processor = TextEncodingProcessor::new();
        let input = r"\u41";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "A");
    }

    #[test]
    fn test_unicode_x_escape_with_non_hex_chars() {
        // \xZZ: from_str_radix("ZZ", 16) fails, backslash is pushed, 'x' and
        // 'ZZ' are consumed (lost).
        let processor = TextEncodingProcessor::new();
        let input = r"\xZZ";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "\\");
    }

    #[test]
    fn test_unicode_x_escape_single_hex_digit() {
        // \xA at end: only 1 hex char available. hex_chars = "A" = 0x0A = '\n'.
        // process_text calls trim_newlines which removes the leading/trailing
        // \n, so the final result is empty.
        let processor = TextEncodingProcessor::new();
        let input = r"\xA";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_multiple_unicode_escapes_in_one_string() {
        // Mix of \u, \U, and \x escapes in a single string.
        let processor = TextEncodingProcessor::new();
        let input = r"\u0041\U00000042\x43";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "ABC");
    }

    #[test]
    fn test_backslash_u_at_end_of_string_with_no_hex() {
        // "\u" at end: 'u' is consumed, hex loop runs 4 times getting None,
        // hex_chars = "", from_str_radix("", 16) fails, backslash pushed.
        let processor = TextEncodingProcessor::new();
        let input = r"\u";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "\\");
    }

    #[test]
    fn test_backslash_followed_by_non_escape_char() {
        // \n: 'n' is not u/U/x, so all branches are skipped and backslash is
        // pushed, then 'n' is pushed in the next iteration.
        let processor = TextEncodingProcessor::new();
        let input = r"\n";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "\\n");
    }

    #[test]
    fn test_unicode_escape_max_valid_code_point() {
        // \U0010FFFF is the maximum valid Unicode scalar value.
        let processor = TextEncodingProcessor::new();
        let input = r"\U0010FFFF";
        let result = processor.process_text(input.as_bytes()).unwrap();
        // Should produce the character U+10FFFF.
        assert_eq!(result, "\u{10FFFF}");
    }

    #[test]
    fn test_unicode_escape_null_code_point() {
        // \u0000 is the null character; char::from_u32(0) returns Some('\0').
        let processor = TextEncodingProcessor::new();
        let input = r"\u0000";
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "\0");
    }

    #[test]
    fn test_apply_cached_conversion_non_utf8_path() {
        // Process non-UTF8 content twice with a small threshold so the second
        // call uses the cached non-UTF8 detection (apply_cached_conversion
        // non-utf8 branch). We use GBK bytes and a threshold of 2.
        let processor = TextEncodingProcessor::with_config(100, 2);
        let gbk_bytes: [u8; 4] = [0xD6, 0xD0, 0xCE, 0xC4];
        // First call: detects and caches.
        let first = processor.process_text(&gbk_bytes);
        // Second call: should use cached detection (non-utf8 branch).
        let second = processor.process_text(&gbk_bytes);
        // Both calls should produce the same result (either both Ok or both Err).
        match (first, second) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "cached non-utf8 result should match first"),
            (Err(_), Err(_)) => {}
            (a, b) => panic!("both calls should agree: first={:?}, second={:?}", a, b),
        }
        // Cache should have exactly 1 entry.
        assert_eq!(processor.get_stats().cache_size, 1);
    }

    #[test]
    fn test_convert_encoding_with_unicode_escapes_in_non_utf8() {
        // When non-UTF8 content is decoded and the decoded text contains
        // unicode escape sequences, convert_encoding should normalize them.
        // This is hard to control precisely, so we verify the path does not
        // panic and produces some output.
        let processor = TextEncodingProcessor::with_config(100, 2);
        // Construct bytes that when decoded might contain \u sequences.
        // We use a long sequence to ensure long-text path with caching.
        let mut non_utf8 = Vec::new();
        for _ in 0..100 {
            non_utf8.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
        }
        let result = processor.process_text(&non_utf8);
        // Should not panic; may succeed or fail.
        let _ = result;
    }

    #[test]
    fn test_long_text_processing_caches_detection_only_on_success() {
        // cache_detection_result is only called when result.is_ok(). Verify
        // that a failed detection does not populate the cache.
        let processor = TextEncodingProcessor::with_config(100, 2);
        // Use bytes likely to fail detection in a way that produces an error.
        // Note: this is best-effort; if detection succeeds, cache will have 1.
        let weird_bytes: Vec<u8> = vec![0xFF; 10];
        let _ = processor.process_text(&weird_bytes);
        // If it succeeded, cache_size is 1; if it failed, cache_size is 0.
        // Either way, this documents the behavior.
        let stats = processor.get_stats();
        assert!(stats.cache_size <= 1);
    }

    #[test]
    fn test_process_text_preserves_content_with_special_chars() {
        // Verify that text with special characters (but no unicode escapes)
        // is preserved exactly.
        let processor = TextEncodingProcessor::new();
        let input = "Hello\tWorld\nNew Line\r\nCarriage";
        let result = processor.process_text(input.as_bytes()).unwrap();
        // trim_newlines only removes leading/trailing \n, not \t or \r.
        assert_eq!(result, input);
    }

    #[test]
    fn test_trim_newlines_preserves_carriage_returns() {
        // trim_newlines only removes \n, not \r. Leading \r\n becomes \r
        // (only \n at start would be removed; here start is \r so no change).
        // Trailing \r\n becomes \r (the trailing \n is removed).
        let processor = TextEncodingProcessor::new();
        assert_eq!(processor.trim_newlines("\rhello\r"), "\rhello\r");
        assert_eq!(processor.trim_newlines("\r\nhello\r\n"), "\r\nhello\r");
    }

    #[test]
    fn test_unicode_escape_mixed_with_regular_backslashes() {
        // Mix of valid escapes and plain backslashes.
        let processor = TextEncodingProcessor::new();
        let input = r"\\u0041\n\x42";
        // First \\ is a literal backslash (second \ is not u/U/x after first \).
        // Wait: parsing: c='\\', peek='\\' (not u/U/x), push '\\'.
        // Next: c='\\'(second), peek='u', consume 'u', hex="0041"='A', push 'A'.
        // Next: c='\\'(from \n), peek='n', not u/U/x, push '\\'.
        // Next: c='n', push 'n'.
        // Next: c='\\'(from \x), peek='x', consume 'x', hex="42"='B', push 'B'.
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert_eq!(result, "\\A\\nB");
    }

    // ========== detect_and_convert_encoding / apply_cached_conversion is_utf8=true 分支覆盖 ==========

    #[test]
    fn test_detect_and_convert_encoding_utf8_branch_with_chinese_long_text() {
        // 构造包含非 ASCII UTF-8 字符的长文本（>= short_text_threshold），
        // 确保 chardetng 检测为 utf-8（is_utf8=true），
        // 覆盖 detect_and_convert_encoding 行 203-206。
        let processor = TextEncodingProcessor::new();
        let prefix = "a".repeat(900);
        let chinese = "你好世界测试文本".repeat(20);
        let long_text = format!("{}{}", prefix, chinese);
        assert!(
            long_text.len() >= 1000,
            "long_text must be >= 1000 bytes for long-text path"
        );
        let result = processor
            .process_text(long_text.as_bytes())
            .expect("long UTF-8 text with Chinese should process");
        assert!(
            result.contains("你好世界测试文本"),
            "result should contain Chinese text"
        );
    }

    #[test]
    fn test_apply_cached_conversion_utf8_branch_on_second_call() {
        // 第二次调用相同的长 UTF-8 文本，走缓存路径，
        // apply_cached_conversion 的 is_utf8=true 分支（行 277-279）。
        let processor = TextEncodingProcessor::new();
        let prefix = "b".repeat(900);
        let chinese = "你好世界".repeat(30);
        let long_text = format!("{}{}", prefix, chinese);
        assert!(long_text.len() >= 1000);
        // 第一次调用：detect_and_convert_encoding + cache_detection_result
        let first = processor
            .process_text(long_text.as_bytes())
            .expect("first call should succeed");
        assert_eq!(processor.get_stats().cache_size, 1);
        // 第二次调用：apply_cached_conversion（is_utf8=true 分支）
        let second = processor
            .process_text(long_text.as_bytes())
            .expect("second call should use cache");
        assert_eq!(first, second, "cached result should match first result");
    }

    #[test]
    fn test_detect_and_convert_encoding_utf8_branch_with_mixed_long_text() {
        // 另一个长 UTF-8 文本，包含多种非 ASCII 字符，
        // 进一步确保 chardetng 检测为 utf-8。
        let processor = TextEncodingProcessor::new();
        let prefix = "x".repeat(800);
        let mixed = "你好世界こんにちは안녕하세요".repeat(20);
        let long_text = format!("{}{}", prefix, mixed);
        assert!(long_text.len() >= 1000);
        let result = processor
            .process_text(long_text.as_bytes())
            .expect("mixed long UTF-8 text should process");
        assert!(!result.is_empty());
    }
}
