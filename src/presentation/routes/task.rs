// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::FromRequestParts;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Extension;
use axum::Router;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::broadcast;

use crate::domain::id::Id;
use crate::domain::models::task_model::Task;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::infrastructure::events::event_bus::{DomainEvent, EventBus};
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::presentation::middleware::auth_middleware::AuthState;

/// 创建任务相关路由
///
/// # 返回值
///
/// 返回配置好的任务路由
///
/// # RESTful 规范说明
///
/// - POST /v1/tasks/_query - 复杂查询使用 POST + _query 后缀
/// - POST /v1/tasks/_cancel - 批量取消操作使用 POST + _cancel 后缀
/// - GET /v1/tasks/{id}/ws - 任务状态实时推送（R-ws-task-status-001）
pub fn task_routes() -> Router {
    Router::new()
        .route(
            "/v1/tasks/_query",
            post(task_handler::query_tasks::<TaskRepositoryImpl>),
        )
        .route(
            "/v1/tasks/_cancel",
            post(task_handler::cancel_tasks::<TaskRepositoryImpl>),
        )
        .route("/v1/tasks/{id}/ws", get(task_status_ws))
}

/// WS 连接治理守卫（R-ws-task-status-003）。
///
/// 计数当前活跃连接并在超过 `max_connections` 时拒绝新连接；
/// `idle_timeout` 是服务端在无可推送事件时保持连接的窗口。
pub struct TaskWsGuard {
    live: AtomicUsize,
    pub max_connections: usize,
    pub idle_timeout: Duration,
}

/// 未显式注入 `TaskWsGuard` 扩展时的进程级缺省守卫——缺省上限仍真实生效，
/// 而非每个请求各自新建守卫使上限形同虚设。
static GLOBAL_GUARD: OnceLock<Arc<TaskWsGuard>> = OnceLock::new();

impl TaskWsGuard {
    pub fn new(max_connections: usize, idle_timeout: Duration) -> Self {
        Self {
            live: AtomicUsize::new(0),
            max_connections,
            idle_timeout,
        }
    }

    /// 尝试占用一个连接名额；超限返回 `false`（调用方应拒绝连接）。
    fn try_acquire(&self) -> bool {
        self.live
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < self.max_connections).then_some(live + 1)
            })
            .is_ok()
    }

    fn release(&self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
    }

    fn live(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }
}

/// GET /v1/tasks/{id}/ws —— 任务状态实时推送。
///
/// 鉴权/租户语义（R-ws-task-status-002）：
/// - 无 `AuthState` 扩展（未携带凭据）→ 401；
/// - task 不存在或属于其他 team → 404（不泄露存在性）；
/// - 连接成功后先推 `subscribed` 确认帧，再按 task/team 过滤推送状态帧。
///
/// 资源治理（R-ws-task-status-003）：连接数超限 → 429；空闲超时 → 服务端
/// 主动关闭。`TaskWsGuard` 扩展缺省时使用进程级缺省守卫（100 连接 / 300s）。
async fn task_status_ws(
    Extension(repo): Extension<Arc<TaskRepositoryImpl>>,
    Extension(bus): Extension<Arc<dyn EventBus>>,
    auth: Option<Extension<AuthState>>,
    guard: Option<Extension<Arc<TaskWsGuard>>>,
    Path(task_id): Path<String>,
    mut parts: axum::http::request::Parts,
) -> Response {
    let Some(Extension(auth)) = auth else {
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    };

    let Ok(id) = task_id.parse::<Id>() else {
        return (StatusCode::BAD_REQUEST, "invalid task id").into_response();
    };

    // 任务存在性 + 租户归属一次判定；跨租户与不存在统一 404（不泄露存在性）。
    let task: Task = match repo.find_by_id(id).await {
        Ok(Some(task)) if task.team_id == auth.team_id => task,
        Ok(_) => {
            return (StatusCode::NOT_FOUND, "task not found").into_response();
        }
        Err(e) => {
            log::error!("ws task lookup failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "task lookup failed").into_response();
        }
    };

    let guard = guard
        .map(|Extension(g)| g)
        .unwrap_or_else(default_global_guard);
    if !guard.try_acquire() {
        log::warn!(
            "ws connection rejected: max_connections={} reached (live={})",
            guard.max_connections,
            guard.live()
        );
        return (StatusCode::TOO_MANY_REQUESTS, "too many ws connections").into_response();
    }

    // 升级判定放最后：先完成鉴权/租户/限额校验，再协商升级——
    // 非 upgrade 请求打到已存在资源时明确要求升级（426）而非 404/挂起。
    let ws = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(ws) => ws,
        Err(_) => {
            guard.release();
            return (StatusCode::UPGRADE_REQUIRED, "websocket upgrade required").into_response();
        }
    };

    let rx = bus.subscribe();
    ws.on_upgrade(move |socket| task_ws_session(socket, rx, task.id, auth.team_id, guard))
}

/// 未注入守卫扩展时的进程级缺省守卫（100 连接 / 300s 空闲）。
/// `Arc` 共享同一份 live 计数，使缺省上限真实生效。
fn default_global_guard() -> Arc<TaskWsGuard> {
    GLOBAL_GUARD
        .get_or_init(|| Arc::new(TaskWsGuard::new(100, Duration::from_secs(300))))
        .clone()
}

/// 单个 ws 会话：先发 subscribed 确认帧，再循环按 task/team 过滤推送事件；
/// 空闲超时（窗口内无可推送事件）即关闭连接并释放名额。
async fn task_ws_session(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<DomainEvent>,
    task_id: Id,
    team_id: Id,
    guard: Arc<TaskWsGuard>,
) {
    let ack = json!({
        "event": "subscribed",
        "task_id": task_id.to_string(),
        "team_id": team_id.to_string(),
    });
    if socket
        .send(Message::Text(ack.to_string().into()))
        .await
        .is_err()
    {
        guard.release();
        return;
    }

    loop {
        match tokio::time::timeout(guard.idle_timeout, rx.recv()).await {
            Ok(Ok(event)) => {
                if let Some(frame) = status_frame(&event, &task_id, &team_id) {
                    if socket
                        .send(Message::Text(frame.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            // broadcast 满时丢弃最旧消息（尽力投递语义）：记录并继续。
            Ok(Err(broadcast::error::RecvError::Lagged(dropped))) => {
                log::warn!("ws subscriber lagged, dropped {dropped} events for task {task_id}");
            }
            // 总线关闭（进程退出路径）→ 结束会话。
            Ok(Err(broadcast::error::RecvError::Closed)) => break,
            // 空闲超时：窗口内无可推送事件 → 主动关闭（R-ws-task-status-003）。
            Err(_elapsed) => {
                log::debug!("ws idle timeout, closing connection for task {task_id}");
                break;
            }
        }
    }

    guard.release();
    let _ = socket.send(Message::Close(None)).await;
}

/// 把领域事件映射为该订阅（task+team 双重过滤）的状态帧；无关事件返回 None。
fn status_frame(event: &DomainEvent, task_id: &Id, team_id: &Id) -> Option<serde_json::Value> {
    match event {
        DomainEvent::TaskCompleted {
            task_id: id,
            team_id: tid,
        } if id == task_id && tid == team_id => Some(json!({
            "event": "task_completed",
            "task_id": task_id.to_string(),
            "team_id": team_id.to_string(),
        })),
        DomainEvent::TaskFailed {
            task_id: id,
            team_id: tid,
            error,
        } if id == task_id && tid == team_id => Some(json!({
            "event": "task_failed",
            "task_id": task_id.to_string(),
            "team_id": team_id.to_string(),
            "error": error,
        })),
        _ => None,
    }
}

use crate::presentation::handlers::task_handler;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_routes_returns_router_without_panic() {
        // task_routes should construct a Router with the ws route and two
        // POST routes, without requiring any state or panicking.
        let router = task_routes();
        // Router<> is Clone; verify it is usable
        let _clone = router.clone();
    }

    #[test]
    fn test_task_routes_clone_is_independent() {
        let router = task_routes();
        let clone = router.clone();
        // Both should be usable (no shared mutable state issues)
        drop(router);
        drop(clone);
    }

    /// R-ws-task-status-003：守卫在达到上限时拒绝新连接，释放后恢复。
    #[test]
    fn ws_guard_rejects_over_limit_and_recovers() {
        let guard = TaskWsGuard::new(2, Duration::from_secs(1));
        assert!(guard.try_acquire(), "first acquire");
        assert!(guard.try_acquire(), "second acquire");
        assert!(!guard.try_acquire(), "third must be rejected at cap");
        assert_eq!(guard.live(), 2);
        guard.release();
        assert!(guard.try_acquire(), "acquire after release");
    }

    /// 状态帧只匹配同 task 同 team 的事件；其他事件与跨租户事件均被过滤。
    #[test]
    fn status_frame_filters_by_task_and_team() {
        let task_id = Id::generate();
        let team_id = Id::generate();

        let own = DomainEvent::TaskCompleted { task_id, team_id };
        let frame = status_frame(&own, &task_id, &team_id).expect("own event matches");
        assert_eq!(frame["event"], "task_completed");
        assert_eq!(frame["task_id"], task_id.to_string());

        // 跨租户事件：同 task 不同 team → 不过帧。
        let cross = DomainEvent::TaskCompleted {
            task_id,
            team_id: Id::generate(),
        };
        assert!(status_frame(&cross, &task_id, &team_id).is_none());

        // 同 team 不同 task → 不过帧。
        let other_task = DomainEvent::TaskFailed {
            task_id: Id::generate(),
            team_id,
            error: "x".to_string(),
        };
        assert!(status_frame(&other_task, &task_id, &team_id).is_none());

        // 无关领域事件（crawl/scrape）→ 不过帧。
        let unrelated = DomainEvent::CrawlCompleted {
            crawl_id: Id::generate(),
            team_id,
        };
        assert!(status_frame(&unrelated, &task_id, &team_id).is_none());
    }
}
