-- D7 修复：crawl 子任务 (crawl_id, url) 唯一索引（MySQL 方言）。
--
-- 审查修复（F1/H3）：MySQL/InnoDB 不允许对 TEXT 列直接建索引（ERROR 1170），
-- 原 `(crawl_id, url)` 唯一索引必然失败。改为 STORED 生成列
-- `url_hash = SHA2(url, 256)`（64 hex），对 `(crawl_id, url_hash)` 建唯一索引；
-- SHA-256 碰撞概率可忽略，唯一性语义与 url 等价，且不增加应用写入字段。
--
-- 审查修复（F2）：按状态优先级保留信息量最大的一行，清理被删任务的
-- scrape_results / tasks_backlog，并按受影响 crawl 重算聚合计数。
-- crawl_id IS NULL 的行在 MySQL 唯一索引中互不相等，普通任务不受约束。

CREATE TEMPORARY TABLE _crawl_dup_tasks AS
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

CREATE TEMPORARY TABLE _crawl_dup_crawls AS
SELECT DISTINCT crawl_id FROM _crawl_dup_tasks;

DELETE sr FROM scrape_results sr
JOIN _crawl_dup_tasks d ON sr.task_id = d.id;

DELETE tb FROM tasks_backlog tb
JOIN _crawl_dup_tasks d ON tb.task_id = d.id;

DELETE t FROM tasks t
JOIN _crawl_dup_tasks d ON t.id = d.id;

UPDATE crawls c
JOIN _crawl_dup_crawls d ON c.id = d.crawl_id
SET c.total_tasks = (SELECT COUNT(*) FROM tasks t WHERE t.crawl_id = c.id),
    c.completed_tasks = (
        SELECT COUNT(*) FROM tasks t
        WHERE t.crawl_id = c.id AND t.status = 'completed'
    ),
    c.failed_tasks = (
        SELECT COUNT(*) FROM tasks t
        WHERE t.crawl_id = c.id AND t.status = 'failed'
    );

DROP TEMPORARY TABLE _crawl_dup_tasks;
DROP TEMPORARY TABLE _crawl_dup_crawls;

ALTER TABLE tasks
    ADD COLUMN url_hash CHAR(64)
        GENERATED ALWAYS AS (SHA2(url, 256)) STORED;

CREATE UNIQUE INDEX idx_tasks_crawl_url_unique
    ON tasks (crawl_id, url_hash);
