// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::services::geo_location::{is_ip_in_cidr, GeoLocationService};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

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
        team_id: Uuid,
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

        if let Some(ref whitelist) = restrictions.ip_whitelist {
            for cidr in whitelist {
                if is_ip_in_cidr(&ip, cidr) {
                    log::info!(
                        "IP {} allowed by whitelist (CIDR: {}) for team {}",
                        ip_address,
                        cidr,
                        team_id
                    );
                    return Ok(GeoRestrictionResult::Allowed);
                }
            }
        }

        let country_code = self.get_country_code(team_id, ip_address, &ip).await?;

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
                return Ok(GeoRestrictionResult::Denied(format!(
                    "Access from country {} is not allowed",
                    country_code
                )));
            }
        }

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
                return Ok(GeoRestrictionResult::Denied(format!(
                    "Access from country {} is not allowed",
                    country_code
                )));
            }
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
        team_id: Uuid,
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
        if !restrictions.enable_geo_restrictions {
            return Ok(GeoRestrictionResult::Allowed);
        }

        // Normalize input: extract host from URL/domain:port/path forms.
        // This prevents substring false positives (e.g., "evil-example.com" matching "example.com")
        // while still supporting URL inputs gracefully.
        let normalized = extract_host(domain);

        if let Some(ref blacklist) = restrictions.domain_blacklist {
            for blocked_domain in blacklist {
                // Block on exact match or proper subdomain match (suffix).
                // Substring matching is intentionally avoided to prevent security bypass
                // where "evil-example.com" would be blocked by "example.com" blacklist,
                // and to prevent false positives where "example.com" blacklist blocks "evil-example.com".
                if normalized == blocked_domain.as_str()
                    || normalized.ends_with(&format!(".{}", blocked_domain))
                {
                    return Ok(GeoRestrictionResult::Denied(format!(
                        "Domain {} is in the blacklist",
                        domain
                    )));
                }
            }
        }

        Ok(GeoRestrictionResult::Allowed)
    }

    pub async fn get_team_geo_restrictions(&self, team_id: Uuid) -> TeamGeoRestrictions {
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
                log::warn!(
                    "Failed to get geo restrictions for team {}: {}. Using default configuration.",
                    team_id,
                    e
                );
                TeamGeoRestrictions::default()
            }
        }
    }
}

/// Extract the host (domain) from a URL, domain:port, or bare domain string.
///
/// Handles the following input forms:
/// - `scheme://host/path?query#fragment` → `host`
/// - `host:port` → `host`
/// - `host/path` → `host`
/// - `host` → `host` (unchanged)
///
/// This is used by [`TeamService::validate_domain_blacklist`] to normalize inputs
/// before matching against the blacklist, preventing substring false positives
/// (e.g., `evil-example.com/path` must not match `example.com` blacklist entry).
fn extract_host(input: &str) -> &str {
    // Strip scheme if present (e.g., "https://host/path" → "host/path")
    let after_scheme = input.split("://").last().unwrap_or(input);
    // Strip path/query/fragment if present (e.g., "host/path" → "host")
    let host_with_port = after_scheme.split('/').next().unwrap_or(after_scheme);

    // Handle IPv6 in brackets: "[::1]:8080" → "[::1]"
    // IPv6 addresses contain colons, so the simple `split(':')` would break them.
    if host_with_port.starts_with('[') {
        if let Some(end) = host_with_port.find(']') {
            // Include both brackets: "[::1]"
            return &host_with_port[..=end];
        }
    }

    // Strip port if present (e.g., "host:8080" → "host")
    host_with_port.split(':').next().unwrap_or(host_with_port)
}

#[cfg(test)]
mod extract_host_tests {
    use super::extract_host;

    #[test]
    fn test_bare_domain() {
        assert_eq!(extract_host("example.com"), "example.com");
    }

    #[test]
    fn test_subdomain() {
        assert_eq!(extract_host("sub.example.com"), "sub.example.com");
    }

    #[test]
    fn test_url_with_scheme_and_path() {
        assert_eq!(
            extract_host("https://www.example.com/path"),
            "www.example.com"
        );
    }

    #[test]
    fn test_url_with_port() {
        assert_eq!(extract_host("example.com:8080"), "example.com");
    }

    #[test]
    fn test_url_with_scheme_port_path() {
        assert_eq!(
            extract_host("http://example.com:8080/path?q=1"),
            "example.com"
        );
    }

    #[test]
    fn test_host_with_path_only() {
        assert_eq!(extract_host("example.com/admin"), "example.com");
    }

    #[test]
    fn test_ipv4_address() {
        assert_eq!(extract_host("192.168.1.1"), "192.168.1.1");
    }

    #[test]
    fn test_ipv6_address_with_brackets() {
        assert_eq!(extract_host("[::1]:8080"), "[::1]");
    }
}

#[cfg(test)]
#[path = "team_service_test.rs"]
mod tests;
