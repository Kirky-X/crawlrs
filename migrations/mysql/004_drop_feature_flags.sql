-- 删除未使用的 feature_flags 与 feature_flag_overrides 表 (MySQL dialect)
-- Migration: drop_feature_flags
--
-- MySQL 无 DROP INDEX IF EXISTS，索引随 DROP TABLE 级联消失；
-- 显式 DROP INDEX 仅在表尚存时有意义，故直接按依赖序删表。

DROP TABLE IF EXISTS feature_flag_overrides;
DROP TABLE IF EXISTS feature_flags;
