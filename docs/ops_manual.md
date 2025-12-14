
# crawlrs 运维手册

## 1. 部署架构

### 1.1 组件依赖
- **PostgreSQL**: 主数据存储 (Tasks, Crawls, Webhooks)
- **Redis**: 任务队列与缓存
- **Application Service**: crawlrs API 服务

### 1.2 环境变量
| 变量名 | 说明 | 示例 |
|--------|------|------|
| `DATABASE_URL` | PostgreSQL 连接字符串 | `postgres://user:pass@localhost:5432/crawlrs_db` |
| `REDIS_URL` | Redis 连接字符串 | `redis://localhost:6379` |
| `APP_ENVIRONMENT` | 运行环境 | `production` |
| `RUST_LOG` | 日志级别 | `info` |

## 2. 启动与停止

### 2.1 使用 Docker Compose
```bash
# 启动所有服务
docker-compose up -d

# 查看日志
docker-compose logs -f crawlrs

# 停止服务
docker-compose down
```

### 2.2 裸机部署
1. 确保数据库和 Redis 已运行
2. 运行迁移:
   ```bash
   sea-orm-cli migrate up
   ```
3. 启动服务:
   ```bash
   ./target/release/crawlrs
   ```

## 3. 监控与告警

### 3.1 健康检查
- **Endpoint**: `/health`
- **预期响应**: `200 OK`

### 3.2 关键指标
- **CPU/Memory**: 容器资源使用率
- **Database Connections**: 连接池状态
- **Queue Lag**: Redis 队列堆积情况

## 4. 故障排查

### 4.1 常见问题
- **数据库连接超时**: 检查 `DATABASE_URL` 和网络连通性
- **任务不执行**: 检查 Redis 状态和 Worker 日志
- **抓取失败**: 检查目标网站是否屏蔽或网络限制

### 4.2 日志分析
日志采用 JSON 格式，包含 `request_id` 便于追踪链路：
```json
{"level":"INFO","fields":{"message":"Task completed","task_id":"...","request_id":"..."}}
```

## 5. 备份与恢复

### 5.1 数据库备份
```bash
pg_dump -U user -h localhost crawlrs_db > backup.sql
```

### 5.2 数据库恢复
```bash
psql -U user -h localhost crawlrs_db < backup.sql
```
