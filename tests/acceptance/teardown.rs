// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! run 结束清理：显式停止 testcontainers PG + 关停 inklog 日志 worker。
//!
//! 两项都曾是伴生缺陷：
//! 1. Ryuk 的回收依赖客户端进程断开连接后触发；本机观测到回收失效
//!    （每个 run 泄漏 1 个 postgres:16-alpine 容器，曾累积 159 个），
//!    显式 stop 是不依赖 Ryuk 的确定性兜底（外部 TEST_DATABASE_URL 时 no-op）。
//! 2. LoggerManager 若存 bootstrap 局部变量会在返回时 drop → 日志 worker
//!    立即关停（"shutdown signal lost"），运行期日志全被吞；改进程级保活后，
//!    进程退出时 tokio runtime drop 会等 spawn_blocking 的 worker，
//!    因此 run 结束必须显式 shutdown 再收尾。

/// run 结束清理（幂等；未走对应路径时为 no-op）。
pub async fn shutdown() {
    stop_testcontainers_pg().await;
    shutdown_logger();
}

/// 停止并删除 harness 启动的 testcontainers PG（未走 testcontainers 返回 false）。
///
/// 用 `rm()`（force + 删卷）而非 `stop()`：容器删除同样依赖 Ryuk，
/// 本机 Ryuk 回收失效，只 stop 会残留 exited 容器条目。`take()` 取出所有权
/// （`rm` 吃 `self`），取后静态为空，天然幂等。
async fn stop_testcontainers_pg() -> bool {
    let container = crate::support::TC_CONTAINER.lock().take();
    match container {
        Some(container) => match container.rm().await {
            Ok(()) => {
                eprintln!("[teardown] testcontainers PostgreSQL stopped and removed");
                true
            }
            Err(e) => {
                eprintln!("[teardown] failed to remove testcontainers PostgreSQL: {e}");
                false
            }
        },
        None => false,
    }
}

/// 关停 inklog 日志 worker（未初始化时为 no-op）。
fn shutdown_logger() {
    match crate::support::LOGGER_MANAGER.get() {
        Some(manager) => {
            if let Err(e) = manager.shutdown() {
                eprintln!("[teardown] inklog shutdown error: {e}");
            }
            eprintln!("[teardown] inklog workers shut down");
        }
        None => {}
    }
}
