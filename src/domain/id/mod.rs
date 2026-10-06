// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 统一 ID 承载体 —— crawlrs 全量 ID 生成/处理迁移到 NebulaId 的落地类型。
//!
//! # 设计动机（门禁 1）
//!
//! [`nebulaid::core::Id`] 是一个 `u128` 新类型，仅实现
//! `Debug/Clone/PartialEq/Eq/Serialize/Deserialize`，**缺少** crawlrs 依赖的
//! `Copy`/`Hash`/`Default`/`Ord`/`FromStr`。Rust 孤儿规则禁止 crawlrs 为外部类型
//! 实现这些外部 trait，故在此定义本地 newtype 承载体 [`Id`]，内部直接存 `u128`
//! （NebulaId `Id` 的原值），并补齐全部所需 trait。
//!
//! # 值语义对齐 NebulaId
//!
//! [`Id::to_uuid_v8`] / [`Id::from_uuid_v8`] 与 NebulaId 的 [`nebulaid::core::Id`]
//! 同名方法逐字等价（`Uuid::from_u128` / `uuid.as_u128`）；[`Display`]/[`FromStr`]
//! 委托给 NebulaId 的对应实现，保证「version 位 ∈ {4,7,8} → 渲染为 UUID 字符串，
//! 否则渲染为十进制」的口径与 NebulaId 完全一致。
//!
//! # serde 契约
//!
//! [`Id`] 序列化为**字符串**：默认 `uuid_v8` 算法下值形如 UUID，`Display` 渲染为
//! 小写带连字符的 36 位 UUID 字符串，与迁移前 `uuid::Uuid` 的 JSON 表示完全一致，
//! 故 API/DTO 契约零改动、无需逐字段 `#[serde(with)]`。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// 进程级同步 ID 生成器门面（NebulaId 支撑）。
pub mod generator;

/// 统一 ID 承载体。
///
/// 存 `u128`（NebulaId `Id::as_u128` 的原值），使其可 `Copy`。跨进程边界类型只有
/// 本 newtype 与 [`nebulaid::core::Id`]（单一来源），`uuid::Uuid` 版本经依赖图并集
/// 统一为单一版本，故 [`Id::to_uuid_v8`] 返回的 `Uuid` 与 crawlrs 其它处使用的
/// `Uuid` 为同一类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(u128);

/// [`FromStr`] 解析失败错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseIdError(pub String);

impl fmt::Display for ParseIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid id string: {}", self.0)
    }
}

impl std::error::Error for ParseIdError {}

impl Id {
    /// 从 `u128` 原值构造。
    #[inline]
    pub const fn from_u128(value: u128) -> Self {
        Id(value)
    }

    /// 取 `u128` 原值（与 [`nebulaid::core::Id::as_u128`] 等价）。
    #[inline]
    pub const fn as_u128(&self) -> u128 {
        self.0
    }

    /// 全零哨兵 ID（等价于 `Uuid::nil()`）。
    #[inline]
    pub const fn nil() -> Self {
        Id(0)
    }

    /// 是否为哨兵 ID。
    #[inline]
    pub const fn is_nil(&self) -> bool {
        self.0 == 0
    }

    /// 渲染为 UUID v8 兼容的 [`Uuid`]（默认迁移路径：落库 `UUID` 列 / 边界互转）。
    ///
    /// 与 [`nebulaid::core::Id::to_uuid_v8`] 逐字等价。
    #[inline]
    pub fn to_uuid_v8(&self) -> Uuid {
        Uuid::from_u128(self.0)
    }

    /// 从 [`Uuid`] 构造（默认迁移路径读库：`Model.id: Uuid` → `Id`）。
    ///
    /// 与 [`nebulaid::core::Id::from_uuid_v8`] 逐字等价。
    #[inline]
    pub fn from_uuid_v8(uuid: Uuid) -> Self {
        Id(uuid.as_u128())
    }

    /// [`to_uuid_v8`](Id::to_uuid_v8) 的语义别名（边界代码可读性）。
    #[inline]
    pub fn to_uuid(&self) -> Uuid {
        self.to_uuid_v8()
    }

    /// [`from_uuid_v8`](Id::from_uuid_v8) 的语义别名。
    #[inline]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Id::from_uuid_v8(uuid)
    }

    /// 取 i64（Snowflake 备选算法落 `BIGINT` 用；对 UUID 形态值会截断，勿在默认路径使用）。
    #[inline]
    pub const fn to_i64(&self) -> i64 {
        self.0 as i64
    }

    /// 从 i64 构造（Snowflake 备选算法路径）。
    #[inline]
    pub const fn from_i64(value: i64) -> Self {
        Id(value as u128)
    }

    /// 测试专用确定性构造：从任意 `u64` 生成非零稳定 ID，免依赖生成器、避免 flaky。
    #[inline]
    pub const fn for_test(seed: u64) -> Self {
        Id(seed as u128)
    }

    /// 测试专用：返回进程内全局唯一的非零 ID（原子计数器），用于需要互不
    /// 重复的测试夹具（替代测试中的 `Uuid::new_v4()`，无需依赖生成器）。
    ///
    /// 计数器以**进程随机基址**起始而非 1：`cargo test --tests` 下多个测试二进制
    /// 共享同一 PostgreSQL，若各自都从 1 起，则 `to_uuid_v8` 后与其他二进制已插入
    /// 行主键碰撞（PK violation，或“未知 team”读到他人行），破坏 new_v4 原有的
    /// 跨进程唯一语义。随机基址 + 递增恢复该唯一性。
    pub fn test_new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::OnceLock;
        static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
        let counter = COUNTER.get_or_init(|| AtomicU64::new(rand::random::<u64>() | 1));
        Id(counter.fetch_add(1, Ordering::Relaxed) as u128)
    }

    /// 借出底层 NebulaId `Id`（供需要直接调用 NebulaId API 的场景）。
    #[inline]
    pub fn as_nebulaid(&self) -> nebulaid::core::Id {
        nebulaid::core::Id::from_u128(self.0)
    }

    /// 同步生成下一个 ID（NebulaId 支撑，`Uuid::new_v4()` 的直接替换物）。
    ///
    /// 免 `await`、免注入；适用于 `impl Default`、结构体字面量、同步 mapper 与
    /// 测试等无法 await 的上下文。详见 [`generator::generate`]。
    #[inline]
    pub fn generate() -> Id {
        generator::generate()
    }
}

impl From<nebulaid::core::Id> for Id {
    #[inline]
    fn from(id: nebulaid::core::Id) -> Self {
        Id(id.as_u128())
    }
}

impl From<Id> for nebulaid::core::Id {
    #[inline]
    fn from(id: Id) -> Self {
        nebulaid::core::Id::from_u128(id.0)
    }
}

impl From<Uuid> for Id {
    #[inline]
    fn from(uuid: Uuid) -> Self {
        Id(uuid.as_u128())
    }
}

impl From<Id> for Uuid {
    #[inline]
    fn from(id: Id) -> Self {
        Uuid::from_u128(id.0)
    }
}

/// 与 `uuid::Uuid` 的跨类型相等比较（承载体为本地类型，合法）。
///
/// 便于测试断言与边界比较：`id == some_uuid`（按 u128 原值比较）。
impl PartialEq<Uuid> for Id {
    #[inline]
    fn eq(&self, other: &Uuid) -> bool {
        self.0 == other.as_u128()
    }
}

impl Default for Id {
    #[inline]
    fn default() -> Self {
        Id::nil()
    }
}

impl fmt::Display for Id {
    /// 委托 NebulaId 的 `Display`：version 位 ∈ {4,7,8} → UUID 字符串，否则十进制。
    ///
    /// 特例：全零哨兵（`Id::nil()`）渲染为与迁移前 `uuid::Uuid::nil()` 一致的
    /// 零值 UUID 字符串，保持错误信息/日志/序列化契约不变（生成的 uuid_v8/test
    /// ID 永不为全零，故仅影响哨兵）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_nil() {
            return f.write_str("00000000-0000-0000-0000-000000000000");
        }
        write!(f, "{}", self.as_nebulaid())
    }
}

impl fmt::Debug for Id {
    /// 与 `Display` 一致（对齐 `uuid::Uuid` 的 Debug 输出为字符串形态）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Id {
    type Err = ParseIdError;

    /// 解析：含 `-` 且长度 36 且为合法 UUID → u128；否则按十进制 u128 解析。
    ///
    /// 与 [`nebulaid::core::Id::from_string`] 同口径，兼容 UUID 字符串路径参数与
    /// 数值 ID。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cleaned = s.trim();
        if cleaned.contains('-') && cleaned.len() == 36 {
            if let Ok(uuid) = Uuid::parse_str(cleaned) {
                return Ok(Id(uuid.as_u128()));
            }
        }
        cleaned
            .parse::<u128>()
            .map(Id)
            .map_err(|_| ParseIdError(s.to_string()))
    }
}

impl Serialize for Id {
    /// 序列化为字符串（UUID 形态值为标准 36 位 UUID 字符串，保持 API 契约不变）。
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nil_default_and_is_nil() {
        assert!(Id::nil().is_nil());
        assert!(Id::default().is_nil());
        assert_eq!(Id::nil(), Id::default());
        assert_eq!(Id::nil().as_u128(), 0);
    }

    #[test]
    fn test_u128_roundtrip() {
        let id = Id::from_u128(12345678901234567890u128);
        assert_eq!(id.as_u128(), 12345678901234567890u128);
    }

    #[test]
    fn test_uuid_roundtrip_preserves_bytes() {
        let uuid = Uuid::new_v4();
        let id = Id::from_uuid_v8(uuid);
        assert_eq!(id.to_uuid_v8(), uuid);
    }

    #[test]
    fn test_uuidv8_shape_displays_as_hyphenated_uuid() {
        // 构造 version=8 / variant=10 的 v8 UUID（NebulaId uuid_v8 输出形态）
        let mut bytes = [0u8; 16];
        bytes[6] = 0x80; // version nibble = 8
        bytes[8] = 0x80; // variant = 10
        let uuid = Uuid::from_bytes(bytes);
        let id = Id::from_uuid_v8(uuid);
        let rendered = id.to_string();
        assert_eq!(rendered.len(), 36, "v8 形态应渲染为 36 位 UUID 字符串");
        assert_eq!(rendered.matches('-').count(), 4);
        assert_eq!(rendered, uuid.to_string());
    }

    #[test]
    fn test_numeric_id_displays_as_decimal() {
        let id = Id::from_u128(12345);
        assert_eq!(id.to_string(), "12345");
    }

    #[test]
    fn test_from_str_uuid_and_numeric() {
        let uuid = Uuid::new_v4();
        let parsed: Id = uuid.to_string().parse().unwrap();
        assert_eq!(parsed.to_uuid_v8(), uuid);

        let num: Id = "9876543210".parse().unwrap();
        assert_eq!(num.as_u128(), 9876543210u128);
    }

    #[test]
    fn test_from_str_invalid() {
        assert!("not-an-id".parse::<Id>().is_err());
        assert!("".parse::<Id>().is_err());
    }

    #[test]
    fn test_serde_uuid_string_roundtrip() {
        let mut bytes = [0u8; 16];
        bytes[6] = 0x80;
        bytes[8] = 0x80;
        let id = Id::from_uuid(Uuid::from_bytes(bytes));
        let json = serde_json::to_string(&id).unwrap();
        // 序列化必须是带引号的 UUID 字符串，与迁移前 uuid::Uuid 契约一致
        assert!(json.starts_with('"') && json.ends_with('"'));
        assert_eq!(json.matches('-').count(), 4);
        let back: Id = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn test_nebulaid_conversions_roundtrip() {
        let neb = nebulaid::core::Id::from_u128(42);
        let id: Id = neb.into();
        assert_eq!(id.as_u128(), 42);
        let back: nebulaid::core::Id = id.into();
        assert_eq!(back.as_u128(), 42);
        assert_eq!(id.as_nebulaid().as_u128(), 42);
    }

    #[test]
    fn test_is_hashable_and_copyable() {
        use std::collections::HashMap;
        let a = Id::for_test(1);
        let b = a; // Copy
        let mut map = HashMap::new();
        map.insert(a, "x");
        assert_eq!(map[&b], "x"); // Hash + Eq + Copy 可用
    }

    #[test]
    fn test_ordering() {
        assert!(Id::for_test(1) < Id::for_test(2));
    }

    /// 覆盖 `to_uuid`/`from_uuid` 语义别名与 `to_uuid_v8`/`from_uuid_v8` 一致。
    #[test]
    fn test_uuid_aliases_equivalence() {
        let uuid = Uuid::new_v4();
        let id = Id::from_uuid(uuid);
        assert_eq!(id.to_uuid(), id.to_uuid_v8());
        assert_eq!(id.to_uuid(), uuid);
        assert_eq!(Id::from_uuid(id.to_uuid()), id);
    }

    /// 覆盖 i64 边界转换（Snowflake 备选路径）与 `for_test` 确定性。
    #[test]
    fn test_i64_and_for_test_conversions() {
        let id = Id::from_i64(42);
        assert_eq!(id.to_i64(), 42);
        assert_eq!(id.as_u128(), 42u128);
        // for_test 确定性：同种子同值、非零。
        let a = Id::for_test(7);
        let b = Id::for_test(7);
        assert_eq!(a, b);
        assert!(!a.is_nil());
    }

    /// 覆盖 `test_new` 的进程内唯一、非零语义（跨二进制碰撞回归）。
    #[test]
    fn test_new_is_unique_and_nonzero() {
        let a = Id::test_new();
        let b = Id::test_new();
        assert!(!a.is_nil() && !b.is_nil(), "test_new 必须非 nil");
        assert_ne!(a, b, "同进程连续两次 test_new 必须唯一");
    }

    /// 覆盖 `as_nebulaid` 借出与 `From<Id> for nebulaid::core::Id`/`From<Id> for Uuid`
    /// 双向转换，以及 `PartialEq<Uuid>` 跨类型比较。
    #[test]
    fn test_nebulaid_and_uuid_cross_conversions() {
        let id = Id::from_u128(0x0000_0000_0000_0000_0000_0000_0000_002a_u128); // 42
                                                                                // as_nebulaid 原值一致。
        assert_eq!(id.as_nebulaid().as_u128(), 42u128);
        // From<Id> for nebulaid::core::Id
        let neb: nebulaid::core::Id = id.into();
        assert_eq!(neb.as_u128(), 42u128);
        // From<Id> for Uuid 与 to_uuid_v8 一致。
        let as_uuid: Uuid = id.into();
        assert_eq!(as_uuid, id.to_uuid_v8());
        // PartialEq<Uuid> 跨类型比较（按 u128 原值）。
        assert!(id == as_uuid);
        assert!(id != Uuid::nil());
    }
}
