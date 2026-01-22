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
    process_crawled_batch_with_processor, process_crawled_content_with_processor,
    process_web_content_with_processor, ContentQuality, CrawlProcessingError, CrawlProcessorConfig,
    CrawlTextProcessor, ProcessedCrawlContent, ProcessedWebContent, WebContentError,
    WebContentProcessor, detect_html_structure, init_encoding_patterns,
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
