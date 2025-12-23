// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 文本编码处理功能使用示例
//!
//! 本示例展示了如何在现有的爬虫系统中集成和使用文本编码处理功能

use crate::utils::crawl_text_integration::{CrawlTextIntegration, ScrapeResponseInput};
use crate::utils::crawl_text_processor::CrawlTextProcessor;
use crate::utils::text_encoding::TextEncodingProcessor;
use crate::utils::web_content_processor::WebContentProcessor;
use tracing::{error, info};

/// 示例1: 基础文本编码处理
///
/// 展示如何使用文本编码处理器处理单个文本内容
pub fn example_basic_text_encoding() {
    info!("=== 示例1: 基础文本编码处理 ===");

    let processor = TextEncodingProcessor::new();

    // 示例1: 处理UTF-8编码的中文内容
    let utf8_content = "这是一个UTF-8编码的中文测试内容。This is UTF-8 encoded Chinese text.";
    match processor.process_text(utf8_content.as_bytes()) {
        Ok(processed) => {
            info!("UTF-8内容处理成功: {}", processed);
        }
        Err(e) => {
            error!("UTF-8内容处理失败: {}", e);
        }
    }

    // 示例2: 处理包含Unicode转义的内容
    let unicode_content = r#"Unicode转义序列: \u4e2d\u6587 \u6d4b\u8bd5"#;
    match processor.process_text(unicode_content.as_bytes()) {
        Ok(processed) => {
            info!("Unicode转义处理成功: {}", processed);
        }
        Err(e) => {
            error!("Unicode转义处理失败: {}", e);
        }
    }

    // 示例3: 处理短文本（性能优化路径）
    let short_content = "短文本测试";
    match processor.process_text(short_content.as_bytes()) {
        Ok(processed) => {
            info!("短文本处理成功: {}", processed);
        }
        Err(e) => {
            error!("短文本处理失败: {}", e);
        }
    }
}

/// 示例2: 网页内容处理
///
/// 展示如何处理网页内容，包括HTML解析和文本提取
pub fn example_web_content_processing() {
    info!("=== 示例2: 网页内容处理 ===");

    let processor = WebContentProcessor::new();

    // 示例1: 处理HTML内容
    let html_content = r#"
    <!DOCTYPE html>
    <html>
    <head>
        <meta charset="utf-8">
        <title>测试页面</title>
    </head>
    <body>
        <h1>这是一个测试页面</h1>
        <p>包含中文内容和English text。</p>
        <div class="content">
            <p>更多的内容在这里。</p>
        </div>
    </body>
    </html>
    "#;

    match processor.process_web_content(html_content.as_bytes(), Some("text/html")) {
        Ok(processed) => {
            info!("HTML内容处理成功:");
            info!("  原始内容长度: {}", processed.original_content.len());
            info!("  提取文本长度: {}", processed.extracted_text.len());
            info!("  是否为HTML: {}", processed.is_html);
            info!("  检测到的语言: {:?}", processed.detected_language);
            info!("  声明的编码: {:?}", processed.declared_encoding);
            info!("  提取的文本: {}", processed.extracted_text);
        }
        Err(e) => {
            error!("HTML内容处理失败: {}", e);
        }
    }

    // 示例2: 处理纯文本内容
    let plain_text = "这是一个纯文本内容。\n包含多行文本。\nNo HTML tags here.";
    match processor.process_web_content(plain_text.as_bytes(), Some("text/plain")) {
        Ok(processed) => {
            info!("纯文本内容处理成功:");
            info!("  提取文本: {}", processed.extracted_text);
            info!("  是否为HTML: {}", processed.is_html);
        }
        Err(e) => {
            error!("纯文本内容处理失败: {}", e);
        }
    }
}

/// 示例3: 爬虫内容处理
///
/// 展示如何使用爬虫文本处理器处理抓取的内容
pub fn example_crawl_text_processing() {
    info!("=== 示例3: 爬虫内容处理 ===");

    let processor = CrawlTextProcessor::new();

    // 示例1: 处理单个网页内容
    let url = "http://example.com";
    let html_content = r#"
    <!DOCTYPE html>
    <html>
    <head><title>示例页面</title></head>
    <body>
        <h1>欢迎访问示例页面</h1>
        <p>这是一个包含中文和English的测试页面。</p>
        <div class="main-content">
            <p>主要内容区域，包含重要的信息。</p>
        </div>
    </body>
    </html>
    "#;

    match processor.process_crawled_content(html_content.as_bytes(), url, Some("text/html")) {
        Ok(processed) => {
            info!("爬虫内容处理成功:");
            info!("  URL: {}", processed.url);
            info!("  原始大小: {} 字节", processed.original_size);
            info!("  处理后大小: {} 字符", processed.processed_size);
            info!("  处理时间: {:?}", processed.processing_time);
            info!("  提取文本: {}", processed.processed_content.extracted_text);

            // 验证内容质量
            let quality = processor.validate_content_quality(&processed);
            info!("  内容质量: {:?}", quality);
        }
        Err(e) => {
            error!("爬虫内容处理失败: {}", e);
        }
    }

    // 示例2: 批量处理多个内容
    let batch = vec![
        (
            r#"<html><body><h1>页面1</h1><p>内容1</p></body></html>"#.as_bytes(),
            "http://example1.com",
            Some("text/html"),
        ),
        (
            r#"<html><body><h1>页面2</h1><p>内容2</p></body></html>"#.as_bytes(),
            "http://example2.com",
            Some("text/html"),
        ),
        (
            b"Plain text content",
            "http://example3.com",
            Some("text/plain"),
        ),
    ];

    let results = processor.process_batch(batch);
    info!("批量处理结果:");
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(processed) => {
                info!(
                    "  内容 {} 处理成功: URL={}, 文本长度={}",
                    i + 1,
                    processed.url,
                    processed.processed_content.extracted_text.len()
                );
            }
            Err(e) => {
                error!("  内容 {} 处理失败: {}", i + 1, e);
            }
        }
    }
}

/// 示例4: 集成使用
///
/// 展示如何在现有爬虫系统中集成文本处理功能
pub async fn example_integration_usage() {
    info!("=== 示例4: 集成使用 ===");

    // 创建集成器（启用状态）
    let integration = CrawlTextIntegration::new(true);

    // 模拟爬虫响应
    let scrape_responses = vec![
        ScrapeResponseInput {
            content:
                r#"<html><body><h1>测试页面1</h1><p>这是第一个测试页面的内容。</p></body></html>"#
                    .as_bytes()
                    .to_vec(),
            url: "http://test1.com".to_string(),
            content_type: Some("text/html".to_string()),
            status_code: 200,
        },
        ScrapeResponseInput {
            content: b"Plain text content for testing.".to_vec(),
            url: "http://test2.com".to_string(),
            content_type: Some("text/plain".to_string()),
            status_code: 200,
        },
    ];

    // 批量处理爬虫响应
    let results = integration
        .process_batch_scrape_responses(scrape_responses)
        .await;

    info!("集成处理结果:");
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(processed) => {
                info!("  响应 {} 处理成功:", i + 1);
                info!("    URL: {}", processed.url);
                info!("    状态码: {}", processed.status_code);
                info!("    内容类型: {:?}", processed.content_type);
                info!("    处理成功: {}", processed.processing_success);
                info!("    提取文本长度: {}", processed.processed_content.len());
                info!("    检测到的语言: {:?}", processed.language_detected);
                info!("    是否为HTML: {}", processed.is_html);

                // 验证内容质量
                let quality = integration.validate_content(processed).await;
                info!("    内容质量: {:?}", quality);
            }
            Err(e) => {
                error!("  响应 {} 处理失败: {}", i + 1, e);
            }
        }
    }

    // 获取集成器状态
    let status = integration.get_status().await;
    info!(
        "集成器状态: 启用={}, 配置={:?}",
        status.enabled, status.config
    );
}

/// 示例5: 错误处理
///
/// 展示如何处理各种错误情况
pub fn example_error_handling() {
    info!("=== 示例5: 错误处理 ===");

    let processor = CrawlTextProcessor::new();

    // 示例1: 处理过大的内容
    let large_content = vec![b'A'; 20 * 1024 * 1024]; // 20MB内容
    match processor.process_crawled_content(&large_content, "http://example.com", None) {
        Ok(_) => {
            info!("大内容处理成功");
        }
        Err(e) => {
            info!("大内容处理失败（预期）: {}", e);
        }
    }

    // 示例2: 处理空内容
    let empty_content = b"";
    match processor.process_crawled_content(empty_content, "http://example.com", None) {
        Ok(processed) => {
            info!(
                "空内容处理成功: 提取文本长度={}",
                processed.processed_content.extracted_text.len()
            );
        }
        Err(e) => {
            error!("空内容处理失败: {}", e);
        }
    }

    // 示例3: 处理无效编码的内容
    let invalid_encoding = vec![0xFF, 0xFE, 0xFD, 0xFC]; // 无效的字节序列
    match processor.process_crawled_content(&invalid_encoding, "http://example.com", None) {
        Ok(_) => {
            info!("无效编码处理成功");
        }
        Err(e) => {
            info!("无效编码处理失败（预期）: {}", e);
        }
    }
}

/// 运行所有示例
pub async fn run_all_examples() {
    info!("开始运行文本编码处理功能示例");

    example_basic_text_encoding();
    example_web_content_processing();
    example_crawl_text_processing();
    example_integration_usage().await;
    example_error_handling();

    info!("所有示例运行完成");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_examples() {
        example_basic_text_encoding();
        example_web_content_processing();
        example_crawl_text_processing();
        example_error_handling();
    }

    #[tokio::test]
    async fn test_integration_example() {
        example_integration_usage().await;
    }
}
