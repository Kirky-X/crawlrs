# SDK Examples

SDK 开发示例，演示如何使用 sdforge 0.4 定义 API 端点和自定义中间件。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_sdk.rs` | 基础端点定义（`#[forge]` 宏 + 路径参数推断） |
| `custom_middleware.rs` | 自定义中间件（认证、日志、错误处理） |

## 核心功能

### 端点定义
- `#[forge(name, version, path, method, description)]` 声明 API 端点
- `#[state]` 从 axum Extension 提取认证状态
- `#[param(kind = "path")]` 显式声明参数来源
- 参数推断：路径参数（`:id`）、Query 参数（GET）、Body（POST）

### 错误处理
- `ApiError` 枚举（`InvalidInput`、`NotFound`、`AccessDenied`、`Internal`）
- 端点返回 `Result<T, ApiError>` 自动转换为 HTTP 响应

### 自定义中间件
- API Key 认证中间件（`X-API-Key` header 验证）
- 请求日志中间件（方法、路径、耗时）
- 错误处理中间件（5xx 错误捕获）
- 通过 `Extension` 在中间件与 handler 间传递状态

## 快速开始

```rust
use sdforge::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateUserRequest {
    name: String,
}

#[derive(Serialize)]
struct UserResponse {
    id: u64,
    name: String,
}

#[forge(
    name = "create_user",
    version = "v1",
    path = "/sdk/users",
    method = "POST",
    description = "Create a new user"
)]
async fn create_user(req: CreateUserRequest) -> Result<UserResponse, ApiError> {
    Ok(UserResponse { id: 1, name: req.name })
}
```

## 前置条件

- sdforge 启用特性：`http`
- axum 0.8

## 相关示例

- 认证示例：`../auth/`
- 数据库示例：`../database/`
- 配置示例：`../config/`
