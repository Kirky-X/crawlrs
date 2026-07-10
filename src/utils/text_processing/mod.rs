// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 文本处理模块
//!
//! 提供统一的文本编码处理和内容处理功能，包括：
//! - 编码检测与转换
//! - 网页内容解析
//! - 爬虫内容处理
//!
//! # 使用示例
//!
//! ```
//! use crawlrs::utils::text_processing::{process_web_content, process_crawled_content};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let html = b"<html><body><p>Hello, World!</p></body></html>";
//!     let processed = process_web_content(html, Some("text/html"))?;
//!     println!("Extracted: {}", processed.extracted_text);
//!     Ok(())
//! }
//! ```

pub mod encoding;
pub mod processor;

pub use encoding::{
    process_batch_with_processor, process_with_processor, EncodingDetection, TextEncodingError,
    TextEncodingProcessor, TextProcessorStats,
};
pub use processor::{
    detect_html_structure, init_encoding_patterns, process_crawled_batch_with_processor,
    process_crawled_content_with_processor, process_web_content_with_processor, ContentQuality,
    CrawlProcessingError, CrawlProcessorConfig, CrawlTextProcessor, ProcessedCrawlContent,
    ProcessedWebContent, WebContentError, WebContentProcessor,
};

/// 初始化文本处理模块（可选，用于预热默认实例）
/// 注意：此函数为向后兼容保留，现在推荐直接使用处理器实例
pub fn init_text_processing() {
    // 现在使用可配置的处理器而不是全局单例
    // 保留此函数以保持向后兼容
}

/// 模块健康检查
pub fn health_check() -> Result<(), String> {
    let test_content = "健康检查测试内容";
    match TextEncodingProcessor::new().process_text(test_content.as_bytes()) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("文本处理模块故障: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== init_text_processing tests ==========

    #[test]
    fn test_init_text_processing_does_not_panic() {
        // init_text_processing is a no-op retained for backward compatibility.
        // It should simply return without panicking.
        init_text_processing();
    }

    // ========== health_check tests ==========

    #[test]
    fn test_health_check_returns_ok_with_valid_content() {
        let result = health_check();
        assert!(
            result.is_ok(),
            "health_check should return Ok for valid UTF-8 content"
        );
    }

    #[test]
    fn test_health_check_ok_value_is_unit() {
        let result = health_check();
        assert!(result.is_ok());
        // Verify the Ok variant carries unit ()
        result.expect("health_check should succeed");
    }

    // ========== TextEncodingProcessor (re-exported) tests ==========

    #[test]
    fn test_text_encoding_processor_new_does_not_panic() {
        let _processor = TextEncodingProcessor::new();
    }

    #[test]
    fn test_text_encoding_processor_processes_ascii() {
        let processor = TextEncodingProcessor::new();
        let input = b"Hello, World!";
        let result = processor.process_text(input);
        assert!(result.is_ok(), "process_text should succeed on ASCII input");
        let processed = result.expect("ASCII processing should succeed");
        assert!(
            !processed.is_empty(),
            "processed ASCII text should be non-empty"
        );
    }

    #[test]
    fn test_text_encoding_processor_processes_chinese() {
        let processor = TextEncodingProcessor::new();
        let input = "你好，世界".as_bytes();
        let result = processor.process_text(input);
        assert!(
            result.is_ok(),
            "process_text should succeed on Chinese input"
        );
    }

    #[test]
    fn test_text_encoding_processor_processes_empty_input() {
        let processor = TextEncodingProcessor::new();
        let result = processor.process_text(b"");
        assert!(result.is_ok(), "process_text should succeed on empty input");
    }

    // ========== re-exports presence tests ==========

    #[test]
    fn test_process_with_processor_is_callable() {
        // Verify the re-exported function exists and is callable with a processor.
        let processor = TextEncodingProcessor::new();
        let input = b"re-export test";
        let result = process_with_processor(&processor, input);
        assert!(
            result.is_ok(),
            "process_with_processor should succeed on ASCII"
        );
    }

    #[test]
    fn test_process_batch_with_processor_empty_batch() {
        let processor = TextEncodingProcessor::new();
        let batch: Vec<&[u8]> = vec![];
        let results = process_batch_with_processor(&processor, batch);
        assert!(
            results.is_empty(),
            "empty batch should produce empty results"
        );
    }
}
