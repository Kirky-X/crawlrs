// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 优先级队列（公平调度）
//!
//! 基于 `BinaryHeap` 实现 `max-heap`：`effective_priority` 大者先出。
//! 等待时长通过 `effective = base + waited_secs / aging_factor` 提升优先级，
//! 避免低优先级任务在长期高优先级流量下被饿死。
//!
//! 设计参考：crawl4ai `async_dispatcher.py` 优先级调度逻辑。

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::Instant;

/// 默认老化因子：每等待 10 秒，优先级提升 1.0
const DEFAULT_AGING_FACTOR: f64 = 10.0;

/// 调度任务
///
/// `effective_priority` 由 `base_priority` 加上等待时长贡献组成：
/// `effective = base_priority + waited_secs / aging_factor`
///
/// `cached_priority` 在构造/重入队时计算一次，保证 `Ord` 契约的稳定性。
/// 若需反映最新等待时长，调用 `refresh_priority()` 后重新入队。
#[derive(Debug, Clone)]
pub struct ScheduledTask {
    /// 业务侧赋予的静态优先级（数值大者优先）
    pub base_priority: u32,
    /// 入队时刻；用于计算等待时长
    pub enqueued_at: Instant,
    /// 老化因子：值越小，等待时长对优先级的提升越显著
    aging_factor: f64,
    /// 缓存的有效优先级（构造时计算一次，保证 Ord 稳定性）
    cached_priority: f64,
}

impl ScheduledTask {
    /// 创建任务，使用默认老化因子
    pub fn new(base_priority: u32) -> Self {
        let enqueued_at = Instant::now();
        Self {
            base_priority,
            enqueued_at,
            aging_factor: DEFAULT_AGING_FACTOR,
            cached_priority: base_priority as f64,
        }
    }

    /// 创建任务并指定入队时刻（测试与重排场景使用）
    pub fn with_enqueued_at(base_priority: u32, enqueued_at: Instant) -> Self {
        let waited_secs = enqueued_at.elapsed().as_secs_f64();
        let cached_priority = base_priority as f64 + waited_secs / DEFAULT_AGING_FACTOR;
        Self {
            base_priority,
            enqueued_at,
            aging_factor: DEFAULT_AGING_FACTOR,
            cached_priority,
        }
    }

    /// 创建任务并显式指定老化因子
    pub fn with_aging_factor(base_priority: u32, aging_factor: f64) -> Self {
        assert!(aging_factor > 0.0, "aging_factor must be positive");
        let enqueued_at = Instant::now();
        Self {
            base_priority,
            enqueued_at,
            aging_factor,
            cached_priority: base_priority as f64,
        }
    }

    /// 创建任务并显式指定全部字段（测试与精细重排场景使用）
    pub fn with_all(base_priority: u32, enqueued_at: Instant, aging_factor: f64) -> Self {
        assert!(aging_factor > 0.0, "aging_factor must be positive");
        let waited_secs = enqueued_at.elapsed().as_secs_f64();
        let cached_priority = base_priority as f64 + waited_secs / aging_factor;
        Self {
            base_priority,
            enqueued_at,
            aging_factor,
            cached_priority,
        }
    }

    /// 计算有效优先级（动态，含实时等待时长；仅供外部展示/调试用）
    ///
    /// `effective = base_priority + waited_secs / aging_factor`
    pub fn effective_priority(&self) -> f64 {
        let waited_secs = self.enqueued_at.elapsed().as_secs_f64();
        self.base_priority as f64 + waited_secs / self.aging_factor
    }

    /// 刷新缓存优先级（重入队前调用，反映最新等待时长）
    pub fn refresh_priority(&mut self) {
        self.cached_priority = self.effective_priority();
    }
}

/// `BinaryHeap` 要求 `Ord`；按 `cached_priority` 降序排列（max-heap）
///
/// 使用构造/刷新时缓存的 `cached_priority` 而非实时计算，
/// 保证 `Ord::cmp` 的稳定性契约（同一对元素的比较结果不随时间变化）。
impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // f64 不实现 Ord，使用 partial_cmp 并对 NaN 做兜底
        // （cached_priority 由有限数构造，正常场景不会产生 NaN）
        self.cached_priority
            .partial_cmp(&other.cached_priority)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ScheduledTask {
    fn eq(&self, other: &Self) -> bool {
        self.cached_priority == other.cached_priority
    }
}

impl Eq for ScheduledTask {}

/// 优先级队列
///
/// 包装 `BinaryHeap<ScheduledTask>`，对外暴露 `push`/`pop`/`peek` 等。
pub struct PriorityQueue {
    heap: BinaryHeap<ScheduledTask>,
}

impl PriorityQueue {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    /// 创建指定容量的队列
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            heap: BinaryHeap::with_capacity(capacity),
        }
    }

    /// 入队
    pub fn push(&mut self, task: ScheduledTask) {
        self.heap.push(task);
    }

    /// 出队：返回当前 `effective_priority` 最高的任务
    pub fn pop(&mut self) -> Option<ScheduledTask> {
        self.heap.pop()
    }

    /// 查看队首（不出队）
    pub fn peek(&self) -> Option<&ScheduledTask> {
        self.heap.peek()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 辅助：构造一个"过去某时刻入队"的任务，模拟长等待
    fn task_enqueued_ago(base_priority: u32, secs_ago: u64) -> ScheduledTask {
        let enqueued_at = Instant::now() - Duration::from_secs(secs_ago);
        ScheduledTask::with_enqueued_at(base_priority, enqueued_at)
    }

    // ========== 基础出队顺序 ==========

    #[test]
    fn test_pop_returns_highest_base_priority_when_no_aging() {
        let mut q = PriorityQueue::new();
        q.push(ScheduledTask::new(1));
        q.push(ScheduledTask::new(5));
        q.push(ScheduledTask::new(3));

        // 三个任务几乎同时入队，等待时长差异忽略不计，按 base 排序
        let first = q.pop().unwrap();
        let second = q.pop().unwrap();
        let third = q.pop().unwrap();
        assert_eq!(first.base_priority, 5);
        assert_eq!(second.base_priority, 3);
        assert_eq!(third.base_priority, 1);
    }

    #[test]
    fn test_pop_returns_none_when_empty() {
        let mut q = PriorityQueue::new();
        assert!(q.is_empty());
        assert!(q.pop().is_none());
    }

    #[test]
    fn test_peek_does_not_remove() {
        let mut q = PriorityQueue::new();
        q.push(ScheduledTask::new(10));
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap().base_priority, 10);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut q = PriorityQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);

        q.push(ScheduledTask::new(1));
        q.push(ScheduledTask::new(2));
        assert_eq!(q.len(), 2);
        assert!(!q.is_empty());

        q.pop();
        assert_eq!(q.len(), 1);
    }

    // ========== 老化机制：低优先级任务等待足够久后反超 ==========

    #[test]
    fn test_aging_promotes_low_priority_task_after_long_wait() {
        // 低优先级任务 60 秒前入队：base=1 + 60/10 = 7.0
        // 高优先级任务此刻入队：base=5 + 0/10 = 5.0
        // 低优先级任务应先出队（不饿死）
        let mut q = PriorityQueue::new();
        q.push(task_enqueued_ago(1, 60));
        q.push(ScheduledTask::new(5));

        let first = q.pop().unwrap();
        assert_eq!(
            first.base_priority, 1,
            "等待 60s 的低优先级任务（effective≈7）应先于新入队的高优先级任务（effective≈5）出队"
        );
    }

    #[test]
    fn test_aging_does_not_starve_high_priority_short_wait() {
        // 高优先级任务短暂等待（base=10 + 0/10 = 10）
        // 低优先级任务短暂等待（base=1 + 1/10 = 1.1）
        // 高优先级先出
        let mut q = PriorityQueue::new();
        q.push(task_enqueued_ago(10, 0));
        q.push(task_enqueued_ago(1, 1));

        let first = q.pop().unwrap();
        assert_eq!(first.base_priority, 10);
    }

    #[test]
    fn test_aging_cross_over_boundary() {
        // base=1 + 90/10 = 10.0  vs  base=10 + 0/10 = 10.0
        // 边界：相等时 BinaryHeap 顺序未指定，但都能正常出队
        // 用更严格的反超：91s 让低优先级 = 10.1 > 10
        let mut q = PriorityQueue::new();
        q.push(task_enqueued_ago(1, 91));
        q.push(ScheduledTask::new(10));

        let first = q.pop().unwrap();
        assert_eq!(first.base_priority, 1);
    }

    // ========== 自定义 aging_factor ==========

    #[test]
    fn test_custom_aging_factor_accelerates_promotion() {
        // 同一入队时刻（5s 前），不同 aging_factor：
        //   低优先级 aging=1.0：base=1 + 5/1   = 6.0
        //   高优先级 aging=10.0：base=5 + 5/10 = 5.5
        // 小 aging_factor 加速提升，低优先级反超
        let mut q = PriorityQueue::new();
        let enqueued_at = Instant::now() - Duration::from_secs(5);
        q.push(ScheduledTask::with_all(1, enqueued_at, 1.0));
        q.push(ScheduledTask::with_all(5, enqueued_at, 10.0));

        let first = q.pop().unwrap();
        assert_eq!(first.base_priority, 1);
    }

    #[test]
    fn test_custom_aging_factor_slows_promotion() {
        // 同一入队时刻（60s 前），同一 aging_factor=100：
        //   低优先级：base=1 + 60/100 = 1.6
        //   高优先级：base=5 + 60/100 = 5.6
        // 大 aging_factor 减缓提升，高优先级保持优势
        let mut q = PriorityQueue::new();
        let enqueued_at = Instant::now() - Duration::from_secs(60);
        q.push(ScheduledTask::with_all(1, enqueued_at, 100.0));
        q.push(ScheduledTask::with_all(5, enqueued_at, 100.0));

        let first = q.pop().unwrap();
        assert_eq!(first.base_priority, 5);
    }

    #[test]
    #[should_panic(expected = "aging_factor must be positive")]
    fn test_zero_aging_factor_panics() {
        let _ = ScheduledTask::with_aging_factor(1, 0.0);
    }

    // ========== 公平性回归：长流量下低优先级必能出队 ==========

    #[test]
    fn test_low_priority_eventually_dispatched_under_high_priority_stream() {
        // 模拟：低优先级任务入队后，持续有高优先级新任务涌入。
        // 让低优先级任务的 enqueued_at 足够久（>10*差距），保证其反超。
        let mut q = PriorityQueue::new();
        let low = task_enqueued_ago(1, 200); // base 1 + 200/10 = 21
        q.push(low);

        // 持续推入 base=10 的新任务
        for _ in 0..1000 {
            q.push(ScheduledTask::new(10)); // effective ≈ 10
        }

        // 队首必然是低优先级任务
        let first = q.pop().unwrap();
        assert_eq!(first.base_priority, 1);
    }

    // ========== effective_priority 单调性 ==========

    #[test]
    fn test_effective_priority_increases_with_wait_time() {
        let task = task_enqueued_ago(5, 0);
        let p0 = task.effective_priority();
        std::thread::sleep(Duration::from_millis(50));
        let p1 = task.effective_priority();
        assert!(p1 > p0, "等待后 effective_priority 必须上升");
        assert!(p1 - p0 < 1.0, "50ms 内提升幅度应远小于 1.0");
    }
}
