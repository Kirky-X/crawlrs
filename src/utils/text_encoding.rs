use chardetng::EncodingDetector;
use encoding_rs::Encoding;
use deunicode::deunicode;
use lru::LruCache;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use thiserror::Error;
use tracing::{debug, warn, error};

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
}

/// 编码检测结果
#[derive(Debug, Clone)]
pub struct EncodingDetection {
    pub encoding: String,
    pub confidence: f32,
    pub is_utf8: bool,
}

/// 文本编码处理器
pub struct TextEncodingProcessor {
    encoding_cache: Mutex<LruCache<String, EncodingDetection>>,
    short_text_threshold: usize,
}

/// 全局文本编码处理器实例
static TEXT_PROCESSOR: Lazy<TextEncodingProcessor> = Lazy::new(TextEncodingProcessor::new);

impl TextEncodingProcessor {
    /// 创建新的文本编码处理器
    pub fn new() -> Self {
        Self {
            encoding_cache: Mutex::new(LruCache::new(std::num::NonZeroUsize::new(1000).unwrap())),
            short_text_threshold: 1000, // 1KB阈值，用于区分长短文本
        }
    }

    /// 获取全局处理器实例
    pub fn global() -> &'static Self {
        &TEXT_PROCESSOR
    }

    /// 处理文本编码，确保输出为UTF-8格式
    pub fn process_text(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        // 检查输入数据大小，对小文本进行特殊处理
        if input.len() < self.short_text_threshold {
            self.process_short_text(input)
        } else {
            self.process_long_text(input)
        }
    }

    /// 处理短文本（优化性能）
    fn process_short_text(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        debug!("处理短文本，长度: {} 字节", input.len());
        
        // 首先尝试直接作为UTF-8解析
        if let Ok(utf8_str) = std::str::from_utf8(input) {
            // 检测是否为Unicode字符串需要转换
            if self.contains_unicode_escapes(utf8_str) {
                return self.normalize_unicode(utf8_str);
            }
            return Ok(utf8_str.to_string());
        }

        // UTF-8解析失败，进行编码检测
        self.detect_and_convert_encoding(input)
    }

    /// 处理长文本（完整处理流程）
    fn process_long_text(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        debug!("处理长文本，长度: {} 字节", input.len());
        
        // 检查缓存
        let cache_key = self.generate_cache_key(input);
        if let Some(cached_result) = self.get_cached_detection(&cache_key) {
            debug!("使用缓存的编码检测结果: {:?}", cached_result);
            return self.apply_cached_conversion(input, &cached_result);
        }

        // 完整的编码检测和转换流程
        let result = self.detect_and_convert_encoding(input)?;
        
        // 缓存结果
        self.cache_detection_result(&cache_key, &result);
        
        Ok(result)
    }

    /// 检测文本是否包含Unicode转义序列
    fn contains_unicode_escapes(&self, text: &str) -> bool {
        text.contains("\\u") || text.contains("\\U") || text.contains("\\x")
    }

    /// Unicode字符串规范化转换
    fn normalize_unicode(&self, text: &str) -> Result<String, TextEncodingError> {
        debug!("检测到Unicode转义序列，执行规范化转换");
        
        let normalized = deunicode(text);
        debug!("Unicode转换成功");
        Ok(normalized)
    }

    /// 检测并转换编码
    fn detect_and_convert_encoding(&self, input: &[u8]) -> Result<String, TextEncodingError> {
        debug!("开始编码检测");
        
        // 使用chardetng进行编码检测
        let mut detector = EncodingDetector::new();
        detector.feed(input, true);
        
        let encoding = detector.guess(None, true);
        let encoding_name = encoding.name();
        
        debug!("检测到编码: {}", encoding_name);
        
        // 如果已经是UTF-8且置信度高，直接返回
        if encoding_name == "utf-8" {
            return std::str::from_utf8(input)
                .map(|s| s.to_string())
                .map_err(|e| TextEncodingError::ConversionFailed(e.to_string()));
        }

        // 进行编码转换
        self.convert_encoding(input, encoding, 0.9)
    }

    /// 转换编码到UTF-8
    fn convert_encoding(&self, input: &[u8], encoding: &'static Encoding, confidence: f32) -> Result<String, TextEncodingError> {
        if confidence < 0.3 {
            warn!("编码检测置信度过低: {:.2}，使用UTF-8尝试解析", confidence);
        }

        let (decoded, _, had_errors) = encoding.decode(input);
        
        if had_errors && confidence < 0.5 {
            return Err(TextEncodingError::ConversionFailed(
                format!("编码转换错误，检测到编码: {}，置信度: {:.2}", encoding.name(), confidence)
            ));
        }

        let result = decoded.into_owned();
        
        // 检查转换后的文本是否包含Unicode转义序列
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
        self.encoding_cache.lock().unwrap().get(key).cloned()
    }

    /// 缓存检测结果
    fn cache_detection_result(&self, key: &str, _result: &str) {
        let detection = EncodingDetection {
            encoding: "utf-8".to_string(),
            confidence: 1.0,
            is_utf8: true,
        };
        
        self.encoding_cache.lock().unwrap().put(key.to_string(), detection);
    }

    /// 应用缓存的转换结果
    fn apply_cached_conversion(&self, input: &[u8], detection: &EncodingDetection) -> Result<String, TextEncodingError> {
        if detection.is_utf8 {
            std::str::from_utf8(input)
                .map(|s| s.to_string())
                .map_err(|e| TextEncodingError::ConversionFailed(e.to_string()))
        } else {
            // 如果不是UTF-8，需要重新检测转换
            self.detect_and_convert_encoding(input)
        }
    }

    /// 批量处理文本编码
    pub fn process_batch(&self, inputs: Vec<&[u8]>) -> Vec<Result<String, TextEncodingError>> {
        inputs.into_iter()
            .map(|input| self.process_text(input))
            .collect()
    }

    /// 获取处理器统计信息
    pub fn get_stats(&self) -> TextProcessorStats {
        let cache_size = self.encoding_cache.lock().unwrap().len();
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

/// 便捷函数：处理单个文本
pub fn process_text_encoding(input: &[u8]) -> Result<String, TextEncodingError> {
    TextEncodingProcessor::global().process_text(input)
}

/// 便捷函数：批量处理文本
pub fn process_text_batch(inputs: Vec<&[u8]>) -> Vec<Result<String, TextEncodingError>> {
    TextEncodingProcessor::global().process_batch(inputs)
}

/// 便捷函数：处理字符串（UTF-8输入）
pub fn process_string(input: &str) -> Result<String, TextEncodingError> {
    // 检查是否包含Unicode转义序列
    if TextEncodingProcessor::global().contains_unicode_escapes(input) {
        TextEncodingProcessor::global().normalize_unicode(input)
    } else {
        Ok(input.to_string())
    }
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
        let input = "Hello \\u4e16\\u754c!"; // "世界" in Unicode escapes
        let result = processor.process_text(input.as_bytes()).unwrap();
        assert!(result.contains("世界") || result.contains("Shi Jie"));
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
    fn test_stats() {
        let processor = TextEncodingProcessor::new();
        let stats = processor.get_stats();
        assert_eq!(stats.cache_capacity, 1000);
        assert_eq!(stats.short_text_threshold, 1000);
    }
}