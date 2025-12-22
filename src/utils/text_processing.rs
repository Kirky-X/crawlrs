// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 文本编码处理模块
//!
//! 提供全面的文本编码处理功能，包括：
//! - Unicode检测与转换
//! - 编码格式检测
//! - 编码转换处理
//! - 错误处理与日志记录
//! - 性能优化
//!
//! # 使用示例
//!
//! ```rust
//! use crawlrs::utils::text_encoding::{process_text_encoding, TextEncodingError};
//!
//! fn main() -> Result<(), TextEncodingError> {
//!     let content = "Hello, 世界!";
//!     let processed = process_text_encoding(content.as_bytes())?;
//!     println!("处理后的文本: {}", processed);
//!     Ok(())
//! }
//! ```

// 重新导出主要功能
pub use crate::utils::crawl_text_integration::{
    create_crawl_text_integration, CrawlTextIntegration, ProcessedScrapeResponse,
    ScrapeResponseInput,
};
pub use crate::utils::crawl_text_processor::{
    process_crawled_batch, process_crawled_content, CrawlProcessingError, CrawlTextProcessor,
    ProcessedCrawlContent,
};
pub use crate::utils::text_encoding::{
    process_text_encoding, TextEncodingError, TextEncodingProcessor,
};
pub use crate::utils::web_content_processor::{
    process_web_content, ProcessedWebContent, WebContentError, WebContentProcessor,
};

use tracing::{error, info};

/// 初始化文本编码处理模块
///
/// 在应用程序启动时调用此函数来初始化文本处理功能
pub fn init_text_encoding() {
    info!("初始化文本编码处理模块");

    // 可以在这里添加全局初始化逻辑
    // 例如：设置缓存大小、配置日志级别等

    info!("文本编码处理模块初始化完成");
}

/// 获取模块版本信息
pub fn get_text_encoding_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 模块健康检查
pub fn health_check() -> Result<(), String> {
    // 检查依赖库是否可用
    let test_content = "健康检查测试内容";
    match process_text_encoding(test_content.as_bytes()) {
        Ok(_) => {
            info!("文本编码处理模块健康检查通过");
            Ok(())
        }
        Err(e) => {
            error!("文本编码处理模块健康检查失败: {}", e);
            Err(format!("文本编码处理模块故障: {}", e))
        }
    }
}

/// 便捷函数：快速处理文本编码
///
/// 这是最简单的使用方式，适合快速集成
pub fn quick_process_text(input: &[u8]) -> Result<String, TextEncodingError> {
    process_text_encoding(input)
}

/// 便捷函数：处理网页内容
///
/// 处理网页内容，自动检测HTML结构并提取文本
pub fn quick_process_web_content(
    content: &[u8],
    content_type: Option<&str>,
) -> Result<ProcessedWebContent, WebContentError> {
    process_web_content(content, content_type)
}

/// 便捷函数：处理爬虫内容
///
/// 处理爬虫抓取的内容，包含完整的错误处理和质量验证
pub fn quick_process_crawl_content(
    content: &[u8],
    url: &str,
    content_type: Option<&str>,
) -> Result<ProcessedCrawlContent, CrawlProcessingError> {
    process_crawled_content(content, url, content_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_initialization() {
        init_text_encoding();
        assert!(health_check().is_ok());
    }

    #[test]
    fn test_quick_process_functions() {
        let content = "测试文本内容";

        // 测试快速文本处理
        let result = quick_process_text(content.as_bytes());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), content);

        // 测试快速网页内容处理
        let html = "<html><body><p>测试HTML</p></body></html>";
        let web_result = quick_process_web_content(html.as_bytes(), Some("text/html"));
        assert!(web_result.is_ok());
        let processed = web_result.unwrap();
        assert!(processed.extracted_text.contains("测试HTML"));

        // 测试快速爬虫内容处理
        let crawl_result =
            quick_process_crawl_content(html.as_bytes(), "http://test.com", Some("text/html"));
        assert!(crawl_result.is_ok());
        let processed_crawl = crawl_result.unwrap();
        assert!(processed_crawl
            .processed_content
            .extracted_text
            .contains("测试HTML"));
    }

    #[test]
    fn test_version_info() {
        let version = get_text_encoding_version();
        assert!(!version.is_empty());
        info!("文本编码处理模块版本: {}", version);
    }
}
