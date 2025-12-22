// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::utils::text_encoding::{process_text_encoding, TextEncodingError};
use crate::utils::web_content_processor::{
    process_web_content, ProcessedWebContent, WebContentError,
};
use std::time::{Duration, Instant};
use tracing::{error, info};

/// 爬虫文本处理集成器
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
    /// 创建新的爬虫文本处理器
    pub fn new() -> Self {
        Self {
            max_processing_time: Duration::from_secs(30),
            max_content_size: 10 * 1024 * 1024, // 10MB
        }
    }

    /// 处理爬虫抓取的内容
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

        // 检查内容大小
        if content_size > self.max_content_size {
            return Err(CrawlProcessingError::ContentTooLarge {
                size: content_size,
                max_size: self.max_content_size,
            });
        }

        // 检查处理时间
        if start_time.elapsed() > self.max_processing_time {
            return Err(CrawlProcessingError::ProcessingTimeout);
        }

        // 处理文本编码
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

    /// 带超时的内容处理
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

    /// 批量处理多个抓取的内容
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

    /// 快速处理简单文本（用于标题、描述等）
    pub fn process_simple_text(&self, text: &str) -> Result<String, CrawlProcessingError> {
        if text.is_empty() {
            return Ok(String::new());
        }

        // 使用文本编码处理器处理简单文本
        match process_text_encoding(text.as_bytes()) {
            Ok(processed) => Ok(processed),
            Err(e) => Err(CrawlProcessingError::TextEncodingError(e)),
        }
    }

    /// 验证内容质量
    pub fn validate_content_quality(&self, content: &ProcessedCrawlContent) -> ContentQuality {
        let text_length = content.processed_content.extracted_text.len();
        let word_count = content
            .processed_content
            .extracted_text
            .split_whitespace()
            .count();

        // 检查文本长度
        if text_length < 50 {
            return ContentQuality::Poor("文本内容过短".to_string());
        }

        // 检查单词数量
        if word_count < 10 {
            return ContentQuality::Poor("单词数量过少".to_string());
        }

        // 检查是否包含有效内容
        let text_ratio = text_length as f64 / content.original_size as f64;
        if text_ratio < 0.01 && content.original_size > 1000 {
            return ContentQuality::Poor("有效内容比例过低".to_string());
        }

        // 检查是否可能是垃圾内容
        let repetitive_ratio =
            self.calculate_repetitive_ratio(&content.processed_content.extracted_text);
        if repetitive_ratio > 0.8 {
            return ContentQuality::Poor("内容重复度过高".to_string());
        }

        // 根据处理时间和内容质量给出评级
        if content.processing_time.as_secs() > 10 {
            ContentQuality::Good
        } else {
            ContentQuality::Excellent
        }
    }

    /// 计算文本重复度
    fn calculate_repetitive_ratio(&self, text: &str) -> f64 {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 10 {
            return 0.0;
        }

        let mut word_counts = std::collections::HashMap::new();
        for word in &words {
            *word_counts.entry(word.to_lowercase()).or_insert(0) += 1;
        }

        let max_count = *word_counts.values().max().unwrap_or(&1);
        let total_words = words.len();

        (max_count as f64 / total_words as f64).min(1.0)
    }

    /// 获取处理器配置
    pub fn get_config(&self) -> CrawlProcessorConfig {
        CrawlProcessorConfig {
            max_processing_time_secs: self.max_processing_time.as_secs(),
            max_content_size_mb: self.max_content_size / (1024 * 1024),
        }
    }

    /// 更新处理器配置
    pub fn update_config(&mut self, config: CrawlProcessorConfig) {
        self.max_processing_time = Duration::from_secs(config.max_processing_time_secs);
        self.max_content_size = config.max_content_size_mb * 1024 * 1024;

        info!("更新爬虫文本处理器配置: {:?}", config);
    }
}

/// 处理后的爬虫内容
#[derive(Debug, Clone)]
pub struct ProcessedCrawlContent {
    pub url: String,
    pub processed_content: ProcessedWebContent,
    pub processing_time: Duration,
    pub original_size: usize,
    pub processed_size: usize,
}

/// 爬虫内容处理错误
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

/// 内容质量评级
#[derive(Debug, Clone, PartialEq)]
pub enum ContentQuality {
    Excellent,
    Good,
    Fair,
    Poor(String),
}

/// 爬虫处理器配置
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

/// 便捷函数：处理爬虫内容
pub fn process_crawled_content(
    content: &[u8],
    url: &str,
    content_type: Option<&str>,
) -> Result<ProcessedCrawlContent, CrawlProcessingError> {
    CrawlTextProcessor::new().process_crawled_content(content, url, content_type)
}

/// 便捷函数：批量处理爬虫内容
pub fn process_crawled_batch(
    batch: Vec<(&[u8], &str, Option<&str>)>,
) -> Vec<Result<ProcessedCrawlContent, CrawlProcessingError>> {
    CrawlTextProcessor::new().process_batch(batch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text_processing() {
        let processor = CrawlTextProcessor::new();
        let text = "Hello, 世界! This is a test.";

        let result = processor.process_simple_text(text).unwrap();
        assert_eq!(result, text);
    }

    #[test]
    fn test_content_quality_validation() {
        let processor = CrawlTextProcessor::new();

        // 创建模拟的处理内容
        let processed_content = ProcessedWebContent {
            original_content: "Test content".to_string(),
            extracted_text: "This is a sufficiently long test content with enough words to pass quality checks. It should contain meaningful information that represents good quality content for web crawling purposes.".to_string(),
            is_html: false,
            declared_encoding: None,
            detected_language: Some("en".to_string()),
            content_type: Some("text/plain".to_string()),
            content_length: 100,
        };

        let crawl_content = ProcessedCrawlContent {
            url: "http://example.com".to_string(),
            processed_content,
            processing_time: Duration::from_secs(5),
            original_size: 1000,
            processed_size: 200,
        };

        let quality = processor.validate_content_quality(&crawl_content);
        assert!(matches!(
            quality,
            ContentQuality::Good | ContentQuality::Excellent
        ));
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

    #[test]
    fn test_processor_config() {
        let processor = CrawlTextProcessor::new();
        let config = processor.get_config();

        assert_eq!(config.max_processing_time_secs, 30);
        assert_eq!(config.max_content_size_mb, 10);
    }
}
