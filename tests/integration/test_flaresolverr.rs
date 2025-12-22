#[tokio::main]
async fn main() {
    println!("=== 测试FlareSolverr服务 ===");

    let client = reqwest::Client::new();

    // 测试1: 检查FlareSolverr状态
    println!("\n1. 检查FlareSolverr状态...");
    match client.get("http://localhost:8191").send().await {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                println!(
                    "✓ FlareSolverr状态: {}",
                    json["msg"].as_str().unwrap_or("未知")
                );
                println!("  版本: {}", json["version"].as_str().unwrap_or("未知"));
            }
            Err(e) => println!("✗ 解析FlareSolverr响应失败: {}"),
        },
        Err(e) => println!("✗ 连接FlareSolverr失败: {}"),
    }

    // 测试2: 使用FlareSolverr访问Google
    println!("\n2. 使用FlareSolverr访问Google...");
    let flaresolverr_request = serde_json::json!({
        "cmd": "request.get",
        "url": "https://www.google.com/search?q=鸿蒙星光大赏",
        "maxTimeout": 60000,
        "headers": {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        }
    });

    match client
        .post("http://localhost:8191/v1")
        .json(&flaresolverr_request)
        .send()
        .await
    {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if json["status"].as_str() == Some("ok") {
                    println!("✓ FlareSolverr成功绕过CF验证");
                    if let Some(solution) = json["solution"].as_object() {
                        println!("  响应状态: {:?}", solution["status"]);
                        println!("  内容长度: {} 字符", solution["response"].as_str().map(|s| s.len()).unwrap_or(0));
                    }
                } else {
                    println!("✗ FlareSolverr处理失败: {:?}", json["message"]);
                }
            }
            Err(e) => println!("✗ 解析FlareSolverr响应失败: {}"),
        },
        Err(e) => println!("✗ FlareSolverr请求失败: {}"),
    }
}