// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use reqwest::Client;
use robotstxt::DefaultMatcher;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;

use async_trait::async_trait;

/// Robots.txt检查器接口
#[async_trait]
pub trait RobotsCheckerTrait: Send + Sync {
    /// 检查URL是否被允许访问
    async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool>;
    /// 获取爬取延迟
    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>>;
}

/// 缓存的Robots.txt内容
#[derive(Clone)]
struct CachedRobots {
    /// 内容
    content: String,

    /// 过期时间
    expires_at: Instant,
}

/// Robots.txt检查器
#[derive(Clone)]
pub struct RobotsChecker {
    /// HTTP客户端
    client: Client,

    /// 缓存
    cache: Arc<Mutex<HashMap<String, CachedRobots>>>,
}

#[async_trait]
impl RobotsCheckerTrait for RobotsChecker {
    async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool> {
        let content = self.get_robots_content(url_str).await?;
        let url = Url::parse(url_str)?;
        let mut matcher = DefaultMatcher::default();
        Ok(matcher.one_agent_allowed_by_robots(user_agent, url.path(), &content))
    }

    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>> {
        let content = self.get_robots_content(url_str).await?;
        Ok(self.parse_crawl_delay(&content, user_agent))
    }
}

impl Default for RobotsChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl RobotsChecker {
    /// 创建新的Robots检查器实例
    ///
    /// # 返回值
    ///
    /// 返回新的Robots检查器实例
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 获取Robots.txt内容（带缓存）
    async fn get_robots_content(&self, url_str: &str) -> Result<String> {
        let url = Url::parse(url_str)?;
        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid URL"))?;
        let scheme = url.scheme();
        let port = url.port_or_known_default().unwrap_or(80);

        let robots_url = format!("{}://{}:{}/robots.txt", scheme, host, port);

        // Check cache
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(&robots_url) {
                if cached.expires_at > Instant::now() {
                    return Ok(cached.content.clone());
                } else {
                    cache.remove(&robots_url);
                }
            }
        }

        // Fetch robots.txt
        let response = self
            .client
            .get(&robots_url)
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        let content = match response {
            Ok(resp) if resp.status().is_success() => resp.text().await.unwrap_or_default(),
            _ => "".to_string(), // Empty means allow all
        };

        // Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(
                robots_url,
                CachedRobots {
                    content: content.clone(),
                    expires_at: Instant::now() + Duration::from_secs(3600), // Cache for 1 hour
                },
            );
        }

        Ok(content)
    }

    /// 解析Crawl-delay指令
    fn parse_crawl_delay(&self, content: &str, user_agent: &str) -> Option<Duration> {
        // 简单的解析逻辑：查找适用于该 User-Agent 的 Crawl-delay
        // 注意：这是一个简化的实现，不完全符合 RFC 规范，但足以处理大多数情况
        // 逻辑：
        // 1. 找到匹配的 User-agent 块
        // 2. 在块内查找 Crawl-delay

        let mut current_agent_matched = false;
        let mut delay: Option<f64> = None;
        let mut specific_agent_found = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let lower_line = line.to_lowercase();
            if lower_line.starts_with("user-agent:") {
                let agent = line[11..].trim();
                if agent == "*" {
                    current_agent_matched = !specific_agent_found;
                } else if user_agent.to_lowercase().contains(&agent.to_lowercase()) {
                    current_agent_matched = true;
                    specific_agent_found = true;
                    // Reset delay if we found a more specific agent
                    delay = None;
                } else {
                    current_agent_matched = false;
                }
            } else if lower_line.starts_with("crawl-delay:") && current_agent_matched {
                if let Ok(d) = line[12..].trim().parse::<f64>() {
                    delay = Some(d);
                }
            }
        }

        delay.map(Duration::from_secs_f64)
    }

    /// 旧的公开方法，为了兼容性保留
    pub async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool> {
        RobotsCheckerTrait::is_allowed(self, url_str, user_agent).await
    }
}
