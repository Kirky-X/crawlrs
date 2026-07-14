// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! robots.txt合规示例
//!
//! 演示如何遵守robots.txt协议进行合规爬取。
//!
//! # robots.txt协议
//!
//! robots.txt是网站管理员创建的文本文件，告诉爬虫哪些页面可以抓取。
//! 遵守robots.txt是网络爬虫的基本职业道德。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example robots_compliance
//!

use log::info;

/// robots.txt解析结果
#[derive(Debug)]
struct RobotsTxt {
    #[allow(dead_code)]
    user_agent: String,
    allow: Vec<String>,
    disallow: Vec<String>,
    crawl_delay: Option<u64>,
}

impl RobotsTxt {
    fn new(user_agent: &str) -> Self {
        Self {
            user_agent: user_agent.to_string(),
            allow: Vec::new(),
            disallow: Vec::new(),
            crawl_delay: None,
        }
    }

    fn parse(&mut self, content: &str) {
        let mut current_section = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#") {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() < 2 {
                continue;
            }

            let key = parts[0].to_lowercase();
            let value = parts[1].trim().to_string();

            match key.as_str() {
                "user-agent" => {
                    current_section = Some(value);
                }
                "allow" => {
                    if current_section.is_none() || current_section.as_ref().unwrap() == "*" {
                        self.allow.push(value);
                    }
                }
                "disallow" => {
                    if current_section.is_none() || current_section.as_ref().unwrap() == "*" {
                        self.disallow.push(value);
                    }
                }
                "crawl-delay"
                    if current_section.is_none() || current_section.as_ref().unwrap() == "*" =>
                {
                    self.crawl_delay = value.parse().ok();
                }
                _ => {}
            }
        }
    }

    fn is_allowed(&self, url: &str) -> bool {
        // 检查是否被disallow
        for disallow in &self.disallow {
            if url.starts_with(disallow) {
                return false;
            }
        }

        // 检查是否被allow覆盖
        for allow in &self.allow {
            if url.starts_with(allow) {
                return true;
            }
        }

        !self.disallow.is_empty()
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始robots.txt合规示例");
    info!("=====================================\n");

    // 1. robots.txt协议介绍
    info!("1️⃣  robots.txt协议介绍");
    info!("-----------------------------");
    info!("");
    info!("📖 robots.txt是网站的标准爬虫协议文件");
    info!("   - 位于网站根目录: https://example.com/robots.txt");
    info!("   - 告诉爬虫哪些页面可以访问");
    info!("   - 是网站管理员控制爬虫行为的主要方式");
    info!("");
    info!("📋 常用指令:");
    info!("   User-agent: 指定适用的爬虫");
    info!("   Allow: 允许访问的路径");
    info!("   Disallow: 禁止访问的路径");
    info!("   Crawl-delay: 请求间隔（秒）");
    info!("   Sitemap: 网站地图位置");
    info!("");

    // 2. 示例robots.txt解析
    info!("2️⃣  解析示例robots.txt");
    info!("-----------------------------");

    let sample_robots = r#"
User-agent: *
Disallow: /admin/
Disallow: /private/
Disallow: /api/
Disallow: /login
Allow: /public/
Crawl-delay: 5

User-agent: Googlebot
Disallow: /cached/

Sitemap: https://example.com/sitemap.xml
"#;

    let mut robots = RobotsTxt::new("*");
    robots.parse(sample_robots);

    info!("📝 解析结果:");
    info!("  Disallow规则:");
    for d in &robots.disallow {
        info!("    - {}", d);
    }
    info!("  Allow规则:");
    for a in &robots.allow {
        info!("    - {}", a);
    }
    info!("  Crawl-delay: {}秒", robots.crawl_delay.unwrap_or(0));
    info!("");

    // 3. URL访问检查
    info!("3️⃣  URL访问检查");
    info!("-----------------------------");

    let test_urls = vec![
        "https://example.com/",
        "https://example.com/home",
        "https://example.com/admin/dashboard",
        "https://example.com/private/profile",
        "https://example.com/api/users",
        "https://example.com/login",
        "https://example.com/public/articles",
        "https://example.com/robots.txt",
    ];

    info!("📋 URL访问测试:");
    for url in &test_urls {
        let allowed = robots.is_allowed(url);
        let status = if allowed { "✅ 允许" } else { "❌ 禁止" };
        info!("  {} {}", status, url);
    }
    info!("");

    // 4. 合规爬取最佳实践
    info!("4️⃣  合规爬取最佳实践");
    info!("-----------------------------");
    info!("");
    info!("✅ 应该做的:");
    info!("   - 始终检查并遵守robots.txt");
    info!("   - 遵守Crawl-delay指令");
    info!("   - 设置有意义的User-Agent");
    info!("   - 只爬取允许的页面");
    info!("   - 尊重网站的带宽限制");
    info!("");
    info!("❌ 不应该做的:");
    info!("   - 忽略robots.txt规则");
    info!("   - 忽略Crawl-delay");
    info!("   - 爬取被明确禁止的页面");
    info!("   - 过于频繁的请求");
    info!("   - 绕过IP限制或User-Agent检测");
    info!("");

    // 5. 实际应用示例
    info!("5️⃣  实际应用示例");
    info!("-----------------------------");

    info!("📝 爬取策略配置:");
    info!("   let config = CrawlConfigDto {{");
    info!("       check_robots_txt: true,");
    info!("       obey_crawl_delay: true,");
    info!("       user_agent: \"MyBot/1.0\"");
    info!("       ...");
    info!("   }};");
    info!("");

    info!("📝 合规检查流程:");
    info!("   1. 访问 https://target.com/robots.txt");
    info!("   2. 解析并提取规则");
    info!("   3. 检查URL是否允许访问");
    info!("   4. 根据Crawl-delay设置请求间隔");
    info!("   5. 爬取允许的页面");
    info!("");

    info!("🔍 常见网站的robots.txt示例:");
    info!("   Google: https://www.google.com/robots.txt");
    info!("   Bing: https://www.bing.com/robots.txt");
    info!("   Wikipedia: https://en.wikipedia.org/robots.txt");

    info!("\n=====================================");
    info!("✨ robots.txt合规示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 遵守robots.txt是法律和道德要求");
    info!("   - 某些网站可能有额外的访问限制");
    info!("   - 建议在爬取前先检查robots.txt");
    info!("   - 使用有意义的User-Agent便于网站管理员联系");
}
