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
    process_string, process_text_batch, process_text_encoding, EncodingDetection,
    TextEncodingError, TextEncodingProcessor, TextProcessorStats,
};
pub use processor::{
    process_crawled_batch, process_crawled_content, process_web_content, ContentQuality,
    CrawlProcessingError, CrawlProcessorConfig, CrawlTextProcessor, ProcessedCrawlContent,
    ProcessedWebContent, WebContentError, WebContentProcessor,
};

/// 初始化文本处理模块
pub fn init_text_processing() {
    let _ = encoding::TextEncodingProcessor::global();
    let _ = processor::WebContentProcessor::global();
}

/// 模块健康检查
pub fn health_check() -> Result<(), String> {
    let test_content = "健康检查测试内容";
    match process_text_encoding(test_content.as_bytes()) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("文本处理模块故障: {}", e)),
    }
}
