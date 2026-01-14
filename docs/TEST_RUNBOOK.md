# Crawlrs 测试运行手册

## 环境要求

### 硬件要求
- CPU: 4 核心以上
- 内存: 8GB 以上
- 磁盘: 50GB 以上可用空间
- Docker: 20.10+
- Docker Compose: 2.0+

### 软件要求
```bash
docker --version   # >= 20.10
docker-compose --version  # >= 2.0
python3 --version  # >= 3.8
```

## 快速开始

### 1. 启动测试环境
```bash
./scripts/start-test-env.sh
```

### 2. 安装测试依赖
```bash
pip install -r tests/python/requirements.txt
```

### 3. 运行完整测试
```bash
./scripts/run-full-test.sh
```

## 测试配置

### 可用配置

| 配置名称 | 描述 | 服务 |
|---------|------|------|
| `minio_only` | 仅 MinIO 存储 | PostgreSQL, Redis, MinIO |
| `browser_only` | 仅浏览器服务 | PostgreSQL, Redis, Chrome, FlareSolverr |
| `search_only` | 仅搜索功能 | PostgreSQL, Redis, FlareSolverr |
| `full_features` | 所有功能 | PostgreSQL, Redis, Chrome, FlareSolverr, MinIO |

### 运行特性测试
```bash
# MinIO 测试
docker-compose -f docker-compose.test.minio.yml up -d
python -m pytest tests/python/test_api_endpoints.py -v

# 浏览器测试
docker-compose -f docker-compose.test.browser.yml up -d
python -m pytest tests/python/test_api_endpoints.py -v

# 搜索测试
docker-compose -f docker-compose.test.search.yml up -d
python -m pytest tests/python/test_api_endpoints.py -v
```

## 测试套件

### API 测试
```bash
python -m pytest tests/python/test_api_endpoints.py -v
```

### 性能测试
```bash
python -m pytest tests/python/test_performance.py -v -s
```

### 错误处理测试
```bash
python -m pytest tests/python/test_error_handling.py -v
```

## 监控

测试期间可以访问以下监控界面：
- **Prometheus**: http://localhost:9090
- **Grafana**: http://localhost:3001 (admin/admin)

## 故障排除

### 端口冲突
```bash
# 检查端口占用
lsof -i :3000
lsof -i :5432

# 停止冲突服务
docker-compose down
```

### 服务启动失败
```bash
# 查看日志
docker-compose -f docker-compose.test.full.yml logs crawlrs

# 重启服务
docker-compose -f docker-compose.test.full.yml restart
```

### 数据库连接失败
```bash
# 检查数据库状态
docker ps | grep postgres
docker logs crawlrs-test-postgres

# 验证连接
pg_isready -U crawlrs -d crawlrs -h localhost -p 5432
```

## 性能指标

| 指标 | 目标值 |
|------|--------|
| API 吞吐量 | 10000 RPS |
| P50 延迟 | < 50ms |
| P99 延迟 | < 200ms |
| 成功率 | > 99.9% |
| 缓存命中率 | > 60% |
