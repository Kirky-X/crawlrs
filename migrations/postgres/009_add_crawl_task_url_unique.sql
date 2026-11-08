-- D7 修复：crawl 子任务 (crawl_id, url) 唯一索引，封堵 Bloom→DB→入队
-- 并发窗口的重复入库（E2E SC-09：同一 URL 双任务入库）。
--
-- 审查修复（F2）：去重不再简单保留最小 id——
--   1. 按状态优先级保留"信息量最大"的一行（completed > failed > cancelled
--      > active > queued），避免保留 queued 行、删掉已有结果的 completed 行；
--   2. 同步清理被删任务的 scrape_results / tasks_backlog（无外键，留孤儿）；
--   3. 按受影响 crawl 重算 total/completed/failed 计数，修复聚合漂移。
-- crawl_id IS NULL 的普通任务不受约束（PostgreSQL 语义下 NULL 互不相等）。

CREATE TEMP TABLE _crawl_dup_tasks AS
SELECT id, crawl_id
FROM (
    SELECT
        id,
        crawl_id,
        ROW_NUMBER() OVER (
            PARTITION BY crawl_id, url
            ORDER BY CASE status
                         WHEN 'completed' THEN 5
                         WHEN 'failed' THEN 4
                         WHEN 'cancelled' THEN 3
                         WHEN 'active' THEN 2
                         ELSE 1
                     END DESC,
                     created_at ASC,
                     id ASC
        ) AS rn
    FROM tasks
    WHERE crawl_id IS NOT NULL
) ranked
WHERE rn > 1;

CREATE TEMP TABLE _crawl_dup_crawls AS
SELECT DISTINCT crawl_id FROM _crawl_dup_tasks;

DELETE FROM scrape_results
WHERE task_id IN (SELECT id FROM _crawl_dup_tasks);

DELETE FROM tasks_backlog
WHERE task_id IN (SELECT id FROM _crawl_dup_tasks);

DELETE FROM tasks
WHERE id IN (SELECT id FROM _crawl_dup_tasks);

UPDATE crawls c
SET total_tasks = (SELECT COUNT(*) FROM tasks t WHERE t.crawl_id = c.id),
    completed_tasks = (
        SELECT COUNT(*) FROM tasks t
        WHERE t.crawl_id = c.id AND t.status = 'completed'
    ),
    failed_tasks = (
        SELECT COUNT(*) FROM tasks t
        WHERE t.crawl_id = c.id AND t.status = 'failed'
    )
WHERE c.id IN (SELECT crawl_id FROM _crawl_dup_crawls);

DROP TABLE _crawl_dup_tasks;
DROP TABLE _crawl_dup_crawls;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_crawl_url_unique
    ON tasks (crawl_id, url)
    WHERE crawl_id IS NOT NULL;
