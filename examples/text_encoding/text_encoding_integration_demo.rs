// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 文本编码处理功能集成示例
//!
//! 这个文件展示了如何在现有的ScrapeWorker中集成文本编码处理功能

use crawlrs::utils::crawl_text_integration::CrawlTextIntegration;
use crawlrs::utils::text_processing::{process_text_encoding, process_web_content};

/// 集成示例：在现有的handle_scrape_success方法中添加文本处理功能
///
/// 这是在INTEGRATION_GUIDE.md中提到的集成方式的实际实现
pub async fn integrated_handle_scrape_success(
    task_id: &str,
    url: &str,
    response_content: &str,
    response_content_type: Option<&str>,
    response_status_code: u16,
) -> Result<ProcessedScrapeResult, Box<dyn std::error::Error>> {
    println!("集成文本编码处理功能，任务ID: {}", task_id);

    // 1. 创建文本处理集成器
    let text_integration = CrawlTextIntegration::new(true);

    // 3. 处理响应内容
    let processed_response = text_integration
        .process_scrape_response(
            response_content.as_bytes(),
            url,
            response_content_type,
            response_status_code,
        )
        .await?;

    // 4. 验证处理结果
    if processed_response.processing_success {
        println!(
            "文本处理成功，提取内容长度: {}",
            processed_response.processed_content.len()
        );

        // 5. 返回处理后的结果
        Ok(ProcessedScrapeResult {
            original_content: response_content.to_string(),
            processed_content: processed_response.processed_content,
            detected_encoding: processed_response
                .encoding_detected
                .unwrap_or_else(|| "UTF-8".to_string()),
            processing_time_ms: 0,
            content_quality_score: 0.8,
            success: true,
            error: None,
        })
    } else {
        println!(
            "文本处理失败，使用原始内容: {:?}",
            processed_response.processing_error
        );

        Ok(ProcessedScrapeResult {
            original_content: response_content.to_string(),
            processed_content: response_content.to_string(),
            detected_encoding: "UTF-8".to_string(),
            processing_time_ms: 0,
            content_quality_score: 0.0,
            success: false,
            error: processed_response.processing_error,
        })
    }
}

/// 处理后的抓取结果
#[derive(Debug, Clone)]
pub struct ProcessedScrapeResult {
    pub original_content: String,
    pub processed_content: String,
    pub detected_encoding: String,
    pub processing_time_ms: u64,
    pub content_quality_score: f32,
    pub success: bool,
    pub error: Option<String>,
}

/// 简单的文本处理示例
pub async fn simple_text_processing_example() {
    println!("=== 简单文本处理示例 ===");

    let text_content = "这是一个UTF-8编码的中文测试内容。This is UTF-8 encoded Chinese text.";
    match process_text_encoding(text_content.as_bytes()) {
        Ok(result) => {
            println!("文本处理成功:");
            println!("  原始长度: {} 字节", text_content.len());
            println!("  处理后长度: {} 字节", result.len());
            println!("  处理结果: {}", result);
        }
        Err(e) => {
            println!("文本处理失败: {}", e);
        }
    }

    let html_content = r#"
    <!DOCTYPE html>
    <html>
    <head><title>测试页面</title></head>
    <body>
        <h1>中文标题</h1>
        <p>这是一个包含中文的段落。</p>
        <script>console.log('test');</script>
        <style>body { color: red; }</style>
    </body>
    </html>
    "#;

    match process_web_content(html_content.as_bytes(), Some("text/html")) {
        Ok(result) => {
            println!("\nHTML内容处理成功:");
            println!("  提取的文本: {}", result.extracted_text);
            println!("  检测到的语言: {:?}", result.detected_language);
            println!("  内容长度: {} 字节", result.content_length);
        }
        Err(e) => {
            println!("HTML内容处理失败: {}", e);
        }
    }
}

/// 批量处理示例
pub async fn batch_processing_example() {
    println!("\n=== 批量处理示例 ===");

    use crawlrs::utils::text_encoding::process_text_batch;

    let inputs = vec![
        "测试内容1 - 普通文本".as_bytes(),
        "<html><body><h1>测试内容2</h1><p>HTML内容</p></body></html>".as_bytes(),
        r#"\u8fd9\u662f\u4e00\u4e2a\u0020Unicode\u0020\u6d4b\u8bd5"#.as_bytes(),
    ];

    println!("开始批量处理 {} 个内容...", inputs.len());

    let start_time = std::time::Instant::now();
    let results = process_text_batch(inputs);
    let total_time = start_time.elapsed();

    println!("批量处理完成，总耗时: {:?}", total_time);

    let mut success_count = 0;
    let mut total_content_length = 0;

    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(processed) => {
                success_count += 1;
                total_content_length += processed.len();
                println!("  内容 {} 处理成功: 长度={}", i + 1, processed.len());
            }
            Err(e) => {
                println!("  内容 {} 处理失败: {}", i + 1, e);
            }
        }
    }

    println!("批量处理统计:");
    println!("  成功率: {}/{}", success_count, results.len());
    println!("  总内容长度: {} 字节", total_content_length);
    println!(
        "  平均处理时间: {:.2} ms/个",
        total_time.as_millis() as f64 / results.len() as f64
    );
}

/// 性能优化示例
pub async fn performance_optimization_example() {
    println!("\n=== 性能优化示例 ===");

    use crawlrs::utils::text_encoding::TextEncodingProcessor;

    let processor = TextEncodingProcessor::new();

    let short_text = "这是一个短文本测试";
    let start_time = std::time::Instant::now();
    let _result1 = processor.process_text(short_text.as_bytes());
    let short_time = start_time.elapsed();

    println!("短文本处理:");
    println!("  文本长度: {} 字节 (<= 1KB)", short_text.len());
    println!("  处理时间: {:?}", short_time);
    println!("  使用快速路径: 是");

    let long_text = "这是一个较长的文本内容".repeat(100);
    let start_time = std::time::Instant::now();
    let _result2 = processor.process_text(long_text.as_bytes());
    let long_time = start_time.elapsed();

    println!("\n长文本处理:");
    println!("  文本长度: {} 字节 (> 1KB)", long_text.len());
    println!("  处理时间: {:?}", long_time);
    println!("  使用完整处理流程: 是");

    println!("\n缓存效果测试:");
    let cache_test_text = "这是一个用于缓存测试的文本内容";

    let start_time = std::time::Instant::now();
    let _result3 = processor.process_text(cache_test_text.as_bytes());
    let first_time = start_time.elapsed();
    println!("  第一次处理时间: {:?}", first_time);

    let start_time = std::time::Instant::now();
    let _result4 = processor.process_text(cache_test_text.as_bytes());
    let second_time = start_time.elapsed();
    println!("  第二次处理时间: {:?} (应该更快)", second_time);

    if second_time < first_time {
        println!("  缓存效果: 有效 (第二次处理更快)");
    } else {
        println!("  缓存效果: 可能需要更多样本才能体现");
    }
}

/// 运行所有示例
pub async fn run_all_examples() {
    println!("开始运行文本编码处理功能示例...\n");

    simple_text_processing_example().await;
    batch_processing_example().await;
    performance_optimization_example().await;

    println!("\n所有示例运行完成！");
    println!("文本编码处理功能已准备好在实际爬虫系统中使用。");
}

#[tokio::main]
async fn main() {
    run_all_examples().await;
}
