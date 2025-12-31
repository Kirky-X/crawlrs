#!/bin/bash

# 重置所有卡住的任务
docker exec docker-postgres-1 psql -U crawlrs -d crawlrs_test -c "UPDATE tasks SET status = 'queued', lock_token = NULL, lock_expires_at = NULL, started_at = NULL, attempt_count = 0 WHERE status = 'active';"