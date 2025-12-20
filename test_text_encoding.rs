//! 文本编码处理功能测试
//! 
//! 这个测试文件专门用于验证文本编码处理模块的功能

use crawlrs::utils::text_encoding::{TextEncodingProcessor, process_text_encoding};
use crawlrs::utils::web_content_processor::{WebContentProcessor, process_web_content};

#[tokio::test]
async fn test_unicode_detection_and_conversion() {
    // 测试Unicode检测和转换
    let processor = TextEncodingProcessor::new();
    
    // 测试包含Unicode转义的中文文本
    let unicode_text = r#"\u8fd9\u662f\u4e00\u4e2a\u4e2d\u6587\u6d4b\u8bd5"#;
    let result = processor.detect_and_convert_unicode(unicode_text);
    assert!(result.is_ok());
    
    let converted = result.unwrap();
    println!("Unicode转换结果: {}", converted);
    assert!(converted.contains("这是一个中文测试"));
}

#[tokio::test]
async fn test_encoding_detection() {
    // 测试编码检测
    let processor = TextEncodingProcessor::new();
    
    // 测试UTF-8编码的中文文本
    let utf8_text = "这是一个UTF-8编码的中文测试内容";
    let result = processor.detect_encoding(utf8_text.as_bytes());
    
    assert!(result.is_ok());
    let detection = result.unwrap();
    println!("编码检测结果: {:?}", detection);
    assert_eq!(detection.encoding, "UTF-8");
    assert!(detection.is_utf8);
}

#[tokio::test]
async fn test_text_encoding_integration() {
    // 测试完整的文本编码处理流程
    let text = "这是一个测试文本，包含中文和English混合内容。";
    let result = process_text_encoding(text.as_bytes()).await;
    
    assert!(result.is_ok());
    let processed = result.unwrap();
    println!("文本编码处理结果: {}", processed.text);
    assert!(processed.text.contains("这是一个测试文本"));
}

#[tokio::test]
async fn test_web_content_processing() {
    // 测试网页内容处理
    let html_content = r#"
    <!DOCTYPE html>
    <html>
    <head><title>测试页面</title></head>
    <body>
        <h1>中文标题</h1>
        <p>这是一个包含中文的段落。</p>
        <script>console.log('test');</script>
    </body>
    </html>
    "#;
    
    let result = process_web_content(html_content.as_bytes(), "https://example.com").await;
    assert!(result.is_ok());
    
    let processed = result.unwrap();
    println!("网页内容处理结果: {}", processed.cleaned_text);
    assert!(processed.cleaned_text.contains("中文标题"));
    assert!(processed.cleaned_text.contains("这是一个包含中文的段落"));
    assert!(!processed.cleaned_text.contains("script")); // 应该已经移除了script标签
}

#[tokio::test]
async fn test_batch_processing() {
    // 测试批量处理功能
    use crawlrs::utils::crawl_text_processor::{CrawlTextProcessor, CrawlTextInput};
    
    let processor = CrawlTextProcessor::new();
    
    let inputs = vec![
        CrawlTextInput {
            content: "测试内容1".as_bytes().to_vec(),
            url: "https://example1.com".to_string(),
            content_type: "text/plain".to_string(),
        },
        CrawlTextInput {
            content: "测试内容2".as_bytes().to_vec(),
            url: "https://example2.com".to_string(),
            content_type: "text/html".to_string(),
        },
    ];
    
    let results = processor.process_batch(inputs).await;
    assert_eq!(results.len(), 2);
    
    for result in results {
        assert!(result.is_ok());
        let processed = result.unwrap();
        println!("批量处理结果: URL={}, 内容长度={}", processed.url, processed.text.len());
    }
}

fn main() {
    // 简单的功能验证
    println!("开始文本编码处理功能测试...");
    
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        test_unicode_detection_and_conversion().await;
        println!("✓ Unicode检测和转换测试通过");
        
        test_encoding_detection().await;
        println!("✓ 编码检测测试通过");
        
        test_text_encoding_integration().await;
        println!("✓ 文本编码集成测试通过");
        
        test_web_content_processing().await;
        println!("✓ 网页内容处理测试通过");
        
        test_batch_processing().await;
        println!("✓ 批量处理测试通过");
    });
    
    println!("所有测试通过！文本编码处理功能正常工作。");
}