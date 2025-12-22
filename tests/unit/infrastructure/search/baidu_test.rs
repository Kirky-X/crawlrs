// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crawlrs::infrastructure::search::baidu::{BaiduSearchEngine, BaiduSearchCategory};
    
    #[test]
    fn test_baidu_url_construction() {
        let engine = BaiduSearchEngine::new();
        
        // 测试通用搜索URL构建
        let (url, params) = engine.build_baidu_url("测试查询", 1, BaiduSearchCategory::General);
        
        assert_eq!(url, "https://www.baidu.com/s");
        assert_eq!(params.get("wd"), Some(&"测试查询".to_string()));
        assert_eq!(params.get("rn"), Some(&"10".to_string()));
        assert_eq!(params.get("pn"), Some(&"0".to_string()));
        assert_eq!(params.get("tn"), Some(&"json".to_string()));
        
        // 测试第二页
        let (_url, params) = engine.build_baidu_url("测试查询", 2, BaiduSearchCategory::General);
        assert_eq!(params.get("pn"), Some(&"10".to_string()));
        
        // 测试图片搜索
        let (url, params) = engine.build_baidu_url("图片查询", 1, BaiduSearchCategory::Images);
        assert_eq!(url, "https://image.baidu.com/search/acjson");
        assert_eq!(params.get("word"), Some(&"图片查询".to_string()));
        assert_eq!(params.get("tn"), Some(&"resultjson_com".to_string()));
    }
    
    #[test]
    fn test_baidu_json_parsing() {
        let engine = BaiduSearchEngine::new();
        
        // 测试JSON解析
        let json_response = r#"{
            "feed": {
                "entry": [
                    {
                        "title": "测试标题&lt;div&gt;",
                        "url": "https://example.com",
                        "abs": "测试摘要&lt;span&gt;"
                    }
                ]
            }
        }"#;
        
        let results = engine.parse_baidu_response(json_response).unwrap();
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "测试标题<div>"); // HTML转义应该被解码
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].description, Some("测试摘要<span>".to_string()));
    }
    
    #[test]
    fn test_baidu_empty_response() {
        let engine = BaiduSearchEngine::new();
        
        // 测试空响应
        let empty_response = r#"{"feed": {}}"#;
        let results = engine.parse_baidu_response(empty_response).unwrap();
        assert_eq!(results.len(), 0);
        
        let no_feed_response = r#"{}"#;
        let results = engine.parse_baidu_response(no_feed_response).unwrap();
        assert_eq!(results.len(), 0);
        
        let no_entry_response = r#"{"feed": {"entry": null}}"#;
        let results = engine.parse_baidu_response(no_entry_response).unwrap();
        assert_eq!(results.len(), 0);
    }
    
    #[test]
    fn test_baidu_malformed_entry() {
        let engine = BaiduSearchEngine::new();
        
        // 测试缺少必要字段的条目
        let incomplete_response = r#"{
            "feed": {
                "entry": [
                    {
                        "title": "只有标题"
                    },
                    {
                        "url": "https://only-url.com"
                    }
                ]
            }
        }"#;
        
        let results = engine.parse_baidu_response(incomplete_response).unwrap();
        assert_eq!(results.len(), 0); // 应该过滤掉不完整的条目
    }

    #[tokio::test]
    async fn test_baidu_search_hongmeng() {
        use crawlrs::domain::search::engine::SearchEngine;
        
        println!("测试百度搜索引擎 - 搜索: 鸿蒙星光大赏");
        
        let engine = BaiduSearchEngine::new();
        let results = match engine.search("鸿蒙星光大赏", 10, None, None).await {
            Ok(results) => results,
            Err(e) => {
                // 网络错误或验证码在测试环境中是可以接受的
                if e.to_string().contains("CAPTCHA") || e.to_string().contains("rate limiting") {
                    println!("⚠️  百度搜索需要验证码或被限制，跳过测试: {}", e);
                    return; // 优雅地跳过测试
                }
                panic!("百度搜索测试失败: {}", e);
            }
        };
        
        println!("找到 {} 个结果:", results.len());
        for (i, result) in results.iter().enumerate() {
            println!("结果 {}: {}", i + 1, result.title);
            println!("  URL: {}", result.url);
            println!("  描述: {}", result.description.as_ref().unwrap_or(&"无描述".to_string()));
            println!("  引擎: {}", result.engine);
            println!();
        }
        
        // 验证结果
        assert!(!results.is_empty(), "应该找到至少一个结果");
        
        // 检查是否包含相关关键词
        let has_hongmeng = results.iter().any(|r| 
            r.title.contains("鸿蒙") || 
            r.description.as_ref().map_or(false, |d| d.contains("鸿蒙")) ||
            r.title.contains("星光大赏") || 
            r.description.as_ref().map_or(false, |d| d.contains("星光大赏"))
        );
        
        if has_hongmeng {
            println!("✓ 找到包含'鸿蒙'或'星光大赏'的相关结果");
        } else {
            println!("! 未找到直接包含关键词的结果，但找到 {} 个搜索结果", results.len());
        }
    }
}