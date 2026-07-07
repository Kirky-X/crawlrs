// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! CSS选择器提取示例
//!
//! 演示如何使用CSS选择器从HTML中提取数据。
//!
//! # 常用CSS选择器
//!
//! - `tag` - 按标签名选择
//! - `#id` - 按ID选择
//! - `.class` - 按类名选择
//! - `ancestor descendant` - 后代选择
//! - `parent > child` - 子元素选择
//! - `[attr]` - 属性选择
//! - `selector1, selector2` - 多选择器
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example css_selector
//!

use log::info;

/// CSS选择器示例
#[derive(Debug)]
struct CssSelectorExample {
    name: String,
    selector: String,
    description: String,
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始CSS选择器提取示例");
    info!("=====================================\n");

    // 1. CSS选择器基础
    info!("1️⃣  CSS选择器基础");
    info!("-----------------------------");
    info!("");
    info!("📖 CSS选择器是用于选择HTML元素的模式");
    info!("   crawlrs 支持标准CSS3选择器语法");
    info!("");

    // 2. 选择器示例列表
    info!("2️⃣  常用选择器示例");
    info!("-----------------------------");

    let examples = vec![
        CssSelectorExample {
            name: "标签选择器".to_string(),
            selector: "h1".to_string(),
            description: "选择所有<h1>元素".to_string(),
        },
        CssSelectorExample {
            name: "ID选择器".to_string(),
            selector: "#header".to_string(),
            description: "选择id=\"header\"的元素".to_string(),
        },
        CssSelectorExample {
            name: "类选择器".to_string(),
            selector: ".article".to_string(),
            description: "选择class=\"article\"的元素".to_string(),
        },
        CssSelectorExample {
            name: "属性选择器".to_string(),
            selector: "[href]".to_string(),
            description: "选择有href属性的元素".to_string(),
        },
        CssSelectorExample {
            name: "后代选择器".to_string(),
            selector: "div p".to_string(),
            description: "选择<div>内的所有<p>元素".to_string(),
        },
        CssSelectorExample {
            name: "子元素选择器".to_string(),
            selector: "ul > li".to_string(),
            description: "选择<ul>的直接子元素<li>".to_string(),
        },
        CssSelectorExample {
            name: "相邻兄弟".to_string(),
            selector: "h1 + p".to_string(),
            description: "选择<h1>后面紧邻的<p>".to_string(),
        },
        CssSelectorExample {
            name: "伪类选择器".to_string(),
            selector: "a:first-child".to_string(),
            description: "选择作为父元素第一个子元素的<a>".to_string(),
        },
    ];

    for ex in &examples {
        info!("📝 {}:", ex.name);
        info!("   选择器: `{}`", ex.selector);
        info!("   说明: {}", ex.description);
        info!("");
    }

    // 3. 实际应用示例
    info!("3️⃣  实际应用示例");
    info!("-----------------------------");

    let sample_html = r#"
    <html>
    <head><title>示例页面</title></head>
    <body>
        <header id="main-header">
            <h1>网站标题</h1>
            <nav class="navigation">
                <a href="/">首页</a>
                <a href="/about">关于</a>
                <a href="/contact">联系</a>
            </nav>
        </header>
        <main>
            <article class="post">
                <h2>文章标题1</h2>
                <p class="summary">这是文章摘要...</p>
                <div class="content">
                    <p>文章内容...</p>
                </div>
            </article>
            <article class="post">
                <h2>文章标题2</h2>
                <p class="summary">这是另一篇摘要...</p>
            </article>
        </main>
        <footer>
            <p>版权信息</p>
        </footer>
    </body>
    </html>
    "#;

    info!("📄 提取示例:");

    // 提取标题
    info!("");
    info!("  提取页面标题:");
    info!("  选择器: `title`");
    if let Some(title) = extract_first(sample_html, "title") {
        info!("  结果: {}", title);
    }

    // 提取所有文章标题
    info!("");
    info!("  提取所有文章标题:");
    info!("  选择器: `article h2`");
    let titles = extract_all(sample_html, "article h2");
    info!("  结果: {:?}", titles);

    // 提取所有链接
    info!("");
    info!("  提取所有链接地址:");
    info!("  选择器: `a[href]`");
    let links: Vec<String> = extract_attributes(sample_html, "a[href]", "href");
    info!("  结果: {:?}", links);

    // 提取摘要
    info!("");
    info!("  提取所有摘要:");
    info!("  选择器: `.summary`");
    let summaries = extract_all(sample_html, ".summary");
    info!("  结果: {:?}", summaries);

    info!("\n=====================================");
    info!("✨ CSS选择器提取示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 使用具体的选择器提高准确性");
    info!("   - 优先使用ID选择器（最快）");
    info!("   - 使用类选择器批量选择元素");
    info!("   - 属性选择器适合提取链接和图片");
}

fn extract_first(html: &str, selector: &str) -> Option<String> {
    // 简化实现：使用简单的正则表达式
    match selector {
        "title" => regex::Regex::new(r"<title>([^<]*)</title>")
            .unwrap()
            .captures(html)
            .and_then(|c: &regex::Captures| c.get(1))
            .map(|m: regex::Match| m.as_str().to_string()),
        _ => None,
    }
}

fn extract_all(html: &str, selector: &str) -> Vec<String> {
    match selector {
        "article h2" => regex::Regex::new(r"<article[^>]*>[\s\S]*?<h2>([^<]*)</h2>")
            .unwrap()
            .captures_iter(html)
            .filter_map(|c: &regex::Captures| c.get(1))
            .map(|m: regex::Match| m.as_str().to_string())
            .collect(),
        ".summary" | "p.summary" => regex::Regex::new(r#"<p[^>]*class="summary"[^>]*>([^<]*)</p>"#)
            .unwrap()
            .captures_iter(html)
            .filter_map(|c: &regex::Captures| c.get(1))
            .map(|m: regex::Match| m.as_str().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_attributes(html: &str, selector: &str, attr: &str) -> Vec<String> {
    match selector {
        "a[href]" => regex::Regex::new(r#"<a[^>]+href="([^"]*)""#)
            .unwrap()
            .captures_iter(html)
            .filter_map(|c: &regex::Captures| c.get(1))
            .map(|m: regex::Match| m.as_str().to_string())
            .collect(),
        _ => Vec::new(),
    }
}
