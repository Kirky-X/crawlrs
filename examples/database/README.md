# Database Examples

数据库操作示例，演示如何使用 dbnexus 0.2 进行数据库连接、CRUD、事务和迁移管理。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_crud.rs` | 基础 CRUD 操作（DbConfig / DbPool / Session） |
| `transaction.rs` | 事务管理（提交、回滚、保存点） |
| `migration.rs` | 数据库迁移（SQL 文件、自动迁移） |

## 核心功能

### 连接池管理
- `DbConfig` 配置连接池参数（连接数、超时、权限）
- `DbPool::with_config()` 创建连接池
- `Session` 通过 `pool.get_session(role)` 获取会话

### CRUD 操作
- `session.execute(sql)` 执行原生 SQL
- `session.execute_raw_ddl(sql)` 执行 DDL 语句
- `session.connection()` 获取 Sea-ORM 连接用于实体操作

### 事务管理
- `session.begin_transaction()` 开启事务
- `session.commit()` / `session.rollback()` 提交或回滚
- `SAVEPOINT` / `RELEASE` / `ROLLBACK TO` 保存点

### 迁移管理
- 声明式实体宏（`#[db_entity]`，主项目使用）
- SQL 迁移文件（`pool.run_migrations(dir)`）
- `auto_migrate` 配置项自动应用迁移

## 快速开始

```rust
use dbnexus::{DbConfig, DbPool};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = DbConfig {
        url: "postgresql://user:pass@localhost/db".to_string(),
        max_connections: 10,
        min_connections: 1,
        ..Default::default()
    };
    let pool = DbPool::with_config(config).await?;
    let session = pool.get_session("admin").await?;
    session.execute("SELECT 1").await?;
    Ok(())
}
```

## 前置条件

- PostgreSQL 数据库
- dbnexus 启用特性：`postgres`、`permission`、`cache`、`migration`、`auto-migrate`

## 相关示例

- 缓存示例：`../cache/`
- 配置示例：`../config/`
- 认证示例：`../auth/`
