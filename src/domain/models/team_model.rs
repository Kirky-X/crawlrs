// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Team domain model - pure domain entity without ORM annotations
//!
//! 该模块定义 Team 的纯领域模型，遵循 DDD 原则，
//! 不包含任何 ORM 注解。地理限制配置由 GeoRestrictionRepository 单独管理。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 团队领域模型
///
/// 表示系统中的租户实体，用于多租户隔离。
/// 地理限制配置（allowed_countries 等）不在此模型中，
/// 由 `TeamGeoRestrictions` 和 `GeoRestrictionRepository` 单独管理。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Team {
    /// 团队唯一标识
    pub id: Uuid,
    /// 团队名称
    pub name: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,
}

impl Team {
    /// 创建新的团队
    pub fn new(id: Uuid, name: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            created_at: now,
            updated_at: now,
        }
    }

    /// 创建带自定义时间戳的团队（供 mapper 使用）
    pub fn with_timestamps(
        id: Uuid,
        name: String,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            created_at,
            updated_at,
        }
    }

    /// 校验团队名称
    pub fn validate_name(&self) -> Result<(), TeamError> {
        if self.name.trim().is_empty() {
            return Err(TeamError::InvalidName(
                "Team name cannot be empty".to_string(),
            ));
        }
        if self.name.len() > 255 {
            return Err(TeamError::InvalidName(
                "Team name cannot exceed 255 characters".to_string(),
            ));
        }
        Ok(())
    }
}

/// 团队领域错误类型
#[derive(Debug, thiserror::Error)]
pub enum TeamError {
    /// 无效的团队名称
    #[error("Invalid team name: {0}")]
    InvalidName(String),

    /// 团队未找到
    #[error("Team not found: {0}")]
    NotFound(Uuid),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_new_sets_fields_and_timestamps() {
        let id = Uuid::new_v4();
        let before = Utc::now();
        let team = Team::new(id, "My Team".to_string());
        let after = Utc::now();

        assert_eq!(team.id, id);
        assert_eq!(team.name, "My Team");
        assert!(team.created_at >= before && team.created_at <= after);
        assert_eq!(team.created_at, team.updated_at);
    }

    #[test]
    fn test_team_with_timestamps_sets_all_fields() {
        let id = Uuid::new_v4();
        let created = Utc::now();
        let updated = created + chrono::Duration::seconds(10);

        let team = Team::with_timestamps(id, "Test".to_string(), created, updated);

        assert_eq!(team.id, id);
        assert_eq!(team.name, "Test");
        assert_eq!(team.created_at, created);
        assert_eq!(team.updated_at, updated);
    }

    #[test]
    fn test_validate_name_valid_passes() {
        let team = Team::new(Uuid::new_v4(), "Valid Team".to_string());
        assert!(team.validate_name().is_ok());
    }

    #[test]
    fn test_validate_name_empty_fails() {
        let team = Team::new(Uuid::new_v4(), String::new());
        let err = team.validate_name().expect_err("empty name should fail");
        match err {
            TeamError::InvalidName(msg) => assert!(msg.contains("empty")),
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_name_whitespace_only_fails() {
        let team = Team::new(Uuid::new_v4(), "   ".to_string());
        assert!(team.validate_name().is_err());
    }

    #[test]
    fn test_validate_name_too_long_fails() {
        let long_name = "a".repeat(256);
        let team = Team::new(Uuid::new_v4(), long_name);
        let err = team.validate_name().expect_err("long name should fail");
        match err {
            TeamError::InvalidName(msg) => assert!(msg.contains("255")),
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_name_max_length_passes() {
        let max_name = "a".repeat(255);
        let team = Team::new(Uuid::new_v4(), max_name);
        assert!(team.validate_name().is_ok());
    }

    #[test]
    fn test_team_clone_and_equality() {
        let team = Team::new(Uuid::new_v4(), "Test".to_string());
        let cloned = team.clone();
        assert_eq!(team, cloned);
    }

    #[test]
    fn test_team_serde_roundtrip() {
        let team = Team::new(Uuid::new_v4(), "Serialize Team".to_string());
        let json = serde_json::to_string(&team).expect("serialize");
        let deserialized: Team = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(team, deserialized);
    }

    #[test]
    fn test_team_error_display() {
        let invalid = TeamError::InvalidName("bad".to_string());
        assert!(invalid.to_string().contains("Invalid team name"));
        assert!(invalid.to_string().contains("bad"));

        let not_found = TeamError::NotFound(Uuid::nil());
        assert!(not_found.to_string().contains("Team not found"));
    }
}
