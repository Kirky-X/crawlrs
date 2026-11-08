-- D7 修复：crawl 子任务 (crawl_id, url) 唯一索引（SQLite 方言）。
--
-- 审查修复（F2）：按状态优先级保留信息量最大的一行（completed > failed >
-- cancelled > active > queued），同步清理被删任务的 scrape_results /
-- tasks_backlog（无外键，留孤儿），并按受影响 crawl 重算聚合计数。
-- crawl_id IS NULL 的普通任务不受约束。

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
)
WHERE rn > 1;

CREATE TEMP TABLE _crawl_dup_crawls AS
SELECT DISTINCT crawl_id FROM _crawl_dup_tasks;

DELETE FROM scrape_results
WHERE task_id IN (SELECT id FROM _crawl_dup_tasks);

DELETE FROM tasks_backlog
WHERE task_id IN (SELECT id FROM _crawl_dup_tasks);

DELETE FROM tasks
WHERE id IN (SELECT id FROM _crawl_dup_tasks);

UPDATE crawls
SET total_tasks = (SELECT COUNT(*) FROM tasks t WHERE t.crawl_id = crawls.id),
    completed_tasks = (
        SELECT COUNT(*) FROM tasks t
        WHERE t.crawl_id = crawls.id AND t.status = 'completed'
    ),
    failed_tasks = (
        SELECT COUNT(*) FROM tasks t
        WHERE t.crawl_id = crawls.id AND t.status = 'failed'
    )
WHERE id IN (SELECT crawl_id FROM _crawl_dup_crawls);

DROP TABLE _crawl_dup_tasks;
DROP TABLE _crawl_dup_crawls;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_crawl_url_unique
    ON tasks (crawl_id, url)
    WHERE crawl_id IS NOT NULL;
