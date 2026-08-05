// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Geo restriction repository integration tests
//!
//! Covers DatabaseGeoRestrictionRepository (PostgreSQL-backed) implementation,
//! including get/update team_restrictions, TeamNotFound error path,
//! and log_geo_restriction_action persistence.

use super::super::helpers::create_test_app_no_worker;
use crawlrs::domain::repositories::geo_restriction_repository::{
    GeoRestrictionRepository, GeoRestrictionRepositoryError,
};
use crawlrs::domain::services::team_service::TeamGeoRestrictions;
use crawlrs::infrastructure::database::entities::geo_restriction_log;
use crawlrs::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

// ==================== DatabaseGeoRestrictionRepository 测试 ====================

/// tc_database_get_unknown_team_returns_team_not_found: 未知 team_id 返回 TeamNotFound
#[tokio::test]
async fn tc_database_get_unknown_team_returns_team_not_found() {
    let app = create_test_app_no_worker().await;
    let repo = DatabaseGeoRestrictionRepository::new(app.db_pool.clone());

    let unknown_team = Uuid::new_v4();
    let result = repo.get_team_restrictions(unknown_team).await;

    match result {
        Err(GeoRestrictionRepositoryError::TeamNotFound(id)) => {
            assert_eq!(
                id, unknown_team,
                "TeamNotFound error should carry the queried team_id"
            );
        }
        Err(e) => panic!("expected TeamNotFound error, got different error: {:?}", e),
        Ok(_) => panic!("expected TeamNotFound error, got Ok"),
    }
}

/// tc_database_update_unknown_team_returns_team_not_found: 未知 team_id 更新返回 TeamNotFound
#[tokio::test]
async fn tc_database_update_unknown_team_returns_team_not_found() {
    let app = create_test_app_no_worker().await;
    let repo = DatabaseGeoRestrictionRepository::new(app.db_pool.clone());

    let unknown_team = Uuid::new_v4();
    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string()]),
        blocked_countries: None,
        ip_whitelist: None,
        domain_blacklist: None,
    };

    let result = repo
        .update_team_restrictions(unknown_team, &restrictions)
        .await;

    match result {
        Err(GeoRestrictionRepositoryError::TeamNotFound(id)) => {
            assert_eq!(id, unknown_team);
        }
        Err(e) => panic!("expected TeamNotFound error, got different error: {:?}", e),
        Ok(_) => panic!("expected TeamNotFound error, got Ok"),
    }
}

/// tc_database_update_then_get_roundtrip: 创建团队 → 更新限制 → 读取验证
#[tokio::test]
async fn tc_database_update_then_get_roundtrip() {
    let app = create_test_app_no_worker().await;
    let repo = DatabaseGeoRestrictionRepository::new(app.db_pool.clone());
    let team_id = app.team_id;

    // 初始读取：团队存在，geo 限制应为默认值（disabled）
    let initial = repo
        .get_team_restrictions(team_id)
        .await
        .expect("initial get should succeed");
    assert!(
        !initial.enable_geo_restrictions,
        "team should start with geo restrictions disabled"
    );

    // 更新限制
    let new_restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string(), "CA".to_string()]),
        blocked_countries: Some(vec!["CN".to_string()]),
        ip_whitelist: Some(vec!["127.0.0.1".to_string(), "10.0.0.0/8".to_string()]),
        domain_blacklist: Some(vec!["malicious.example".to_string()]),
    };
    repo.update_team_restrictions(team_id, &new_restrictions)
        .await
        .expect("update should succeed");

    // 读取验证
    let updated = repo
        .get_team_restrictions(team_id)
        .await
        .expect("get after update should succeed");

    assert!(
        updated.enable_geo_restrictions,
        "geo restrictions should be enabled after update"
    );
    assert_eq!(
        updated.allowed_countries.as_ref().unwrap().len(),
        2,
        "allowed_countries should have 2 entries"
    );
    assert_eq!(
        updated.blocked_countries.as_ref().unwrap()[0],
        "CN".to_string()
    );
    assert_eq!(
        updated.ip_whitelist.as_ref().unwrap().len(),
        2,
        "ip_whitelist should have 2 entries"
    );
    assert_eq!(
        updated.domain_blacklist.as_ref().unwrap()[0],
        "malicious.example".to_string()
    );

    // 清理：恢复默认值，避免影响其他测试
    let reset = TeamGeoRestrictions::default();
    repo.update_team_restrictions(team_id, &reset)
        .await
        .expect("reset should succeed");
}

/// tc_database_log_geo_restriction_action_persists: 记录审计日志应写入数据库
#[tokio::test]
async fn tc_database_log_geo_restriction_action_persists() {
    let app = create_test_app_no_worker().await;
    let repo = DatabaseGeoRestrictionRepository::new(app.db_pool.clone());
    let team_id = app.team_id;

    repo.log_geo_restriction_action(
        team_id,
        "203.0.113.50",
        "US",
        "allowed",
        "IP matches whitelist",
    )
    .await
    .expect("log action should succeed");

    // 验证日志已写入数据库
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session for verification");
    let conn = session
        .connection()
        .expect("Failed to get connection for verification");

    let logs = geo_restriction_log::Entity::find()
        .filter(geo_restriction_log::Column::TeamId.eq(team_id))
        .filter(geo_restriction_log::Column::IpAddress.eq("203.0.113.50"))
        .all(conn)
        .await
        .expect("Failed to query logs");

    assert!(
        !logs.is_empty(),
        "at least one log entry should be persisted"
    );
    let log = &logs[0];
    assert_eq!(log.team_id, team_id);
    assert_eq!(log.ip_address, "203.0.113.50");
    assert_eq!(log.country_code.as_ref().unwrap(), "US");
    assert_eq!(log.restriction_type, "allowed");
    assert_eq!(log.reason, "IP matches whitelist");

    // 清理
    let _ = geo_restriction_log::Entity::delete_many()
        .filter(geo_restriction_log::Column::TeamId.eq(team_id))
        .exec(conn)
        .await;
}

/// tc_database_log_multiple_actions_all_persist: 多次记录应都写入数据库
#[tokio::test]
async fn tc_database_log_multiple_actions_all_persist() {
    let app = create_test_app_no_worker().await;
    let repo = DatabaseGeoRestrictionRepository::new(app.db_pool.clone());
    let team_id = app.team_id;

    // 记录 3 条不同 IP 的日志
    let entries = vec![
        ("192.168.1.1", "US", "allowed", "in whitelist"),
        ("10.0.0.5", "CN", "denied", "in blocklist"),
        ("172.16.0.1", "RU", "denied", "geo-blocked"),
    ];

    for (ip, country, action, reason) in &entries {
        repo.log_geo_restriction_action(team_id, ip, country, action, reason)
            .await
            .expect("log action should succeed");
    }

    // 验证所有日志都已写入
    let session = app
        .db_pool
        .get_session("admin")
        .await
        .expect("Failed to get session");
    let conn = session.connection().expect("Failed to get connection");

    let logs = geo_restriction_log::Entity::find()
        .filter(geo_restriction_log::Column::TeamId.eq(team_id))
        .all(conn)
        .await
        .expect("Failed to query logs");

    assert!(
        logs.len() >= 3,
        "should have at least 3 log entries, got {}",
        logs.len()
    );

    // 验证每条 entry 都在数据库中
    for (ip, country, action, reason) in &entries {
        let found = logs.iter().any(|l| {
            l.ip_address == *ip
                && l.country_code.as_deref() == Some(*country)
                && l.restriction_type == *action
                && l.reason == *reason
        });
        assert!(found, "log entry for ip={} should be persisted", ip);
    }

    // 清理
    let _ = geo_restriction_log::Entity::delete_many()
        .filter(geo_restriction_log::Column::TeamId.eq(team_id))
        .exec(conn)
        .await;
}

/// tc_database_update_partial_restrictions: 只设置部分字段，其他字段应为 None
#[tokio::test]
async fn tc_database_update_partial_restrictions() {
    let app = create_test_app_no_worker().await;
    let repo = DatabaseGeoRestrictionRepository::new(app.db_pool.clone());
    let team_id = app.team_id;

    // 只设置 enable + ip_whitelist，其他为 None
    let partial = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: None,
        blocked_countries: None,
        ip_whitelist: Some(vec!["127.0.0.1".to_string()]),
        domain_blacklist: None,
    };
    repo.update_team_restrictions(team_id, &partial)
        .await
        .expect("partial update should succeed");

    let retrieved = repo
        .get_team_restrictions(team_id)
        .await
        .expect("get after partial update should succeed");

    assert!(retrieved.enable_geo_restrictions, "geo should be enabled");
    assert!(
        retrieved.allowed_countries.is_none(),
        "allowed_countries should be None"
    );
    assert!(
        retrieved.blocked_countries.is_none(),
        "blocked_countries should be None"
    );
    assert_eq!(
        retrieved.ip_whitelist.as_ref().unwrap()[0],
        "127.0.0.1".to_string(),
        "ip_whitelist should be set"
    );
    assert!(
        retrieved.domain_blacklist.is_none(),
        "domain_blacklist should be None"
    );

    // 清理
    let reset = TeamGeoRestrictions::default();
    repo.update_team_restrictions(team_id, &reset)
        .await
        .expect("reset should succeed");
}
