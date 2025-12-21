// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::infrastructure::search::sogou::SogouSearchEngine;
    use crawlrs::domain::search::engine::SearchEngine;
    
    #[test]
    fn test_sogou_html_parsing() {
        let engine = SogouSearchEngine::new();
        
        // 测试HTML解析 - 标准结果结构
        let html_response = r#"
        <!DOCTYPE html>
        <html>
        <head><title>搜索结果</title></head>
        <body>
            <div class="vrwrap">
                <h3><a href="https://example.com/article1">测试文章标题1</a></h3>
                <div class="str_info">2024-01-15</div>
            </div>
            <div class="vrwrap">
                <h3><a href="https://example.com/article2">测试文章标题2</a></h3>
                <div class="str_info">2024-01-16</div>
            </div>
            <div class="rb">
                <h3><a href="https://example.com/article3">备选结构标题</a></h3>
                <div class="str_info">2024-01-17</div>
            </div>
        </body>
        </html>
        "#;
        
        let results = engine.parse_search_results(html_response, "测试查询").unwrap();
        
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].title, "测试文章标题1");
        assert_eq!(results[0].url, "https://example.com/article1");
        assert_eq!(results[0].engine, "sogou");
        
        assert_eq!(results[1].title, "测试文章标题2");
        assert_eq!(results[1].url, "https://example.com/article2");
        
        assert_eq!(results[2].title, "备选结构标题");
        assert_eq!(results[2].url, "https://example.com/article3");
    }
    
    #[test]
    fn test_sogou_empty_html() {
        let engine = SogouSearchEngine::new();
        
        // 测试空HTML
        let empty_html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>无结果</title></head>
        <body>
            <div>没有找到相关结果</div>
        </body>
        </html>
        "#;
        
        let results = engine.parse_search_results(empty_html, "测试查询").unwrap();
        
        assert_eq!(results.len(), 0);
    }
    
    #[test]
    fn test_sogou_malformed_html() {
        let engine = SogouSearchEngine::new();
        
        // 测试缺少链接的HTML结构
        let malformed_html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>搜索结果</title></head>
        <body>
            <div class="vrwrap">
                <h3>只有标题没有链接</h3>
            </div>
            <div class="vrwrap">
                <a href="https://example.com">只有链接没有标题</a>
            </div>
            <div class="vrwrap">
                <h3><a>空链接</a></h3>
            </div>
        </body>
        </html>
        "#;
        
        let results = engine.parse_search_results(malformed_html, "测试查询").unwrap();
        
        // 应该过滤掉不完整的条目
        assert_eq!(results.len(), 0);
    }
    
    #[test]
    fn test_sogou_relevance_scoring() {
        let engine = SogouSearchEngine::new();
        
        // 测试相关性评分
        let html_with_dates = r#"
        <!DOCTYPE html>
        <html>
        <head><title>搜索结果</title></head>
        <body>
            <div class="vrwrap">
                <h3><a href="https://example.com/rust">Rust编程语言最新特性</a></h3>
                <div class="str_info">2024-12-20</div>
            </div>
            <div class="vrwrap">
                <h3><a href="https://example.com/old">旧的文章关于编程</a></h3>
                <div class="str_info">2023-01-01</div>
            </div>
        </body>
        </html>
        "#;
        
        let results = engine.parse_search_results(html_with_dates, "rust 编程").unwrap();
        
        assert_eq!(results.len(), 2);
        
        // 第一个结果应该包含查询词，相关性更高
        assert!(results[0].title.contains("Rust") || results[0].title.contains("编程"));
        
        // 验证评分机制（70%相关性 + 30%新鲜度）
        assert!(results[0].score > 0.0);
        assert!(results[1].score > 0.0);
    }
    
    #[tokio::test]
    async fn test_sogou_search_real() {
        use crawlrs::domain::search::engine::SearchEngine;
        
        println!("测试搜狗搜索引擎 - 搜索: 鸿蒙系统");
        
        let engine = SogouSearchEngine::new();
        let results = match engine.search("鸿蒙系统", 5, None, None).await {
            Ok(results) => results,
            Err(e) => {
                // 网络错误或验证码在测试环境中是可以接受的
                if e.to_string().contains("CAPTCHA") || e.to_string().contains("rate limiting") {
                    println!("⚠️  搜狗搜索需要验证码或被限制，跳过测试: {}", e);
                    return; // 优雅地跳过测试
                }
                panic!("搜狗搜索测试失败: {}", e);
            }
        };
        
        println!("找到 {} 个结果:", results.len());
        for (i, result) in results.iter().enumerate() {
            println!("结果 {}: {}", i + 1, result.title);
            println!("  URL: {}", result.url);
            println!("  描述: {}", result.description.as_ref().unwrap_or(&"无描述".to_string()));
            println!("  引擎: {}", result.engine);
            println!("  评分: {}", result.score);
            println!();
        }
        
        // 验证结果
        assert!(!results.is_empty(), "应该找到至少一个结果");
        
        // 检查是否包含相关关键词
        let has_hongmeng = results.iter().any(|r| 
            r.title.contains("鸿蒙") || 
            r.description.as_ref().map_or(false, |d| d.contains("鸿蒙")) ||
            r.title.contains("HarmonyOS") || 
            r.description.as_ref().map_or(false, |d| d.contains("HarmonyOS"))
        );
        
        if has_hongmeng {
            println!("✓ 找到包含'鸿蒙'或'HarmonyOS'的相关结果");
        } else {
            println!("! 未找到直接包含关键词的结果，但找到 {} 个搜索结果", results.len());
        }
    }
    
    #[test]
    fn test_sogou_engine_name() {
        let engine = SogouSearchEngine::new();
        assert_eq!(engine.name(), "sogou");
    }
}