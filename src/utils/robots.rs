// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::Result;
use reqwest::Client;
use robotstxt::DefaultMatcher;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;

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

    /// 检查URL是否被允许访问
    ///
    /// # 参数
    ///
    /// * `url_str` - 要检查的URL字符串
    /// * `user_agent` - 用户代理字符串
    ///
    /// # 返回值
    ///
    /// * `Ok(true)` - URL被允许访问
    /// * `Ok(false)` - URL不被允许访问
    /// * `Err(anyhow::Error)` - 检查过程中发生错误
    pub async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool> {
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
                    let mut matcher = DefaultMatcher::default();
                    return Ok(matcher.one_agent_allowed_by_robots(
                        user_agent,
                        url.path(),
                        &cached.content,
                    ));
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

        // Parse and check
        let mut matcher = DefaultMatcher::default();
        let allowed = matcher.one_agent_allowed_by_robots(user_agent, url.path(), &content);

        // Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(
                robots_url,
                CachedRobots {
                    content,
                    expires_at: Instant::now() + Duration::from_secs(3600), // Cache for 1 hour
                },
            );
        }

        Ok(allowed)
    }
}
