# Crawlrs 测试环境清理工具使用指南

## 概述

本文档描述了 Crawlrs 项目中用于测试环境清理的工具和流程。确保每个测试执行前环境都是干净的，防止测试间污染。

## 工具列表

所有清理工具位于 `docker/test-utils/` 目录下：

| 工具 | 功能 | 使用场景 |
|------|------|----------|
| `cleanup-test-env.sh` | 主清理脚本，协调所有清理 | 主要入口 |
| `cleanup-db.sh` | 清理 PostgreSQL 数据库 | 仅清理数据库 |
| `cleanup-files.sh` | 清理文件系统 | 仅清理文件 |
| `reset-containers.sh` | 重建 Docker 容器 | 容器级清理 |

## 快速开始

### 完整清理（推荐）

```bash
# 在项目根目录下执行
cd /path/to/crawlrs

# 清理所有测试环境
./docker/test-utils/cleanup-test-env.sh
```

### 使用 run_tests_docker.sh（自动清理）

```bash
# 运行测试，脚本会自动在测试前清理环境
./run_tests_docker.sh api

# 运行完整测试
./run_tests_docker.sh full
```

## 详细使用

### 主清理脚本

```bash
# 清理所有（数据库 + 文件系统）
./docker/test-utils/cleanup-test-env.sh

# 仅清理数据库
./docker/test-utils/cleanup-test-env.sh --db-only

# 仅清理文件系统
./docker/test-utils/cleanup-test-env.sh --files-only

# 顺序清理（便于调试）
./docker/test-utils/cleanup-test-env.sh --sequential

# 仅验证（不清理）
./docker/test-utils/cleanup-test-env.sh --verify

# 显示清理前状态
./docker/test-utils/cleanup-test-env.sh --status
```

### 数据库清理

```bash
# 清理所有测试数据
./docker/test-utils/cleanup-db.sh

# 仅清理指定表
./docker/test-utils/cleanup-db.sh --partial task,scrape_result

# 仅验证
./docker/test-utils/cleanup-db.sh --verify

# 查看状态
./docker/test-utils/cleanup-db.sh --status
```

### 文件系统清理

```bash
# 清理所有测试文件
./docker/test-utils/cleanup-files.sh

# 清理 3 天前的文件
./docker/test-utils/cleanup-files.sh --old 3

# 仅清理指定目录
./docker/test-utils/cleanup-files.sh --partial temp,logs

# 仅验证
./docker/test-utils/cleanup-files.sh --verify
```

### Docker 容器管理

```bash
# 重置并启动所有服务
./docker/test-utils/reset-containers.sh

# 仅重置（不启动）
./docker/test-utils/reset-containers.sh --reset

# 仅启动（不重置）
./docker/test-utils/reset-containers.sh --start

# 仅验证状态
./docker/test-utils/reset-containers.sh --verify

# 仅重置指定服务
./docker/test-utils/reset-containers.sh --partial test-db
```

## 清理流程

### 完整清理流程

```
1. 检查依赖工具 (psql)
2. 验证服务连接
3. 并行清理：
   - PostgreSQL: TRUNCATE 所有表 + 重置序列
   - 文件系统: 删除 temp, logs, test-data 等目录内容
4. 验证清理结果
5. 报告状态
```

### 清理目标

- **数据库**: 30 秒内完成
- **文件系统**: 30 秒内完成
- **总体**: 60 秒内完成（并行执行）

## 测试隔离保证

每个测试运行前，清理机制确保：

1. ✅ 数据库中无残留数据
2. ✅ 文件系统中无临时文件
3. ✅ Docker 容器状态干净
4. ✅ 应用状态重置

## CI/CD 集成

### GitHub Actions 示例

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Docker
        run: |
          docker-compose -f docker/docker-compose.test.yml up -d

      - name: Cleanup before tests
        run: |
          ./docker/test-utils/cleanup-test-env.sh

      - name: Run tests
        run: |
          ./run_tests_docker.sh api

      - name: Cleanup after tests
        if: always()
        run: |
          ./docker/test-utils/cleanup-test-env.sh
```

## 故障排除

### 清理失败

```bash
# 1. 检查 Docker 是否运行
docker info

# 2. 尝试容器级清理
./docker/test-utils/reset-containers.sh --reset

# 3. 手动检查
./docker/test-utils/cleanup-test-env.sh --status

# 4. 查看日志
cat docker/test-results/*.log
```

### 数据库连接问题

```bash
# 检查数据库连接
./docker/test-utils/cleanup-db.sh --verify

# 检查容器状态
docker ps | grep test-db

# 查看数据库日志
docker logs crawlrs-test-db
```

## 自定义配置

### 环境变量

```bash
# 数据库配置
export CRAWLRS__DATABASE__HOST=test-db
export CRAWLRS__DATABASE__PORT=5432
export CRAWLRS__DATABASE__NAME=crawlrs_test
export CRAWLRS__DATABASE__USER=crawlrs
export CRAWLRS__DATABASE__PASSWORD=password
```

### 超时配置

```bash
# 设置清理超时为 60 秒
./docker/test-utils/cleanup-test-env.sh --timeout 60
```

## 最佳实践

1. **始终使用清理工具**: 不要手动删除数据
2. **测试前自动清理**: 使用 `run_tests_docker.sh` 自动处理
3. **CI 环境必清理**: 确保每次 CI 运行前环境干净
4. **失败时重建容器**: 如果清理失败，使用 `reset-containers.sh`
5. **验证清理结果**: 使用 `--verify` 选项确认
