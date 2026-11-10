// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::services::geo_location::{is_ip_in_cidr, GeoLocationService};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamGeoRestrictions {
    pub enable_geo_restrictions: bool,
    pub allowed_countries: Option<Vec<String>>,
    pub blocked_countries: Option<Vec<String>>,
    pub ip_whitelist: Option<Vec<String>>,
    pub domain_blacklist: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeoRestrictionResult {
    Allowed,
    Denied(String),
}

pub struct TeamService {
    geolocation_service: Arc<dyn GeoLocationService>,
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamService {
    pub fn new(
        geolocation_service: Arc<dyn GeoLocationService>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    ) -> Self {
        Self {
            geolocation_service,
            geo_restriction_repo,
        }
    }

    pub async fn validate_geographic_restriction(
        &self,
        team_id: crate::domain::id::Id,
        ip_address: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        if !restrictions.enable_geo_restrictions {
            return Ok(GeoRestrictionResult::Allowed);
        }

        let ip = match IpAddr::from_str(ip_address) {
            Ok(ip) => ip,
            Err(_) => {
                return Ok(GeoRestrictionResult::Denied(
                    "Invalid IP address format".to_string(),
                ))
            }
        };

        if let Some(outcome) = whitelist_outcome(restrictions, &ip, ip_address, team_id) {
            return Ok(outcome);
        }

        let country_code = self.get_country_code(team_id, ip_address, &ip).await?;

        if let Some(outcome) =
            country_list_outcome(restrictions, &country_code, ip_address, team_id)
        {
            return Ok(outcome);
        }

        log::info!(
            "IP {} from country {} allowed for team {}",
            ip_address,
            country_code,
            team_id
        );

        Ok(GeoRestrictionResult::Allowed)
    }

    async fn get_country_code(
        &self,
        team_id: crate::domain::id::Id,
        ip_address: &str,
        ip: &IpAddr,
    ) -> Result<String> {
        let location = self
            .geolocation_service
            .get_location(ip)
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to get geolocation for IP {} for team {}: {}",
                    ip_address,
                    team_id,
                    e
                );
                anyhow::anyhow!("Unable to determine geographic location")
            })?;
        Ok(location.country_code.to_uppercase())
    }

    pub fn validate_domain_blacklist(
        &self,
        domain: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        match domain_blacklist_denial(domain, restrictions) {
            Some(reason) => Ok(GeoRestrictionResult::Denied(reason)),
            None => Ok(GeoRestrictionResult::Allowed),
        }
    }

    pub async fn get_team_geo_restrictions(
        &self,
        team_id: crate::domain::id::Id,
    ) -> TeamGeoRestrictions {
        match self
            .geo_restriction_repo
            .get_team_restrictions(team_id)
            .await
        {
            Ok(restrictions) => {
                log::debug!(
                    "Retrieved geo restrictions for team {}: {:?}",
                    team_id,
                    restrictions
                );
                restrictions
            }
            Err(e) => {
                // fail-closed（CWE-1229）：仓储故障时不得回落"无限制"默认值全量放行，
                // 改为"启用限制 + 空 allowed 名单"的 deny-all 配置
                log::error!(
                    "Failed to get geo restrictions for team {}: {}. \
                     Falling back to deny-all configuration.",
                    team_id,
                    e
                );
                TeamGeoRestrictions {
                    enable_geo_restrictions: true,
                    allowed_countries: Some(Vec::new()),
                    blocked_countries: None,
                    ip_whitelist: None,
                    domain_blacklist: None,
                }
            }
        }
    }
}

/// IP 白名单（CIDR）判定：命中时返回 `Allowed`（日志与返回语义原样保留）。
fn whitelist_outcome(
    restrictions: &TeamGeoRestrictions,
    ip: &IpAddr,
    ip_address: &str,
    team_id: crate::domain::id::Id,
) -> Option<GeoRestrictionResult> {
    if let Some(ref whitelist) = restrictions.ip_whitelist {
        for cidr in whitelist {
            if is_ip_in_cidr(ip, cidr) {
                log::info!(
                    "IP {} allowed by whitelist (CIDR: {}) for team {}",
                    ip_address,
                    cidr,
                    team_id
                );
                return Some(GeoRestrictionResult::Allowed);
            }
        }
    }
    None
}

/// 国家名单判定。blocked 优先于 allowed（CWE-285）：同时出现在两个名单的国家必须拒绝，
/// 否则黑名单形同虚设。
fn country_list_outcome(
    restrictions: &TeamGeoRestrictions,
    country_code: &str,
    ip_address: &str,
    team_id: crate::domain::id::Id,
) -> Option<GeoRestrictionResult> {
    if let Some(ref blocked) = restrictions.blocked_countries {
        if blocked
            .iter()
            .any(|code| code.to_uppercase() == country_code)
        {
            log::warn!(
                "IP {} from country {} blocked for team {} (blocked countries: {:?})",
                ip_address,
                country_code,
                team_id,
                blocked
            );
            return Some(GeoRestrictionResult::Denied(format!(
                "Access from country {} is not allowed",
                country_code
            )));
        }
    }

    if let Some(ref allowed) = restrictions.allowed_countries {
        if !allowed
            .iter()
            .any(|code| code.to_uppercase() == country_code)
        {
            log::warn!(
                "IP {} from country {} not in allowed list for team {} (allowed countries: {:?})",
                ip_address,
                country_code,
                team_id,
                allowed
            );
            return Some(GeoRestrictionResult::Denied(format!(
                "Access from country {} is not allowed",
                country_code
            )));
        }
    }

    None
}

/// 域名黑名单硬上限：条目数与单条长度同时约束 CPU 与任务 payload 体积。
pub const MAX_DOMAIN_BLACKLIST_ENTRIES: usize = 1000;
pub const MAX_DOMAIN_BLACKLIST_ENTRY_LEN: usize = 253;

impl TeamGeoRestrictions {
    /// 执行期生效的黑名单：限制关闭时为空（避免 API 放行、worker 拒绝的口径分裂）。
    ///
    /// 超过硬上限时返回 `Err`（fail-closed），不得静默截断——否则尾部条目会
    /// 在派生任务/执行期被悄悄丢弃，形成管理员策略失效。
    pub fn effective_domain_blacklist(&self) -> Result<Vec<String>, String> {
        let list = self.domain_blacklist.as_deref().unwrap_or(&[]);
        // 硬上限先于启用开关校验：关闭状态下的超限配置同样拒绝，
        // 否则重新启用时才会暴露、且历史数据可绕过策略上限。
        if list.len() > MAX_DOMAIN_BLACKLIST_ENTRIES {
            return Err(format!(
                "domain_blacklist has {} entries; maximum is {}",
                list.len(),
                MAX_DOMAIN_BLACKLIST_ENTRIES
            ));
        }
        // 单趟完成长度/可归一化/通配符校验（同一条目只 normalize 一次）。
        for entry in list {
            if entry.is_empty() || entry.len() > MAX_DOMAIN_BLACKLIST_ENTRY_LEN {
                return Err(format!(
                    "domain_blacklist entry must be 1-{} bytes: {}",
                    MAX_DOMAIN_BLACKLIST_ENTRY_LEN, entry
                ));
            }
            let normalized = normalize_host(entry);
            if normalized.is_empty() {
                return Err(format!(
                    "domain_blacklist entry cannot be normalized to a host: {entry}"
                ));
            }
            // 通配符条目不是合法 host，永远不会匹配任何 URL：拒绝而非静默失效。
            if normalized.contains('*') {
                return Err(format!(
                    "domain_blacklist wildcard entries are not supported: {entry}"
                ));
            }
        }
        if !self.enable_geo_restrictions {
            return Ok(Vec::new());
        }
        Ok(normalize_denylist_entries(list))
    }
}

/// 归一化黑名单条目：真实 host、小写、去末尾点、去空/去重（保持顺序）。
pub fn normalize_denylist_entries(list: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let normalized = normalize_host(entry);
        if normalized.is_empty() || out.contains(&normalized) {
            continue;
        }
        out.push(normalized);
    }
    out
}

/// 纯比较：`host` 与已归一化条目按“相等或子域”匹配。零分配。
pub fn host_in_denylist(host: &str, normalized_entries: &[String]) -> bool {
    normalized_entries.iter().any(|entry| {
        host == entry
            || (host.len() > entry.len() + 1
                && host.ends_with(entry.as_str())
                && host.as_bytes()[host.len() - entry.len() - 1] == b'.')
    })
}

/// 与 [`domain_in_denylist`] 相同，但条目已归一化（worker/search 热路径复用）。
pub fn domain_in_normalized_denylist(
    domain_or_url: &str,
    normalized_entries: &[String],
) -> Option<String> {
    if normalized_entries.is_empty() {
        return None;
    }
    let host = normalize_host(domain_or_url);
    if host_in_denylist(&host, normalized_entries) {
        // 拒绝原因会进入任务失败原因/API/webhook：只写归一化后的 host，
        // 不带原始 URL 的 path/query/userinfo，避免敏感值外泄。
        Some(format!("Domain {host} is in the blacklist"))
    } else {
        None
    }
}

/// 团队域名黑名单匹配（`TeamService::validate_domain_blacklist` 与
/// SDK/搜索等无 `TeamService` 句柄的入口共用同一份规则）。
///
/// 返回 `Some(reason)` 表示命中黑名单；`None` 表示放行。
/// 子域名按后缀匹配，不做子串匹配（避免 `evil-example.com` 受 `example.com` 规则误伤）。
pub fn domain_blacklist_denial(domain: &str, restrictions: &TeamGeoRestrictions) -> Option<String> {
    if !restrictions.enable_geo_restrictions {
        return None;
    }
    let blacklist = restrictions.domain_blacklist.as_deref().unwrap_or(&[]);
    domain_in_denylist(domain, blacklist)
}

/// 在给定黑名单中匹配主机（URL 或裸域名）。
///
/// 用 `url::Url` 解析真实 host，并统一小写、去掉末尾点；解析失败时回退到
/// 逐段剥离 scheme/path/query/fragment/userinfo。不得用 `split("://")` 之类
/// 的字符串切分做安全判定：`https://blocked.com/?next=https://safe.com`、
/// `https://good.com@blocked.com/`、大小写与末尾点都会让切分结果偏离真实主机。
pub fn domain_in_denylist(domain_or_url: &str, blacklist: &[String]) -> Option<String> {
    let normalized = normalize_denylist_entries(blacklist);
    domain_in_normalized_denylist(domain_or_url, &normalized)
}

/// 归一化主机：小写、去末尾点、去 scheme/path/query/fragment/userinfo/port。
pub(crate) fn normalize_host(input: &str) -> String {
    let candidate = input.trim();
    if let Ok(parsed) = url::Url::parse(candidate) {
        if let Some(host) = parsed.host_str() {
            return host.trim_end_matches('.').to_ascii_lowercase();
        }
    }
    // 裸域名/无 scheme 形态：先补 scheme 交给 Url 归一（IDN → punycode，
    // 与 seed 的 host_str 同口径），失败再逐段剥离。
    if let Ok(parsed) = url::Url::parse(&format!("http://{candidate}")) {
        if let Some(host) = parsed.host_str() {
            return host.trim_end_matches('.').to_ascii_lowercase();
        }
    }
    let after_scheme = candidate.split("://").last().unwrap_or(candidate);
    let host_with_port = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // userinfo 形式 good.com@blocked.com → 取 @ 之后的真实主机
    let host_with_port = host_with_port.rsplit('@').next().unwrap_or(host_with_port);
    let host = if host_with_port.starts_with('[') {
        host_with_port
    } else {
        host_with_port.split(':').next().unwrap_or(host_with_port)
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
#[path = "team_service_test.rs"]
mod tests;
