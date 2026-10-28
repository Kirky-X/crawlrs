-- 删除未使用的 feature_flags 与 feature_flag_overrides 表 (SQLite dialect)
-- Migration: drop_feature_flags

DROP INDEX IF EXISTS idx_feature_flag_overrides_feature_flag_id;
DROP INDEX IF EXISTS idx_feature_flag_overrides_api_key_id;
DROP TABLE IF EXISTS feature_flag_overrides;

DROP INDEX IF EXISTS idx_feature_flags_name;
DROP INDEX IF EXISTS idx_feature_flags_enabled;
DROP TABLE IF EXISTS feature_flags;
