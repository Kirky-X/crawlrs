// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::infrastructure::geolocation::{is_ip_in_cidr, GeoLocationServiceTrait};
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
    geolocation_service: Arc<dyn GeoLocationServiceTrait>,
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamService {
    pub fn new(
        geolocation_service: Arc<dyn GeoLocationServiceTrait>,
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
                    tracing::info!(
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
                tracing::warn!(
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
                tracing::warn!(
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

        tracing::info!(
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
                tracing::error!(
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

        if let Some(ref blacklist) = restrictions.domain_blacklist {
            for blocked_domain in blacklist {
                if domain.contains(blocked_domain) {
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
                tracing::debug!(
                    "Retrieved geo restrictions for team {}: {:?}",
                    team_id,
                    restrictions
                );
                restrictions
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to get geo restrictions for team {}: {}. Using default configuration.",
                    team_id,
                    e
                );
                TeamGeoRestrictions::default()
            }
        }
    }
}

#[cfg(test)]
#[path = "team_service_test.rs"]
mod tests;
