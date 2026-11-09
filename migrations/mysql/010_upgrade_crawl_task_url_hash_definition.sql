-- 010：把旧版 009 的 url_hash 定义升级为 VIRTUAL BINARY(32)（MySQL 方言）。
--
-- 背景：历史版本（e7252fc6 版 009）使用 `CHAR(64) STORED` 生成列；新版 009
-- 已改为 VIRTUAL BINARY(32)。已应用旧 009 的库按版本号跳过新版 009，
-- 需要本迁移做一次定义升级。
--
-- 代价与窗口：DROP INDEX 后可走 INSTANT/INPLACE 删列（MySQL 8.0.29+ INSTANT，
-- 早期 8.0 INPLACE LOCK=NONE）；主要成本是建索引前的去重全表扫描/排序，
-- 加上 CREATE UNIQUE INDEX 的在线构建 I/O 与变更日志。删旧索引到建新索引
-- 之间存在唯一性空窗；**执行前必须
-- 停止所有应用写入副本**，否则并发插入重复 URL 会让建索引报 1062。本迁移
-- 在建索引前重新执行与 009 等价的去重/孤儿清理/计数重算，用于收敛历史重复；
-- 但不能替代停机。迁移按步骤幂等：中途失败重跑会从当前状态继续收敛。
-- 非旧/新两种预期定义（例如 CHAR(20) 或错误表达式）会显式失败，不静默跳过：
-- 失败方式为标量子查询返回多行（ERROR 1242），全版本有效且无表名碰撞。

SET @legacy = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
      AND data_type = 'char'
      AND column_type = 'char(64)'
      AND extra LIKE '%STORED GENERATED%'
      AND generation_expression = 'sha2(`url`,256)'
);
SET @modern = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
      AND data_type = 'binary'
      AND column_type = 'binary(32)'
      AND extra LIKE '%VIRTUAL GENERATED%'
      AND generation_expression = 'unhex(sha2(`url`,256))'
);
SET @col_exists = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
);
SET @unexpected = @col_exists - @legacy - @modern;
SET @assert_shape = IF(
    @unexpected = 0,
    'SELECT 1',
    'SELECT (SELECT 1 UNION SELECT 2)'
);
PREPARE assert_shape FROM @assert_shape;
EXECUTE assert_shape;
DEALLOCATE PREPARE assert_shape;

-- 旧列存在时先删依赖它的唯一索引，再删列；缺失索引时跳过。
SET @idx_exists = (
    SELECT COUNT(DISTINCT index_name) FROM information_schema.statistics
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND index_name = 'idx_tasks_crawl_url_unique'
);
SET @drop_idx = IF(
    @legacy = 1 AND @idx_exists = 1,
    'ALTER TABLE tasks DROP INDEX idx_tasks_crawl_url_unique',
    'SELECT 1'
);
PREPARE drop_idx FROM @drop_idx;
EXECUTE drop_idx;
DEALLOCATE PREPARE drop_idx;

SET @drop_col = IF(
    @legacy = 1,
    'ALTER TABLE tasks DROP COLUMN url_hash',
    'SELECT 1'
);
PREPARE drop_col FROM @drop_col;
EXECUTE drop_col;
DEALLOCATE PREPARE drop_col;

-- 旧列已删（或从未存在）时创建新定义；已是新定义则跳过。
SET @col_exists_now = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
);
SET @add_col = IF(
    @col_exists_now = 0,
    'ALTER TABLE tasks ADD COLUMN url_hash BINARY(32) GENERATED ALWAYS AS (UNHEX(SHA2(url, 256))) VIRTUAL',
    'SELECT 1'
);
PREPARE add_col FROM @add_col;
EXECUTE add_col;
DEALLOCATE PREPARE add_col;

SET @modern_now = (
    SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND column_name = 'url_hash'
      AND data_type = 'binary'
      AND column_type = 'binary(32)'
      AND extra LIKE '%VIRTUAL GENERATED%'
      AND generation_expression = 'unhex(sha2(`url`,256))'
);
SET @assert_modern = IF(
    @modern_now = 1,
    'SELECT 1',
    'SELECT (SELECT 1 UNION SELECT 2)'
);
PREPARE assert_modern FROM @assert_modern;
EXECUTE assert_modern;
DEALLOCATE PREPARE assert_modern;

-- 建索引前重新去重：旧 009 的“去重→建索引”并发窗口可能留下重复行，
-- 或部分应用状态下索引从未建成。分组键与唯一索引同为 URL 字节精确。
-- 先清理同名临时表，保证同一会话内失败后手工重跑也可执行。
DROP TEMPORARY TABLE IF EXISTS _crawl_dup_tasks;
DROP TEMPORARY TABLE IF EXISTS _crawl_dup_crawls;
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

-- 唯一索引：缺失则建；已存在则校验唯一、列序、无前缀长度。
SET @idx_exists_now = (
    SELECT COUNT(DISTINCT index_name) FROM information_schema.statistics
    WHERE table_schema = DATABASE()
      AND table_name = 'tasks'
      AND index_name = 'idx_tasks_crawl_url_unique'
);
SET @create_idx = IF(
    @idx_exists_now = 0,
    'CREATE UNIQUE INDEX idx_tasks_crawl_url_unique ON tasks (crawl_id, url_hash)',
    'SELECT 1'
);
PREPARE create_idx FROM @create_idx;
EXECUTE create_idx;
DEALLOCATE PREPARE create_idx;

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
    'SELECT (SELECT 1 UNION SELECT 2)'
);
PREPARE assert_idx FROM @assert_idx;
EXECUTE assert_idx;
DEALLOCATE PREPARE assert_idx;
