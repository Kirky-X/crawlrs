# Auth Examples

认证授权示例，演示如何使用 crawlrs 的身份验证和授权功能。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `api_key_auth.rs` | API密钥认证示例 |
| `bearer_token.rs` | Bearer Token认证示例 |
| `team_isolation.rs` | 团队隔离示例 |
| `scope_validation.rs` | 作用域验证示例 |

## 核心功能

### API密钥认证
- API密钥生成
- 密钥验证
- 密钥管理

### Bearer Token认证
- Token获取
- Token刷新
- Token验证

### 团队隔离
- 团队资源隔离
- 团队权限控制
- 团队配额管理

### 作用域验证
- API作用域
- 端点访问控制
- 权限验证

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用API密钥认证
    let client = crawlrs::Client::with_api_key("your-api-key");
    
    let result = client.scrape("https://example.com").await?;
    Ok(())
}
```

## 前置条件

- 确保已配置数据库
- 根据需要配置Redis进行会话存储

## 相关示例

- 团队管理示例：`../teams/`
- 高级功能：`../advanced/`
