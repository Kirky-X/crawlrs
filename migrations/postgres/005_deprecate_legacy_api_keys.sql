-- 标记旧 api_keys/scopes 表为弃用（保留数据只读）
-- Migration: deprecate_legacy_api_keys
--
-- R-key-lifecycle-003 / T030：garrison RBAC 接管认证/签发后，
-- api_keys 表 key_hash 字段弃用（id/team_id/key 列保留供 api_key_id→team_id
-- 映射查询与新 key 写入，见 design.md §3），scopes 表整体弃用。
-- 两表均保留数据用于运维核对与审计追溯。
--
-- 本 migration 加弃用注释 + deprecated_at 标记列，不删表不删列：
--   - COMMENT 标记表/列元数据为弃用（DB 工具可见，pg_dump/pg_admin/psql \d+ 会显示）
--   - deprecated_at 列记录弃用时间点，便于运维查询识别弃用数据
--   - 保留所有历史行，便于审计追溯（spec.md §R-key-lifecycle-003）
--
-- 幂等性（CI 通过 `for f in migrations/*.sql; do psql -f "$f"; done` 重复执行）：
--   - ALTER TABLE ADD COLUMN IF NOT EXISTS：列已存在则跳过
--   - UPDATE ... WHERE deprecated_at IS NULL：仅填充未标记的行，重复执行无副作用
--   - COMMENT ON TABLE/COLUMN：覆盖式更新，重复执行不报错
--   - DISABLE/ENABLE TRIGGER：幂等（已禁用时再 DISABLE 是 no-op，已启用时 ENABLE 是 no-op）
--
-- 触发器处理（架构审查 M-001 / 性能审查 M1 修复）：
--   - 001 schema 在 api_keys/scopes 上挂了 trigger_update_*_updated_at BEFORE UPDATE 触发器
--   - 若不禁用，UPDATE 会污染 updated_at 字段（覆盖原值），破坏审计语义
--   - 显式 DISABLE/ENABLE 包裹 UPDATE，避免触发器副作用
--
-- Spec:
--   - R-key-lifecycle-003：旧表弃用（保留只读）
--   - tasks.md T030

-- ==========================================================
-- 1. api_keys 表（部分弃用：key_hash 弃用，id/team_id/key 保留）
-- ==========================================================

-- 1.1 添加 deprecated_at 列（NULL 默认；通过下方 UPDATE 一次性填充历史行）
--     不使用 DEFAULT NOW() 是为了让 UPDATE 显式标记历史行。
--     api_keys 表的 deprecated_at 语义：仅历史行被标记为弃用时间点；
--     T027 之后由 insert_api_key_mapping 写入的新 mapping 行 deprecated_at 为 NULL
--     （属正常写入，非异常——见 design.md §3 api_keys 表保留供映射查询）。
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS deprecated_at TIMESTAMPTZ;

-- 1.2 一次性填充所有历史行的 deprecated_at
--     禁用触发器避免污染 updated_at 审计字段（架构审查 M-001 / 性能审查 M1 修复）
--     WHERE deprecated_at IS NULL 确保重复执行时跳过已标记行（幂等）
ALTER TABLE api_keys DISABLE TRIGGER trigger_update_api_keys_updated_at;
UPDATE api_keys SET deprecated_at = NOW() WHERE deprecated_at IS NULL;
ALTER TABLE api_keys ENABLE TRIGGER trigger_update_api_keys_updated_at;
DO $$ BEGIN RAISE NOTICE 'migration 005: api_keys 标记弃用历史行数 %', (SELECT count(*) FROM api_keys WHERE deprecated_at IS NOT NULL); END $$;

-- 1.3 表级 COMMENT：标记 api_keys 表为部分弃用（架构审查 HIGH-1 修复）
--     design.md §3 明确 id/team_id/key 列保留供 api_key_id→team_id 映射查询与新 key 写入
COMMENT ON TABLE api_keys IS 'PARTIALLY DEPRECATED (0.2.0): garrison RBAC 接管认证/签发。key_hash 字段弃用（garrison 接管 key 哈希存储，新 key 此字段为 NULL）；id/team_id/key 列保留供 api_key_id→team_id 映射查询与新 key 写入（见 design.md §3）。待全量重签完成后可整体移除。详见 specmark/changes/garrison-auth-migration。';

-- 1.4 列级 COMMENT：key_hash 字段弃用（安全审查 L-001 修复：移除内部存储架构细节）
COMMENT ON COLUMN api_keys.key_hash IS 'DEPRECATED (0.2.0): garrison 接管 key 哈希存储，新 key 此字段为 NULL。保留列供历史数据只读核对。';

-- 1.5 列级 COMMENT：deprecated_at 字段语义说明（架构审查 MEDIUM-1 修复：区分两表语义）
COMMENT ON COLUMN api_keys.deprecated_at IS '历史标记列。migration 005 执行前的历史行通过 UPDATE 填充 NOW()；migration 005 执行后由 T027 insert_api_key_mapping 写入的新 mapping 行保持 NULL（属正常写入，非异常）。scopes 表的 deprecated_at 语义不同（scopes 表整体弃用，新行 NULL 即异常）。';

-- ==========================================================
-- 2. scopes 表（整体弃用：所有列弃用，不再被业务读写）
-- ==========================================================

-- 2.1 添加 deprecated_at 列
ALTER TABLE scopes ADD COLUMN IF NOT EXISTS deprecated_at TIMESTAMPTZ;

-- 2.2 一次性填充所有历史行的 deprecated_at（禁用触发器，理由同 1.2）
ALTER TABLE scopes DISABLE TRIGGER trigger_update_scopes_updated_at;
UPDATE scopes SET deprecated_at = NOW() WHERE deprecated_at IS NULL;
ALTER TABLE scopes ENABLE TRIGGER trigger_update_scopes_updated_at;
DO $$ BEGIN RAISE NOTICE 'migration 005: scopes 标记弃用历史行数 %', (SELECT count(*) FROM scopes WHERE deprecated_at IS NOT NULL); END $$;

-- 2.3 表级 COMMENT：标记整表为弃用
COMMENT ON TABLE scopes IS 'DEPRECATED (0.2.0): garrison RBAC 接管权限/作用域管理。权限串改为 crawlrs:read/write/admin，由 garrison CrawlrsGarrisonInterface 返回。表保留供历史数据迁移/查询，业务代码不再读写。待全量重签完成后可移除。';

-- 2.4 列级 COMMENT：deprecated_at 字段语义说明（scopes 表整体弃用，新行 NULL 即异常）
COMMENT ON COLUMN scopes.deprecated_at IS '弃用时间点。历史行通过 migration 005 一次性填充 NOW()；新行（不应再有 INSERT）保持 NULL 便于识别异常插入。';
