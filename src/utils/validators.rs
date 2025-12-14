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

use std::net::IpAddr;
use thiserror::Error;
use url::Url;

/// 验证错误类型
#[derive(Error, Debug)]
pub enum ValidationError {
    /// URL无效
    #[error("Invalid URL")]
    InvalidUrl,
    /// 检测到SSRF攻击
    #[error("SSRF detected")]
    SsrfDetected,
}

/// 检查IP地址是否安全
///
/// # 参数
///
/// * `ip` - IP地址
///
/// # 返回值
///
/// 如果IP地址是安全的则返回true，否则返回false
pub fn is_safe_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            !ipv4.is_loopback()
                && !ipv4.is_private()
                && !ipv4.is_link_local()
                && !ipv4.is_broadcast()
                && !ipv4.is_documentation()
        }
        IpAddr::V6(ipv6) => !ipv6.is_loopback() && !ipv6.is_unspecified(),
    }
}

/// 验证URL
///
/// # 参数
///
/// * `url` - URL字符串
///
/// # 返回值
///
/// * `Ok(())` - URL有效
/// * `Err(ValidationError)` - URL无效或存在安全风险
pub async fn validate_url(url: &str) -> Result<(), ValidationError> {
    let parsed = Url::parse(url).map_err(|_| ValidationError::InvalidUrl)?;

    // Check scheme
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(ValidationError::InvalidUrl);
    }

    // Resolve domain to IP
    let host = parsed.host_str().ok_or(ValidationError::InvalidUrl)?;
    let addrs = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|_| ValidationError::InvalidUrl)?
        .collect::<Vec<_>>();

    // Check all resolved IPs
    for addr in addrs {
        if !is_safe_ip(addr.ip()) {
            return Err(ValidationError::SsrfDetected);
        }
    }

    Ok(())
}
