use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use std::env;

#[tokio::main]
async fn main() {
    println!("=== 使用远程Chrome测试Google搜索（增加超时时间） ===");

    // 获取最新的WebSocket URL
    let ws_url = get_chrome_ws_url().await;
    println!("使用远程Chrome: {}", ws_url);
    env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", &ws_url);

    // 创建Google搜索引擎
    let google_engine = GoogleSearchEngine::new();

    // 测试几个不同的搜索词
    let test_queries = vec!["鸿蒙星光大赏", "HarmonyOS", "华为"];

    for query in test_queries {
        println!("\n🔍 搜索关键词: {}", query);

        match google_engine
            .search(query, 3, Some("zh-CN"), Some("CN"))
            .await
        {
            Ok(results) => {
                println!("✓ 搜索成功！找到 {} 个结果", results.len());

                for (i, result) in results.iter().enumerate().take(2) {
                    println!("  {}. {} - {}", i + 1, result.title, result.url);
                    println!("     描述: {}", result.description);
                }
            }
            Err(e) => {
                println!("✗ 搜索失败: {:?}", e);
            }
        }

        // 等待一段时间，避免过于频繁的请求
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

async fn get_chrome_ws_url() -> String {
    // 这里应该实现获取Chrome WebSocket URL的逻辑
    // 现在使用固定的URL作为示例
    "ws://localhost:9222/devtools/browser/16bfd1e5-af2b-45c4-85c2-9d8ac98d2817".to_string()
}