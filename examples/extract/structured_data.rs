// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 结构化数据提取示例
//!
//! 演示如何提取和结构化网页数据。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example structured_data
//!

use log::info;

/// 结构化数据项
#[derive(Debug, serde::Serialize)]
struct Product {
    name: String,
    price: f64,
    currency: String,
    description: String,
    rating: Option<f64>,
    in_stock: bool,
    category: String,
}

/// 文章信息
#[derive(Debug, serde::Serialize)]
struct Article {
    title: String,
    author: String,
    publish_date: String,
    content: String,
    tags: Vec<String>,
    read_time: u32,
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始结构化数据提取示例");
    info!("=====================================\n");

    // 1. 结构化数据介绍
    info!("1️⃣  结构化数据介绍");
    info!("-----------------------------");
    info!("");
    info!("📖 结构化数据提取将非结构化的HTML转换为有组织的格式");
    info!("   常见输出格式:");
    info!("   - JSON: 机器可读的标准格式");
    info!("   - CSV: 电子表格兼容格式");
    info!("   - 编程对象: Rust结构体等");
    info!("");

    // 2. 商品数据提取
    info!("2️⃣  商品数据提取示例");
    info!("-----------------------------");

    let product = Product {
        name: "Wireless Bluetooth Headphones".to_string(),
        price: 79.99,
        currency: "USD".to_string(),
        description: "High-quality wireless headphones with noise cancellation".to_string(),
        rating: Some(4.5),
        in_stock: true,
        category: "Electronics".to_string(),
    };

    info!("📦 提取的商品数据:");
    info!("   名称: {}", product.name);
    info!("   价格: {} {}", product.currency, product.price);
    info!("   描述: {}", product.description);
    info!("   评分: {:?}", product.rating);
    info!(
        "   库存: {}",
        if product.in_stock { "有货" } else { "缺货" }
    );
    info!("   分类: {}", product.category);
    info!("");

    // JSON输出
    info!("📄 JSON格式输出:");
    let json = serde_json::to_string_pretty(&product).unwrap();
    for line in json.lines().take(10) {
        info!("   {}", line);
    }
    info!("   ...");
    info!("");

    // 3. 文章数据提取
    info!("3️⃣  文章数据提取示例");
    info!("-----------------------------");

    let article = Article {
        title: "Getting Started with Rust".to_string(),
        author: "John Doe".to_string(),
        publish_date: "2024-01-15".to_string(),
        content: "Rust is a systems programming language...".to_string(),
        tags: vec![
            "Rust".to_string(),
            "Programming".to_string(),
            "Tutorial".to_string(),
        ],
        read_time: 5,
    };

    info!("📰 提取的文章数据:");
    info!("   标题: {}", article.title);
    info!("   作者: {}", article.author);
    info!("   发布日期: {}", article.publish_date);
    info!("   标签: {:?}", article.tags);
    info!("   预计阅读时间: {} 分钟", article.read_time);
    info!("");

    // 4. 批量数据处理
    info!("4️⃣  批量数据处理");
    info!("-----------------------------");

    let products = vec![
        Product {
            name: "Product 1".to_string(),
            price: 10.0,
            currency: "USD".to_string(),
            description: "Description 1".to_string(),
            rating: Some(4.0),
            in_stock: true,
            category: "Electronics".to_string(),
        },
        Product {
            name: "Product 2".to_string(),
            price: 20.0,
            currency: "USD".to_string(),
            description: "Description 2".to_string(),
            rating: Some(3.5),
            in_stock: false,
            category: "Books".to_string(),
        },
        Product {
            name: "Product 3".to_string(),
            price: 30.0,
            currency: "USD".to_string(),
            description: "Description 3".to_string(),
            rating: Some(5.0),
            in_stock: true,
            category: "Clothing".to_string(),
        },
    ];

    info!("📊 批量数据统计:");
    info!("   总商品数: {}", products.len());
    let total_price: f64 = products.iter().map(|p| p.price).sum();
    info!("   总价格: ${:.2}", total_price);
    let avg_price = total_price / products.len() as f64;
    info!("   平均价格: ${:.2}", avg_price);
    let in_stock_count = products.iter().filter(|p| p.in_stock).count();
    info!("   有货商品: {}/{}", in_stock_count, products.len());
    info!("");

    // CSV输出
    info!("📄 CSV格式输出:");
    info!("   name,price,currency,category");
    for p in &products {
        info!("   {},{},{},{}", p.name, p.price, p.currency, p.category);
    }
    info!("");

    // 5. 数据验证
    info!("5️⃣  数据验证");
    info!("-----------------------------");
    info!("");
    info!("✅ 验证检查项:");
    info!("   - 必填字段是否为空");
    info!("   - 价格是否为正数");
    info!("   - 日期格式是否正确");
    info!("   - 数值范围是否合理");
    info!("   - 数据类型是否匹配");
    info!("");

    // 模拟验证
    info!("🔍 验证结果:");
    for (i, p) in products.iter().enumerate() {
        let mut errors = Vec::new();

        if p.name.is_empty() {
            errors.push("名称为空");
        }
        if p.price <= 0.0 {
            errors.push("价格无效");
        }
        if !p.currency.is_empty() && p.currency.len() != 3 {
            errors.push("货币代码格式错误");
        }

        if errors.is_empty() {
            info!("   商品{}: ✅ 验证通过", i + 1);
        } else {
            info!("   商品{}: ❌ {}", i + 1, errors.join(", "));
        }
    }

    info!("\n=====================================");
    info!("✨ 结构化数据提取示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 使用 serde_json 进行JSON序列化");
    info!("   - 使用 serde_csv 进行CSV转换");
    info!("   - 添加数据验证确保数据质量");
    info!("   - 批量处理时注意内存使用");
}
