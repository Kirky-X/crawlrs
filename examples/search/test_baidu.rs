// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Baidu 搜索引擎真实搜索测试

use crawlrs::search::client::baidu::BaiduSearchEngine;
use crawlrs::search::SearchEngine;
use tokio::time::{timeout, Duration};

const TIMEOUT_SECS: u64 = 60;
const TEST_KEYWORD: &str = "test";

#[tokio::main]
async fn main() {
    println!("🚀 测试 Baidu 搜索引擎\n");
    println!("测试关键词: {}", TEST_KEYWORD);
    println!("超时时间: {} 秒\n", TIMEOUT_SECS);

    match timeout(
        Duration::from_secs(TIMEOUT_SECS),
        BaiduSearchEngine::new().search(&crawlrs::search::SearchRequest::new(TEST_KEYWORD)),
    )
    .await
    {
        Ok(result) => match result {
            Ok(response) => {
                println!("✅ Baidu 搜索成功!");
                println!("结果数: {}\n", response.items.len());

                if response.items.is_empty() {
                    println!("⚠️ 没有解析出结果");
                    println!("💡 提示: 可能是解析器问题或搜索引擎返回空结果");
                } else {
                    for (i, item) in response.items.iter().enumerate() {
                        println!("{}. {}", i + 1, item.title);
                        println!("   URL: {}\n", item.url);
                    }
                }
            }
            Err(e) => {
                println!("❌ Baidu 搜索失败: {:?}", e);
            }
        },
        Err(_) => {
            println!("❌ Baidu 搜索超时 ({} 秒)", TIMEOUT_SECS);
        }
    }
}
