// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! bootstrap admin API key 签发的子步骤（从 `inject_bootstrap_admin_api_key` 提取）。

use std::io::IsTerminal;
use std::sync::Arc;

use log::info;

use crate::bootstrap::error::BootstrapError;
use crate::infrastructure::auth::get_garrison_dao;
use crate::infrastructure::database::entities::api_key::{
    ActiveModel as ApiKeyActiveModel, Entity as ApiKeyEntity,
};
use crate::infrastructure::database::entities::team::{
    ActiveModel as TeamActiveModel, Entity as TeamEntity,
};
use dbnexus::sea_orm::{ActiveValue, EntityTrait};

/// 读取 bootstrap admin key 环境变量；不存在或为空时返回 `None`。
pub(super) fn bootstrap_admin_key_config() -> Option<String> {
    match std::env::var("CRAWLRS__BOOTSTRAP_ADMIN_API_KEY") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// 解析可选的 team_id 参数（36 位裸 UUID）；非法/非 36 长度回退自动生成。
///
/// 格式约定（docs/USER_GUIDE.md:178）：`<team_id_uuid>` 是裸 36 位 UUID
/// 字符串，不是 `team_id_uuid` 复合格式；`Id::from_str` 按 UUID 解析的形态
/// 为「含 '-' 且长度 36」或「32 位紧凑 UUID」，本守卫只放行 36 位裸 UUID
/// （bootstrap 契约），非法/非 36 长度一律回退自动生成 team，不静默截断。
pub(super) fn resolve_bootstrap_team_id(cfg_val: &str) -> Option<crate::domain::id::Id> {
    if cfg_val.len() == 36 {
        match cfg_val.parse::<crate::domain::id::Id>() {
            Ok(id) => Some(id),
            Err(e) => {
                log::warn!(
                    "CRAWLRS__BOOTSTRAP_ADMIN_API_KEY 长度 36 但 UUID 解析失败，回退自动生成 team: {e}"
                );
                None
            }
        }
    } else {
        if !cfg_val.is_empty() {
            log::warn!(
                "CRAWLRS__BOOTSTRAP_ADMIN_API_KEY 非空但不是 36 位 UUID（长度 {}），回退自动生成 team",
                cfg_val.len()
            );
        }
        None
    }
}

/// 确保 bootstrap admin team 存在（bootstrap 首次启动时 team 表为空）。
pub(super) async fn ensure_bootstrap_team(
    conn: &dbnexus::sea_orm::DatabaseConnection,
    team_id: crate::domain::id::Id,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), BootstrapError> {
    const DEFAULT_TEAM_NAME: &str = "bootstrap-admin-team";

    if TeamEntity::find_by_id(team_id)
        .one(conn)
        .await
        .map_err(bootstrap_db_err("bootstrap_admin_key: find team"))?
        .is_none()
    {
        let team_active = TeamActiveModel {
            id: ActiveValue::Set(team_id.into()),
            name: ActiveValue::Set(DEFAULT_TEAM_NAME.to_string()),
            allowed_countries: ActiveValue::Set(None),
            blocked_countries: ActiveValue::Set(None),
            ip_whitelist: ActiveValue::Set(None),
            domain_blacklist: ActiveValue::Set(None),
            enable_geo_restrictions: ActiveValue::Set(false),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(now),
        };
        TeamEntity::insert(team_active)
            .exec(conn)
            .await
            .map_err(bootstrap_db_err("bootstrap_admin_key: insert team"))?;
    }

    Ok(())
}

/// 用 `set_garrison_dao` 注入的全局 DAO 签发 bootstrap admin key（与 server 进程共享内存）。
pub(super) async fn issue_bootstrap_admin_key(
    api_key_id: crate::domain::id::Id,
) -> Result<BootstrapApiKey, BootstrapError> {
    use garrison::protocol::apikey::ApiKeyHandler;

    const GARRISON_NS: &str = "crawlrs";
    const PERM_ADMIN: &str = "crawlrs:admin";
    const TTL_SECS: i64 = 30 * 24 * 60 * 60;

    let dao_for_gen = get_garrison_dao().ok_or_else(|| {
        BootstrapError::GarrisonManager(
            "bootstrap_admin_key: get_garrison_dao returned None".to_string(),
        )
    })?;
    let handler = ApiKeyHandler::new(Arc::clone(&dao_for_gen));
    let plaintext_key = handler
        .generate_with_namespace(
            api_key_id.to_string(),
            GARRISON_NS,
            vec![PERM_ADMIN.to_string()],
            TTL_SECS,
        )
        .await
        .map_err(|e| {
            BootstrapError::GarrisonManager(format!("bootstrap_admin_key: garrison generate: {e}"))
        })?;

    let key_id = plaintext_key
        .split_once('.')
        .map(|(k, _)| k.to_string())
        .ok_or_else(|| {
            BootstrapError::GarrisonManager(
                "bootstrap_admin_key: garrison returned malformed key".to_string(),
            )
        })?;

    Ok(BootstrapApiKey {
        plaintext: plaintext_key,
        key_id,
    })
}

/// 写入 crawlrs `api_keys` 表保留映射，auth_middleware 反查 team_id。
pub(super) async fn persist_bootstrap_api_key(
    conn: &dbnexus::sea_orm::DatabaseConnection,
    api_key_id: crate::domain::id::Id,
    team_id: crate::domain::id::Id,
    garrison_key_id: &str,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), BootstrapError> {
    let api_key_active = ApiKeyActiveModel {
        id: ActiveValue::Set(api_key_id.into()),
        team_id: ActiveValue::Set(team_id.into()),
        key: ActiveValue::Set(garrison_key_id.to_string()),
        created_at: ActiveValue::Set(now),
        updated_at: ActiveValue::Set(None),
    };
    ApiKeyEntity::insert(api_key_active)
        .exec(conn)
        .await
        .map_err(bootstrap_db_err("bootstrap_admin_key: insert api_key"))?;

    Ok(())
}

/// 输出 bootstrap admin key 信息：日志固定掩码，完整 key 仅在交互终端显示。
pub(super) fn announce_bootstrap_admin_key(
    plaintext_key: &str,
    team_id: crate::domain::id::Id,
    api_key_id: crate::domain::id::Id,
) {
    info!(
        "Bootstrap admin API key injected: team_id={}, api_key_id={} (TTY: use \
         `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY={}` to reuse team ID)",
        team_id, api_key_id, team_id
    );
    // 完整 key 仅在交互终端输出（运维在场立即保存）；stdout 被管道/日志系统
    // 采集时会持久留存，只输出掩码防止凭证进入 journald/k8s 日志。
    if std::io::stdout().is_terminal() {
        println!(
            "\n\
            ====================================================\n\
            BOOTSTRAP ADMIN API KEY (save this now!):\n\
            {}\n\
            TEAM_ID:      {}\n\
            API_KEY_ID:   {}\n\
            Valid for:    30 days\n\
            ====================================================\n",
            plaintext_key, team_id, api_key_id
        );
    } else {
        println!(
            "\n\
            ====================================================\n\
            BOOTSTRAP ADMIN API KEY (masked; rerun in a terminal to display):\n\
            {}\n\
            TEAM_ID:      {}\n\
            API_KEY_ID:   {}\n\
            Valid for:    30 days\n\
            ====================================================\n",
            crate::common::mask_secret(plaintext_key),
            team_id,
            api_key_id
        );
    }
}

/// bootstrap admin key 的签发结果（明文 key + garrison key_id）。
pub(super) struct BootstrapApiKey {
    pub(super) plaintext: String,
    pub(super) key_id: String,
}

/// 将 dbnexus 错误按上下文包装为类型化 bootstrap 错误。
fn bootstrap_db_err(ctx: &'static str) -> impl Fn(dbnexus::sea_orm::DbErr) -> BootstrapError {
    move |e| BootstrapError::GarrisonManager(format!("{}: db: {}", ctx, e))
}
