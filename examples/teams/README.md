# Teams Examples

团队管理示例，演示如何使用 crawlrs 进行团队创建和管理。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_teams.rs` | 基础团队管理示例 |
| `geo_restrictions.rs` | 地理限制管理示例 |
| `credits_management.rs` | 积分管理示例 |

## 核心功能

### 团队管理
- 团队创建
- 团队成员管理
- 团队配置

### 地理限制
- IP地理位置限制
- 访问区域控制
- 合规性管理

### 积分系统
- 积分配额
- 使用计费
- 余额管理

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建团队
    let team = Team::create("My Team").await?;
    println!("Team ID: {}", team.id);
    Ok(())
}
```

## 前置条件

- 确保已配置数据库
- 配额缓存由 oxcache 自动管理

## 相关示例

- 认证示例：`../auth/`
- 限流示例：`../rate-limiting/`
