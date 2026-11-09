// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! teams 能力装配：地理定位服务 + 团队服务（teams-off 时不编译）。

use std::sync::Arc;

use crate::bootstrap::infrastructure::Repositories;
use crate::bootstrap::services::geolocation::GeoLocationServiceImpl;
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::team_service::TeamService;

/// 装配 teams 相关服务：地理定位服务 + 团队服务（teams-off 时不编译）。
pub(super) fn init_team_services(
    repositories: &Repositories,
    http_client: &Arc<reqwest::Client>,
) -> (Arc<TeamService>, Arc<dyn GeoLocationService>) {
    // teams 相关服务在 teams-off 时不初始化
    let geo_location_service = Arc::new(GeoLocationServiceImpl::new(http_client.clone()));
    let team_service = Arc::new(TeamService::new(
        geo_location_service.clone(),
        repositories.geo_restriction_repo.clone(),
    ));
    (team_service, geo_location_service)
}
