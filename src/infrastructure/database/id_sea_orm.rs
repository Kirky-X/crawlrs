// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 让本地承载体 [`Id`] 成为 sea-orm/sea-query 的一等查询值类型。
//!
//! sea-orm 的查询构造（`Column::eq`、`is_in`、`Expr::value` 等）以 `V: Into<Value>`
//! 为约束；默认迁移路径下 ID 列仍为 `UUID`（实体保持 `uuid::Uuid`），故 [`Id`]
//! 经 `to_uuid_v8()` 委托到 `Uuid` 的 `Into<Value>` 实现即可。`Id` 为本 crate
//! 本地类型，实现 sea-orm 的 `Value` 转换无孤儿规则问题。
//!
//! 这使仓储层大量 `.filter(Column::Id.eq(id))`、`is_in(ids)`、`Expr::value(id)`
//! 等查询点无需逐处手写 `.to_uuid_v8()` 即可编译；实体字段/ActiveModel 赋值、
//! `None::<_>` 空值表达式等「Rust 类型必须为 `Uuid`」的边界仍由对应转换承接。

use crate::domain::id::Id;
use dbnexus::sea_orm::sea_query::Value;

impl From<Id> for Value {
    fn from(id: Id) -> Self {
        Value::from(id.to_uuid_v8())
    }
}
