// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 团队数据库实体模型
///
/// 对应数据库中的 teams 表，存储团队的基本信息和地理限制配置
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "teams")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub allowed_countries: Option<Json>,
    pub blocked_countries: Option<Json>,
    pub ip_whitelist: Option<Json>,
    pub domain_blacklist: Option<Json>,
    pub enable_geo_restrictions: bool,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        has_many = "super::api_key::Entity",
        from = "Column::Id",
        to = "super::api_key::Column::TeamId"
    )]
    ApiKeys,
    #[sea_orm(
        has_many = "super::task::Entity",
        from = "Column::Id",
        to = "super::task::Column::TeamId"
    )]
    Tasks,
    #[sea_orm(
        has_many = "super::crawl::Entity",
        from = "Column::Id",
        to = "super::crawl::Column::TeamId"
    )]
    Crawls,
    #[sea_orm(
        has_many = "super::webhook::Entity",
        from = "Column::Id",
        to = "super::webhook::Column::TeamId"
    )]
    Webhooks,
    #[sea_orm(
        has_many = "super::credits::Entity",
        from = "Column::Id",
        to = "super::credits::Column::TeamId"
    )]
    Credits,
}

impl Related<super::api_key::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ApiKeys.def()
    }
}

impl Related<super::task::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tasks.def()
    }
}

impl Related<super::crawl::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Crawls.def()
    }
}

impl Related<super::webhook::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Webhooks.def()
    }
}

impl Related<super::credits::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Credits.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
