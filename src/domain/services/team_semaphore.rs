// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::utils::adaptive_concurrency::{AIMDController, AdaptiveSemaphore};

/// AIMD 自适应模式参数
///
/// `TeamSemaphore::with_adaptive` 时使用，每队信号量由 `AdaptiveSemaphore` 承载，
/// 配套 `AIMDController` 记录成功/失败并动态调整 target。
///
/// # 构造方式
///
/// - [`AdaptiveParams::new`]：带不变式校验的构造函数（推荐，架构审查 M-3）
/// - 结构体字面量 + `Default::default()`：仅在测试代码中使用，生产代码应走 `new`
///
/// # 不变式
///
/// - `min_limit >= 1`：乘性减少的地板，否则可降至 0 导致死锁
/// - `max_limit >= min_limit`：clamp 语义要求
/// - `increase_threshold >= 1`：否则永远不触发 +1
/// - `initial` 会被 clamp 到 `[min_limit, max_limit]`
#[derive(Clone, Debug)]
pub struct AdaptiveParams {
    /// 初始并发上限
    pub initial: usize,
    /// 最小并发上限（乘性减少的地板）
    pub min_limit: usize,
    /// 最大并发上限（加性增加的天花板）
    pub max_limit: usize,
    /// 连续成功多少次才 +1
    pub increase_threshold: usize,
}

impl AdaptiveParams {
    /// 创建 `AdaptiveParams`，应用不变式校验（架构审查 M-3）
    ///
    /// # Panics
    ///
    /// 参数不变式违反时 panic（fail-fast，规则 12）：
    /// - `min_limit >= 1`
    /// - `max_limit >= min_limit`
    /// - `increase_threshold >= 1`
    ///
    /// `initial` 会被 clamp 到 `[min_limit, max_limit]`。
    ///
    /// # 示例
    ///
    /// ```
    /// # use crawlrs::domain::services::team_semaphore::AdaptiveParams;
    /// let params = AdaptiveParams::new(10, 1, 100, 5);
    /// assert_eq!(params.initial, 10);
    /// ```
    #[must_use]
    pub fn new(
        initial: usize,
        min_limit: usize,
        max_limit: usize,
        increase_threshold: usize,
    ) -> Self {
        // 委托给 AIMDController::with_params 的校验逻辑（避免重复实现）
        // 若校验通过，AIMDController 也会用相同参数构造
        let _ = AIMDController::with_params(
            initial,
            min_limit,
            max_limit,
            increase_threshold,
        );
        let clamped = initial.clamp(min_limit, max_limit);
        Self {
            initial: clamped,
            min_limit,
            max_limit,
            increase_threshold,
        }
    }
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self {
            initial: 10,
            min_limit: crate::utils::adaptive_concurrency::DEFAULT_MIN_LIMIT,
            max_limit: crate::utils::adaptive_concurrency::DEFAULT_MAX_LIMIT,
            increase_threshold: crate::utils::adaptive_concurrency::DEFAULT_INCREASE_THRESHOLD,
        }
    }
}

/// 每队信号量条目（存储于 DashMap）
///
/// - `Fixed`：固定 `Semaphore`（默认行为，`adaptive_enabled=false`）
/// - `Adaptive`：`AdaptiveSemaphore` + `AIMDController`（T037/R-runtime-003）
#[derive(Debug)]
enum TeamEntry {
    /// 固定并发模式
    Fixed(Arc<Semaphore>),
    /// AIMD 自适应并发模式（T037/R-runtime-003）
    Adaptive {
        adaptive: Arc<AdaptiveSemaphore>,
        controller: Arc<AIMDController>,
    },
}

/// 每队信号量的快照句柄（owned clones of the inner Arcs）
///
/// 从 DashMap 中克隆 Arc 出来，避免持有 Ref 跨 await（防止死锁）。
/// DashMap 的 Ref 持有 shard 读锁，跨 await 会阻塞其他写操作。
#[derive(Clone, Debug)]
enum TeamHandle {
    Fixed(Arc<Semaphore>),
    Adaptive {
        adaptive: Arc<AdaptiveSemaphore>,
        controller: Arc<AIMDController>,
    },
}

impl TeamHandle {
    /// 异步获取一个许可
    async fn acquire_owned(&self) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        match self {
            TeamHandle::Fixed(sem) => Arc::clone(sem).acquire_owned().await,
            TeamHandle::Adaptive { adaptive, .. } => Ok(adaptive.acquire().await),
        }
    }

    /// 非阻塞尝试获取许可
    fn try_acquire_owned(&self) -> Option<OwnedSemaphorePermit> {
        match self {
            TeamHandle::Fixed(sem) => Arc::clone(sem).try_acquire_owned().ok(),
            TeamHandle::Adaptive { adaptive, .. } => adaptive.try_acquire(),
        }
    }

    /// 获取当前 target（Fixed 模式返回 available_permits；Adaptive 返回 controller.current_limit）
    fn current_target(&self) -> usize {
        match self {
            TeamHandle::Fixed(sem) => sem.available_permits(),
            TeamHandle::Adaptive { controller, .. } => controller.current_limit(),
        }
    }

    /// 记录一次成功（Adaptive 模式回填 controller，Fixed 模式无操作）
    fn record_success(&self) -> usize {
        match self {
            TeamHandle::Fixed(sem) => sem.available_permits(),
            TeamHandle::Adaptive { controller, adaptive, .. } => {
                let new_target = controller.record_success();
                adaptive.set_target(new_target);
                new_target
            }
        }
    }

    /// 记录一次失败（Adaptive 模式回填 controller，Fixed 模式无操作）
    fn record_failure(&self) -> usize {
        match self {
            TeamHandle::Fixed(sem) => sem.available_permits(),
            TeamHandle::Adaptive { controller, adaptive, .. } => {
                let new_target = controller.record_failure();
                adaptive.set_target(new_target);
                new_target
            }
        }
    }

    /// 是否为 Adaptive 模式
    fn is_adaptive(&self) -> bool {
        matches!(self, TeamHandle::Adaptive { .. })
    }
}

/// 每队并发信号量管理器
///
/// 为每个团队提供一个独立的并发信号量，以限制其并发请求数。
///
/// T037/R-runtime-003：支持两种模式
/// - **Fixed**（默认）：每队固定 `default_permits` 个许可，行为等同 Stage 1 之前
/// - **Adaptive**：每队由 `AdaptiveSemaphore` + `AIMDController` 承载，
///   `record_success`/`record_failure` 调整动态 target，开启后增强固定并发为动态带宽利用
///
/// 安全审查 H-01 修复：`max_teams` 限制最大团队数，防止无界增长 DoS。
/// 超过上限时驱逐空闲团队（available_permits == full），仍持有 permit 的团队不会被驱逐。
#[derive(Clone, Debug)]
pub struct TeamSemaphore {
    /// 存储每队的信号量条目
    semaphores: Arc<DashMap<Uuid, TeamEntry>>,
    /// 默认并发数（Fixed 模式即每队 permits；Adaptive 模式作为 initial）
    default_permits: usize,
    /// 信号量模式参数
    mode: SemaphoreMode,
    /// 最大团队数限制（防止无界增长 DoS，安全审查 H-01）
    max_teams: usize,
}

/// 默认最大团队数（安全审查 H-01）
pub const DEFAULT_MAX_TEAMS: usize = 10000;

/// 信号量模式
#[derive(Clone, Debug)]
enum SemaphoreMode {
    /// 固定并发
    Fixed,
    /// AIMD 自适应并发（T037/R-runtime-003）
    Adaptive(AdaptiveParams),
}

impl std::fmt::Display for SemaphoreMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemaphoreMode::Fixed => write!(f, "Fixed"),
            SemaphoreMode::Adaptive(_) => write!(f, "Adaptive"),
        }
    }
}

impl TeamSemaphore {
    /// 创建一个固定并发模式的 TeamSemaphore（默认行为）
    ///
    /// # 参数
    ///
    /// * `default_permits` - 每个团队的默认并发许可数
    ///
    /// # 返回值
    ///
    /// 返回新的 TeamSemaphore 实例（Fixed 模式）
    pub fn new(default_permits: usize) -> Self {
        Self {
            semaphores: Arc::new(DashMap::new()),
            default_permits,
            mode: SemaphoreMode::Fixed,
            max_teams: DEFAULT_MAX_TEAMS,
        }
    }

    /// T037/R-runtime-003：创建一个 AIMD 自适应并发模式的 TeamSemaphore
    ///
    /// 每队信号量由 `AdaptiveSemaphore` 承载，配套 `AIMDController` 记录成功/失败。
    /// `scrape_worker` 成功/失败后调用 `record_success`/`record_failure` 回填 controller，
    /// controller 输出的动态 target 通过 `AdaptiveSemaphore::set_target` 推入。
    ///
    /// # 参数
    ///
    /// * `params` - AIMD 自适应参数（initial/min/max/increase_threshold）
    ///
    /// # 返回值
    ///
    /// 返回新的 TeamSemaphore 实例（Adaptive 模式）
    pub fn with_adaptive(params: AdaptiveParams) -> Self {
        Self {
            semaphores: Arc::new(DashMap::new()),
            default_permits: params.initial,
            mode: SemaphoreMode::Adaptive(params),
            max_teams: DEFAULT_MAX_TEAMS,
        }
    }

    /// 安全审查 H-01：设置最大团队数限制
    ///
    /// 超过限制时，新建团队会触发驱逐最久空闲的团队（available_permits == full）。
    /// 仍持有 permit 的团队不会被驱逐。
    ///
    /// # 参数
    ///
    /// * `max_teams` - 最大团队数（必须 > 0）
    ///
    /// # 返回值
    ///
    /// 返回 `Self`（builder 模式）
    pub fn with_max_teams(mut self, max_teams: usize) -> Self {
        self.max_teams = max_teams.max(1);
        self
    }

    /// 查询最大团队数限制
    pub fn max_teams(&self) -> usize {
        self.max_teams
    }

    /// 安全审查 H-01：驱逐空闲团队为新团队腾出位置
    ///
    /// 在 `get_or_create` 插入新团队**之前**调用。
    /// 若当前团队数已达 `max_teams`，扫描并驱逐一个空闲团队
    /// （`available_permits == full`）。仍持有 permit 的团队不会被驱逐。
    /// 返回驱逐的数量（0 或 1）。
    ///
    /// 设计要点：必须在新条目插入前调用，否则新创建的团队本身也是空闲的，
    /// 可能被自身驱逐逻辑选中。
    ///
    /// # 性能权衡（性能审查 M-2）
    ///
    /// 线性扫描 `DashMap`，最坏 O(n)（n = max_teams，默认 10000）。
    /// 仅在 `semaphores.len() >= max_teams` 时触发，即团队数达上限时。
    /// 触发频率：每个新团队首次插入时一次（之后 lookup 命中 fast path）。
    ///
    /// 不优化为 LRU/空闲队列的理由：
    /// - 极低触发频率（团队数达 10000 在生产环境罕见）
    /// - 增加旁路数据结构（idle_teams VecDeque）会引入额外同步开销和内存
    /// - DashMap shard 读锁持有期间仅扫描本 shard，不阻塞其他 shard 写
    /// - 若未来 max_teams 提升至百万级，再引入 LRU 缓存（如 moka）
    fn evict_one_for_new(&self) -> usize {
        if self.semaphores.len() < self.max_teams {
            return 0;
        }

        // 扫描一遍，找一个空闲团队驱逐
        let mut evicted_team: Option<Uuid> = None;
        for entry in self.semaphores.iter() {
            let is_idle = match entry.value() {
                TeamEntry::Fixed(sem) => sem.available_permits() >= self.default_permits,
                TeamEntry::Adaptive { adaptive, .. } => {
                    adaptive.available_permits() >= self.default_permits
                }
            };
            if is_idle {
                evicted_team = Some(*entry.key());
                break;
            }
        }

        // 实际驱逐（在锁外执行 remove）
        if let Some(team_id) = evicted_team {
            self.semaphores.remove(&team_id);
            1
        } else {
            0
        }
    }

    /// 查询当前团队数（测试与监控用）
    pub fn team_count(&self) -> usize {
        self.semaphores.len()
    }

    /// 查询当前模式（测试与日志用）
    pub fn mode(&self) -> &str {
        match &self.mode {
            SemaphoreMode::Fixed => "Fixed",
            SemaphoreMode::Adaptive(_) => "Adaptive",
        }
    }

    /// 查询默认许可数（Fixed 模式即每队 permits；Adaptive 模式作为 initial）
    /// 测试与诊断用
    pub fn default_permits(&self) -> usize {
        self.default_permits
    }

    /// 获取指定团队的信号量许可
    ///
    /// 如果该团队的信号量不存在，则会创建一个新的。
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回一个信号量许可
    pub async fn acquire(
        &self,
        team_id: Uuid,
    ) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        let handle = self.get_or_create(team_id);
        handle.acquire_owned().await
    }

    /// 尝试获取指定团队的信号量许可（非阻塞）
    ///
    /// 如果当前没有可用许可，立即返回 `None`。
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回 `Some(permit)` 如果成功获取许可，否则返回 `None`
    pub fn try_acquire(&self, team_id: Uuid) -> Option<OwnedSemaphorePermit> {
        let handle = self.get_or_create(team_id);
        handle.try_acquire_owned()
    }

    /// T037/R-runtime-003：记录某团队一次抓取成功
    ///
    /// - **Fixed 模式**：无操作（返回当前 available_permits，仅用于日志一致性）
    /// - **Adaptive 模式**：回填 `AIMDController::record_success`，若达到阈值则 +1，
    ///   并通过 `AdaptiveSemaphore::set_target` 推入新 target
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回更新后的 target（Fixed 模式为 available_permits；Adaptive 为 controller.current_limit）
    pub fn record_success(&self, team_id: Uuid) -> usize {
        let handle = self.get_or_create(team_id);
        handle.record_success()
    }

    /// T037/R-runtime-003：记录某团队一次抓取失败
    ///
    /// - **Fixed 模式**：无操作（返回当前 available_permits，仅用于日志一致性）
    /// - **Adaptive 模式**：回填 `AIMDController::record_failure`，target 减半 clamp min，
    ///   并通过 `AdaptiveSemaphore::set_target` 推入新 target
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回更新后的 target（Fixed 模式为 available_permits；Adaptive 为 controller.current_limit）
    pub fn record_failure(&self, team_id: Uuid) -> usize {
        let handle = self.get_or_create(team_id);
        handle.record_failure()
    }

    /// 获取某队当前 target（测试与监控用，无副作用）
    ///
    /// - **Fixed 模式**：返回 `available_permits`（随 acquire 动态变化）
    /// - **Adaptive 模式**：返回 `controller.current_limit`（动态 target）
    /// - **未存在 team**：返回 `default_permits`（不创建条目）
    pub fn current_target(&self, team_id: Uuid) -> usize {
        match self.lookup(team_id) {
            Some(handle) => handle.current_target(),
            None => self.default_permits,
        }
    }

    /// 判断某队是否为 Adaptive 模式（测试用，无副作用）
    ///
    /// - **未存在 team**：返回当前模式标志（Fixed 返回 false，Adaptive 返回 true）
    pub fn is_team_adaptive(&self, team_id: Uuid) -> bool {
        match self.lookup(team_id) {
            Some(handle) => handle.is_adaptive(),
            None => matches!(self.mode, SemaphoreMode::Adaptive(_)),
        }
    }

    /// 只读查询某队的信号量句柄（不创建条目）
    ///
    /// 用于 `current_target`/`is_team_adaptive` 等无副作用查询。
    fn lookup(&self, team_id: Uuid) -> Option<TeamHandle> {
        let entry = self.semaphores.get(&team_id)?;
        let handle = match entry.value() {
            TeamEntry::Fixed(sem) => TeamHandle::Fixed(Arc::clone(sem)),
            TeamEntry::Adaptive { adaptive, controller } => TeamHandle::Adaptive {
                adaptive: Arc::clone(adaptive),
                controller: Arc::clone(controller),
            },
        };
        drop(entry);
        Some(handle)
    }

    /// 获取或创建指定团队的信号量句柄
    ///
    /// 返回 [`TeamHandle`]（owned clones of inner Arcs），不持有 DashMap Ref，
    /// 故调用方可在 await 中安全使用，避免死锁。
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    fn get_or_create(&self, team_id: Uuid) -> TeamHandle {
        use dashmap::mapref::entry::Entry;

        // 安全审查 H-01：先检查是否已存在（fast path，避免不必要的 evict 调用）
        //
        // 注意：不能在 `Entry::Vacant(v)` 中调用 `evict_one_for_new`，
        // 因为 `v` 持有 shard 写锁，调用 `self.semaphores.iter()` 会死锁。
        // 故先 lookup 一次（释放读锁后），不存在再走 entry 路径。
        if let Some(handle) = self.lookup(team_id) {
            return handle;
        }

        // 不存在：插入前先驱逐一个空闲团队（如果已达 max_teams）
        //
        // 必须在 entry 之前调用，因为 entry 持有 shard 锁时调用其他 DashMap 操作会死锁。
        // 这里存在 TOCTOU race：lookup 后另一线程可能已插入该 team_id，
        // 但下方 `Entry::Occupied` 分支会正确处理这种情况（复用已插入的 entry）。
        if self.semaphores.len() >= self.max_teams {
            let _evicted = self.evict_one_for_new();
        }

        match self.semaphores.entry(team_id) {
            Entry::Occupied(o) => {
                // 已存在：克隆内部 Arc，返回 handle（drop o 释放 shard 锁）
                let handle = match o.get() {
                    TeamEntry::Fixed(sem) => TeamHandle::Fixed(Arc::clone(sem)),
                    TeamEntry::Adaptive { adaptive, controller } => TeamHandle::Adaptive {
                        adaptive: Arc::clone(adaptive),
                        controller: Arc::clone(controller),
                    },
                };
                drop(o);
                handle
            }
            Entry::Vacant(v) => {
                // 不存在：按模式创建新条目
                let new_entry = match &self.mode {
                    SemaphoreMode::Fixed => {
                        TeamEntry::Fixed(Arc::new(Semaphore::new(self.default_permits)))
                    }
                    SemaphoreMode::Adaptive(params) => {
                        let controller = Arc::new(AIMDController::with_params(
                            params.initial,
                            params.min_limit,
                            params.max_limit,
                            params.increase_threshold,
                        ));
                        let adaptive = Arc::new(AdaptiveSemaphore::new(params.initial));
                        TeamEntry::Adaptive {
                            adaptive,
                            controller,
                        }
                    }
                };
                // 插入前先克隆出 handle（避免再次 lookup）
                let handle = match &new_entry {
                    TeamEntry::Fixed(sem) => TeamHandle::Fixed(Arc::clone(sem)),
                    TeamEntry::Adaptive { adaptive, controller } => TeamHandle::Adaptive {
                        adaptive: Arc::clone(adaptive),
                        controller: Arc::clone(controller),
                    },
                };
                v.insert(new_entry);
                handle
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new_stores_default_permits() {
        let sem = TeamSemaphore::new(5);
        assert_eq!(sem.default_permits, 5);
        assert_eq!(sem.mode(), "Fixed");
    }

    /// 安全审查 H-01：max_teams 限制生效，空闲团队会被驱逐
    #[test]
    fn test_with_max_teams_evicts_idle_teams() {
        let sem = TeamSemaphore::new(2).with_max_teams(2);
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let team_c = Uuid::new_v4();

        // 创建 3 个团队，但全部空闲（无 permit 持有）
        let _handle_a = sem.get_or_create(team_a);
        drop(_handle_a);
        let _handle_b = sem.get_or_create(team_b);
        drop(_handle_b);
        let _handle_c = sem.get_or_create(team_c);

        // 超过 max_teams=2，应驱逐 1 个空闲团队
        assert_eq!(sem.team_count(), 2);
    }

    /// 安全审查 H-01：max_teams 默认值为 DEFAULT_MAX_TEAMS
    #[test]
    fn test_default_max_teams() {
        let sem = TeamSemaphore::new(10);
        assert_eq!(sem.max_teams(), DEFAULT_MAX_TEAMS);
    }

    /// 安全审查 H-01：仍持有 permit 的团队不会被驱逐
    #[test]
    fn test_with_max_teams_keeps_busy_teams() {
        let sem = TeamSemaphore::new(1).with_max_teams(2);
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let team_c = Uuid::new_v4();

        // team_a 持有 permit，team_b 空闲
        let _permit_a = sem.try_acquire(team_a).expect("first acquire");
        let _handle_b = sem.get_or_create(team_b);
        drop(_handle_b);
        // 创建 team_c 触发驱逐
        let _handle_c = sem.get_or_create(team_c);

        // team_a 仍持有 permit 不应被驱逐，team_b 空闲应被驱逐
        assert_eq!(sem.team_count(), 2);
        assert!(sem.semaphores.contains_key(&team_a));
        assert!(sem.semaphores.contains_key(&team_c));
    }

    #[test]
    fn test_new_with_zero_permits() {
        let sem = TeamSemaphore::new(0);
        assert_eq!(sem.default_permits, 0);
    }

    #[test]
    fn test_new_starts_with_empty_map() {
        let sem = TeamSemaphore::new(3);
        let team_id = Uuid::new_v4();
        assert!(!sem.semaphores.contains_key(&team_id));
    }

    #[test]
    fn test_get_or_create_creates_new_semaphore_fixed() {
        let sem = TeamSemaphore::new(4);
        let team_id = Uuid::new_v4();
        let handle = sem.get_or_create(team_id);
        // Fixed 模式：entry 应为 Fixed
        assert!(!handle.is_adaptive());
        // Newly created semaphore should have all permits available
        assert!(handle.try_acquire_owned().is_some());
        // Map should now contain an entry for this team
        assert!(sem.semaphores.contains_key(&team_id));
    }

    #[test]
    fn test_get_or_create_reuses_existing_semaphore() {
        let sem = TeamSemaphore::new(1);
        let team_id = Uuid::new_v4();
        let first = sem.get_or_create(team_id);
        // 提取 Arc<Semaphore> 用于 ptr_eq 比较
        let first_ptr = match &first {
            TeamHandle::Fixed(s) => Arc::clone(s),
            _ => unreachable!(),
        };
        let second = sem.get_or_create(team_id);
        let second_ptr = match &second {
            TeamHandle::Fixed(s) => Arc::clone(s),
            _ => unreachable!(),
        };
        // Both should point to the same underlying semaphore (Arc equality)
        assert!(Arc::ptr_eq(&first_ptr, &second_ptr));
        // Only one entry should exist for this team
        assert_eq!(sem.semaphores.len(), 1);
    }

    #[test]
    fn test_get_or_create_isolates_different_teams() {
        let sem = TeamSemaphore::new(2);
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let handle_a = sem.get_or_create(team_a);
        let handle_b = sem.get_or_create(team_b);
        // Different teams must get distinct entries
        let sem_a = match &handle_a {
            TeamHandle::Fixed(s) => Arc::clone(s),
            _ => unreachable!(),
        };
        let sem_b = match &handle_b {
            TeamHandle::Fixed(s) => Arc::clone(s),
            _ => unreachable!(),
        };
        assert!(!Arc::ptr_eq(&sem_a, &sem_b));
        assert_eq!(sem.semaphores.len(), 2);
    }

    #[tokio::test]
    async fn test_acquire_returns_permit() {
        let sem = TeamSemaphore::new(3);
        let team_id = Uuid::new_v4();
        let permit = sem.acquire(team_id).await;
        assert!(permit.is_ok());
        assert!(sem.semaphores.contains_key(&team_id));
    }

    #[tokio::test]
    async fn test_acquire_within_limit_succeeds() {
        let sem = TeamSemaphore::new(2);
        let team_id = Uuid::new_v4();
        let p1 = sem
            .acquire(team_id)
            .await
            .expect("first acquire should succeed");
        let p2 = sem
            .acquire(team_id)
            .await
            .expect("second acquire should succeed");
        let _ = (p1, p2);
    }

    #[tokio::test]
    async fn test_acquire_over_limit_blocks_until_timeout() {
        let sem = TeamSemaphore::new(1);
        let team_id = Uuid::new_v4();
        let _held_permit = sem
            .acquire(team_id)
            .await
            .expect("first acquire should succeed");
        let result = tokio::time::timeout(Duration::from_millis(50), sem.acquire(team_id)).await;
        assert!(
            result.is_err(),
            "second acquire should time out when permits are exhausted"
        );
    }

    #[tokio::test]
    async fn test_acquire_permit_release_allows_next() {
        let sem = TeamSemaphore::new(1);
        let team_id = Uuid::new_v4();
        {
            let _permit = sem
                .acquire(team_id)
                .await
                .expect("first acquire should succeed");
        }
        let result = tokio::time::timeout(Duration::from_millis(200), sem.acquire(team_id)).await;
        assert!(result.is_ok(), "acquire should succeed after permit is dropped");
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_acquire_isolates_concurrency_per_team() {
        let sem = TeamSemaphore::new(1);
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let _held_a = sem
            .acquire(team_a)
            .await
            .expect("team A acquire should succeed");
        let result = tokio::time::timeout(Duration::from_millis(200), sem.acquire(team_b)).await;
        assert!(result.is_ok(), "team B acquire should not be blocked by team A");
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_clone_shares_underlying_state() {
        let sem = TeamSemaphore::new(2);
        let team_id = Uuid::new_v4();
        let _handle = sem.get_or_create(team_id);
        let cloned = sem.clone();
        assert!(cloned.semaphores.contains_key(&team_id));
        assert!(Arc::ptr_eq(&sem.semaphores, &cloned.semaphores));
    }

    // R-teams-005 / T016：单租户退化模式下的全局并发上限测试

    #[tokio::test]
    async fn test_single_tenant_degraded_global_concurrency_limit() {
        use crate::common::constants::default_identity::DEFAULT_TEAM_ID;

        let sem = TeamSemaphore::new(3);

        let p1 = sem
            .acquire(DEFAULT_TEAM_ID)
            .await
            .expect("first acquire should succeed");
        let p2 = sem
            .acquire(DEFAULT_TEAM_ID)
            .await
            .expect("second acquire should succeed");
        let p3 = sem
            .acquire(DEFAULT_TEAM_ID)
            .await
            .expect("third acquire should succeed");

        let result_exhausted = sem.try_acquire(DEFAULT_TEAM_ID);
        assert!(
            result_exhausted.is_none(),
            "try_acquire should return None when single-tenant permits are exhausted"
        );

        drop(p3);
        let result_after_release = sem.try_acquire(DEFAULT_TEAM_ID);
        assert!(
            result_after_release.is_some(),
            "try_acquire should return Some after a permit is dropped"
        );

        drop(p1);
        drop(p2);
    }

    /// R-teams-005 / T016：单租户模式下信号量仅有一个 team_id 条目
    #[tokio::test]
    async fn test_single_tenant_degraded_uses_only_default_team_id_entry() {
        use crate::common::constants::default_identity::DEFAULT_TEAM_ID;

        let sem = TeamSemaphore::new(2);

        let _p1 = sem.acquire(DEFAULT_TEAM_ID).await.expect("first acquire");
        let _p2 = sem.acquire(DEFAULT_TEAM_ID).await.expect("second acquire");

        assert_eq!(
            sem.semaphores.len(),
            1,
            "single-tenant mode should have exactly one semaphore entry"
        );
        assert!(
            sem.semaphores.contains_key(&DEFAULT_TEAM_ID),
            "semaphore entry must be keyed by DEFAULT_TEAM_ID"
        );
    }

    // ========== T037/R-runtime-003：自适应模式测试 ==========

    /// with_adaptive 创建 Adaptive 模式实例
    #[test]
    fn test_with_adaptive_creates_adaptive_mode() {
        let params = AdaptiveParams {
            initial: 5,
            min_limit: 1,
            max_limit: 50,
            increase_threshold: 3,
        };
        let sem = TeamSemaphore::with_adaptive(params);
        assert_eq!(sem.mode(), "Adaptive");
        assert_eq!(sem.default_permits, 5);
    }

    /// AdaptiveParams::default 提供合理默认值
    #[test]
    fn test_adaptive_params_default() {
        let params = AdaptiveParams::default();
        assert_eq!(params.initial, 10);
        assert_eq!(params.min_limit, 1);
        assert_eq!(params.max_limit, 100);
        assert_eq!(params.increase_threshold, 10);
    }

    /// Adaptive 模式下首次 acquire 创建 Adaptive 条目
    #[tokio::test]
    async fn test_adaptive_acquire_creates_adaptive_entry() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 5,
            ..Default::default()
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire should succeed");
        assert!(sem.is_team_adaptive(team_id));
    }

    /// Adaptive 模式下 current_target 返回 controller.current_limit
    #[tokio::test]
    async fn test_adaptive_current_target_returns_controller_limit() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 10,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 5,
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");
        assert_eq!(sem.current_target(team_id), 10);
    }

    /// Adaptive 模式下 record_success 达到阈值后 target +1
    #[tokio::test]
    async fn test_adaptive_record_success_increases_target() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 10,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 3,
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");

        sem.record_success(team_id);
        sem.record_success(team_id);
        let target_after_3 = sem.record_success(team_id);
        assert_eq!(target_after_3, 11);
        assert_eq!(sem.current_target(team_id), 11);
    }

    /// Adaptive 模式下 record_failure 减半
    #[tokio::test]
    async fn test_adaptive_record_failure_halves_target() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 10,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 10,
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");

        let target = sem.record_failure(team_id);
        assert_eq!(target, 5);
        assert_eq!(sem.current_target(team_id), 5);
    }

    /// Adaptive 模式下连续失败 clamp 到 min_limit
    #[tokio::test]
    async fn test_adaptive_record_failure_clamps_to_min() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 4,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 10,
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");

        // 4 -> 2 -> 1 -> 1
        assert_eq!(sem.record_failure(team_id), 2);
        assert_eq!(sem.record_failure(team_id), 1);
        assert_eq!(sem.record_failure(team_id), 1);
    }

    /// Adaptive 模式下 record_success 在 max_limit 不再增加
    #[tokio::test]
    async fn test_adaptive_record_success_at_max_no_increase() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 9,
            min_limit: 1,
            max_limit: 10,
            increase_threshold: 1,
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");

        // 1 次成功即 +1（threshold=1）
        assert_eq!(sem.record_success(team_id), 10);
        // 已到 max，不再增加
        assert_eq!(sem.record_success(team_id), 10);
    }

    /// Adaptive 模式下 set_target 通过 AdaptiveSemaphore 调和许可
    #[tokio::test]
    async fn test_adaptive_target_changes_affect_permits() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 2,
            min_limit: 1,
            max_limit: 10,
            increase_threshold: 1,
        });
        let team_id = Uuid::new_v4();
        // 占用 2 个许可
        let p1 = sem.acquire(team_id).await.expect("p1");
        let p2 = sem.acquire(team_id).await.expect("p2");
        assert!(sem.try_acquire(team_id).is_none());

        // 1 次成功 → target +1 = 3
        sem.record_success(team_id);
        let p3 = sem.try_acquire(team_id);
        assert!(p3.is_some(), "should be able to acquire after target increase");
        assert!(sem.try_acquire(team_id).is_none());

        drop(p1);
        drop(p2);
        drop(p3);
    }

    /// Adaptive 模式下交替成功/失败验证 AIMD 行为
    #[tokio::test]
    async fn test_adaptive_alternating_success_failure() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 10,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 1,
        });
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");

        // 成功 → 11
        assert_eq!(sem.record_success(team_id), 11);
        // 失败 → 5
        assert_eq!(sem.record_failure(team_id), 5);
        // 成功 → 6
        assert_eq!(sem.record_success(team_id), 6);
        // 失败 → 3
        assert_eq!(sem.record_failure(team_id), 3);
    }

    /// Fixed 模式下 record_success/record_failure 为无操作（返回 available_permits）
    ///
    /// design.md：adaptive_enabled=false 时行为等同 Stage 1 之前，无 AIMD 调整
    #[tokio::test]
    async fn test_fixed_mode_record_success_failure_are_noops() {
        let sem = TeamSemaphore::new(5);
        let team_id = Uuid::new_v4();
        let _permit = sem.acquire(team_id).await.expect("acquire");

        // Fixed 模式：record_success/failure 不改变任何状态
        let target1 = sem.record_success(team_id);
        let target2 = sem.record_failure(team_id);
        // 都是 available_permits（5 - 1 = 4）
        assert_eq!(target1, 4);
        assert_eq!(target2, 4);
        assert_eq!(sem.current_target(team_id), 4);
    }

    /// Adaptive 模式下不同 team 各自独立维护 controller
    #[tokio::test]
    async fn test_adaptive_isolates_different_teams() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 10,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 1,
        });
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let _pa = sem.acquire(team_a).await.expect("a");
        let _pb = sem.acquire(team_b).await.expect("b");

        // team_a 成功 → 11；team_b 不受影响
        assert_eq!(sem.record_success(team_a), 11);
        assert_eq!(sem.current_target(team_b), 10);
        // team_b 失败 → 5；team_a 不受影响
        assert_eq!(sem.record_failure(team_b), 5);
        assert_eq!(sem.current_target(team_a), 11);
    }

    /// Adaptive 模式下 record_success/failure 同时影响 controller 和 AdaptiveSemaphore
    #[tokio::test]
    async fn test_adaptive_record_success_propagates_to_semaphore() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 1,
            min_limit: 1,
            max_limit: 5,
            increase_threshold: 1,
        });
        let team_id = Uuid::new_v4();
        let _p1 = sem.acquire(team_id).await.expect("p1");
        assert!(sem.try_acquire(team_id).is_none());

        // 1 次成功 → target=2，新许可可获取
        sem.record_success(team_id);
        let p2 = sem.try_acquire(team_id);
        assert!(p2.is_some(), "target increase should make new permit available");
    }

    /// Adaptive 模式下初次 acquire 之前的 record_success 也应工作
    #[tokio::test]
    async fn test_adaptive_record_success_without_prior_acquire() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 5,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 1,
        });
        let team_id = Uuid::new_v4();
        let target = sem.record_success(team_id);
        assert_eq!(target, 6);
        assert_eq!(sem.current_target(team_id), 6);
    }

    /// Adaptive 模式下初次 acquire 之前的 record_failure 也应工作
    #[tokio::test]
    async fn test_adaptive_record_failure_without_prior_acquire() {
        let sem = TeamSemaphore::with_adaptive(AdaptiveParams {
            initial: 10,
            min_limit: 1,
            max_limit: 100,
            increase_threshold: 10,
        });
        let team_id = Uuid::new_v4();
        let target = sem.record_failure(team_id);
        assert_eq!(target, 5);
    }

    /// 未存在的 team_id 调用 current_target 返回 default_permits
    #[test]
    fn test_current_target_unknown_team_returns_default() {
        let sem = TeamSemaphore::new(7);
        let team_id = Uuid::new_v4();
        assert_eq!(sem.current_target(team_id), 7);
    }

    /// 未存在的 team_id 调用 is_team_adaptive 返回模式标志
    #[test]
    fn test_is_team_adaptive_unknown_team_returns_mode_flag() {
        let fixed_sem = TeamSemaphore::new(5);
        let adaptive_sem = TeamSemaphore::with_adaptive(AdaptiveParams::default());
        let team_id = Uuid::new_v4();
        assert!(!fixed_sem.is_team_adaptive(team_id));
        assert!(adaptive_sem.is_team_adaptive(team_id));
    }

    /// 并发 get_or_create 同一 team_id 不会重复创建
    #[test]
    fn test_concurrent_get_or_create_no_duplicates() {
        use std::sync::Barrier;
        let sem = Arc::new(TeamSemaphore::new(5));
        let team_id = Uuid::new_v4();
        let barrier = Arc::new(Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let s = sem.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    s.get_or_create(team_id);
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        assert_eq!(sem.semaphores.len(), 1);
        assert!(sem.semaphores.contains_key(&team_id));
    }
}
