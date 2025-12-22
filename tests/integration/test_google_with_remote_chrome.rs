use crawlrs::domain::search::engine::SearchEngine;
use crawlrs::infrastructure::search::google::GoogleSearchEngine;
use std::env;

#[tokio::main]
async fn main() {
    println!("=== 使用远程Chrome测试Google搜索 ===");

    // 设置远程Chrome调试URL - 使用正确的变量名
    env::set_var(
        "CHROMIUM_REMOTE_DEBUGGING_URL",
        "ws://localhost:9222/devtools/browser/16bfd1e5-af2b-45c4-85c2-9d8ac98d2817",
    );

    println!(
        "使用远程Chrome: {}",
        env::var("CHROMIUM_REMOTE_DEBUGGING_URL").unwrap_or_default()
    );

    // 创建Google搜索引擎
    let google_engine = GoogleSearchEngine::new();

    // 测试搜索
    let query = "鸿蒙星光大赏";
    println!("\n搜索关键词: {}", query);

    match google_engine
        .search(query, 5, Some("zh-CN"), Some("CN"))
        .await
    {
        Ok(results) => {
            println!("✅ 搜索成功！找到 {} 个结果", results.len());
            for (i, result) in results.iter().enumerate() {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
                println!("     描述: {}", result.description);
            }
        }
        Err(e) => {
            println!("❌ 搜索失败: {:?}", e);
        }
    }
}