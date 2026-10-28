-- 为 acquire_next 查询添加 partial indexes
-- Migration: add_acquire_next_indexes
--
-- 这些 partial indexes 专为 TaskRepository::acquire_next 的两步查询设计：
--   1. 优先获取 status='queued' 的任务（正常路径）
--   2. 回退获取 status='active' AND lock_expires_at < NOW() 的任务（恢复路径）
--
-- partial index 只索引符合 WHERE 条件的行，相比全表索引：
--   - 写入开销更低（只有部分行需要更新索引）
--   - 查询性能更优（planner 能直接使用索引返回有序结果，无需 Sort）
--
-- 性能对比（60K 行实测，见性能审查报告）：
--   无索引 + OR + CASE WHEN:  85.677 ms (Seq Scan + external Sort 2592kB)
--   partial indexes + 拆分:   0.933 ms (Index Scan with LIMIT 1, 92x 加速)

-- 正常路径：优先获取 queued 任务，按 (priority, created_at) 有序
-- 替代原本依赖 idx_tasks_status 的全表扫描
CREATE INDEX IF NOT EXISTS idx_tasks_acquire_queued
    ON tasks (priority ASC, created_at ASC)
    WHERE status = 'queued';

-- 恢复路径：lock 过期的 active 任务，按 (priority, created_at) 有序
-- 仅索引 active 且有 lock_expires_at 的行（排除 NULL 和未过期）
CREATE INDEX IF NOT EXISTS idx_tasks_acquire_stale
    ON tasks (priority ASC, created_at ASC)
    WHERE status = 'active' AND lock_expires_at IS NOT NULL;
