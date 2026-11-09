-- D7 修复：crawl 子任务 (crawl_id, url) 唯一索引（MySQL 方言）。
--
-- 审查修复（F1/H3）：MySQL/InnoDB 不允许对 TEXT 列直接建索引（ERROR 1170），
-- 原 `(crawl_id, url)` 唯一索引必然失败。改为生成列
-- `url_hash = UNHEX(SHA2(url, 256))`（BINARY(32)），对 `(crawl_id, url_hash)`
-- 建唯一索引；SHA-256 碰撞概率可忽略，唯一性语义与 url 等价，
-- 且不增加应用写入字段。
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
            PARTITION BY crawl_id, CAST(url AS BINARY)
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

-- MySQL DDL 隐式提交：用 information_schema + 动态 SQL 幂等化，
-- 避免部分失败后重跑因列/索引已存在而阻塞（审查 N6）。
--
-- 生成列用 VIRTUAL + BINARY(32)：
--   * STORED 列会让 ALTER TABLE 走 COPY 算法、重建整表且禁止并发 DML；
--     VIRTUAL 列 ADD COLUMN 为 INSTANT，建二级索引为 INPLACE 并允许写入。
--   * BINARY(32) 存储 SHA-256 原始字节；utf8mb4 CHAR(64) 的哈希列
--     最多占 256 字节，BINARY(32) 固定 32 字节。
-- 既有定义的列/索引在 DDL 之后重新校验；不一致时用一条引用不存在表的
-- 语句强制失败（8.0 全版本有效；SIGNAL 不能用于 prepared statement，
-- CHECK 约束在 8.0.16 之前不执行）。不能按名字静默跳过（审查 M-4）。
--
-- 兼容 HEAD 版 009 的 legacy 形状（CHAR(64) STORED + SHA2 hex）：
-- 部分应用（列已建、索引未建）重跑时接受该形状并继续建索引，避免迁移死锁。
SET @col_exists = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
);
SET @ddl = IF(
    @col_exists = 0,
    'ALTER TABLE tasks ADD COLUMN url_hash BINARY(32) GENERATED ALWAYS AS (UNHEX(SHA2(url, 256))) VIRTUAL',
    'SELECT 1'
);
PREPARE add_url_hash FROM @ddl;
EXECUTE add_url_hash;
DEALLOCATE PREPARE add_url_hash;

SET @col_ok = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
      AND (
          (data_type = 'binary' AND column_type = 'binary(32)'
           AND extra LIKE '%VIRTUAL GENERATED%'
           AND generation_expression = 'unhex(sha2(`url`,256))')
          OR
          (data_type = 'char' AND column_type = 'char(64)'
           AND extra LIKE '%STORED GENERATED%'
           AND generation_expression = 'sha2(`url`,256)')
      )
);
SET @assert_col = IF(
    @col_ok = 1,
    'SELECT 1',
    'SELECT * FROM _crawlrs_migration_error_url_hash_definition_mismatch'
);
PREPARE assert_col FROM @assert_col;
EXECUTE assert_col;
DEALLOCATE PREPARE assert_col;

SET @idx_exists = (
    SELECT COUNT(*) FROM information_schema.statistics
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND index_name = 'idx_tasks_crawl_url_unique'
);
SET @ddl_idx = IF(
    @idx_exists = 0,
    'CREATE UNIQUE INDEX idx_tasks_crawl_url_unique ON tasks (crawl_id, url_hash)',
    'SELECT 1'
);
PREPARE add_url_unique FROM @ddl_idx;
EXECUTE add_url_unique;
DEALLOCATE PREPARE add_url_unique;

SET @idx_ok = (
    SELECT COUNT(*) FROM (
        SELECT non_unique,
               GROUP_CONCAT(column_name ORDER BY seq_in_index) AS cols,
               SUM(sub_part IS NOT NULL) AS prefixed
        FROM information_schema.statistics
        WHERE table_schema = DATABASE()
          AND table_name = 'tasks'
          AND index_name = 'idx_tasks_crawl_url_unique'
        GROUP BY non_unique
    ) s
    WHERE s.non_unique = 0
      AND s.cols = 'crawl_id,url_hash'
      AND s.prefixed = 0
);
SET @assert_idx = IF(
    @idx_ok = 1,
    'SELECT 1',
    'SELECT * FROM _crawlrs_migration_error_url_hash_index_mismatch'
);
PREPARE assert_idx FROM @assert_idx;
EXECUTE assert_idx;
DEALLOCATE PREPARE assert_idx;
