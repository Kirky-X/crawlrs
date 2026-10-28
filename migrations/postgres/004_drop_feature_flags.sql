-- 删除未使用的 feature_flags 与 feature_flag_overrides 表
-- Migration: drop_feature_flags
--
-- 架构 MEDIUM-3：FeatureFlag 表为 dead schema — 在 src/ 下无任何 Rust 代码引用
-- （无 model、repository、service、handler 引用），仅残留在 migrations 与权限配置中。
-- 业务上已确认不再使用 feature flag 功能，因此删除表 + 索引 + 触发器。
--
-- 安全说明：
--   - 所有 DROP 语句均使用 IF EXISTS 保证幂等（重复执行不报错）
--   - 先 DROP 子表（feature_flag_overrides）再 DROP 父表（feature_flags），
--     避免外键依赖（虽然本 schema 未声明 FK，但保持顺序以防未来添加）
--   - DROP TABLE 会自动级联删除表上的所有触发器，无需显式 DROP TRIGGER

-- 1. 删除 feature_flag_overrides 表（依赖 feature_flags）
DROP INDEX IF EXISTS idx_feature_flag_overrides_feature_flag_id;
DROP INDEX IF EXISTS idx_feature_flag_overrides_api_key_id;
DROP TABLE IF EXISTS feature_flag_overrides;

-- 2. 删除 feature_flags 表
-- （DROP TABLE 自动级联删除 idx_feature_flags_* 与 trigger_update_feature_flags_updated_at）
DROP INDEX IF EXISTS idx_feature_flags_name;
DROP INDEX IF EXISTS idx_feature_flags_enabled;
DROP TABLE IF EXISTS feature_flags;
