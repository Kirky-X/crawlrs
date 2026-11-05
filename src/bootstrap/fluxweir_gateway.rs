// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! fluxweir 网关生命周期（变更 `fluxweir-llm-gateway`，design D2/D3）
//!
//! - `embedded`：进程内起服（`build_app_with_pool` 单池共享 crawlrs 的
//!   dbnexus 连接池），readyz 探活 fail-fast，停机 flush 完成才退出
//! - `external`：SDK 直连外部实例，不起服、不阻塞装配
//!
//! 句柄类型 [`FluxweirGatewayHandle`] 无条件编译（`ServicesComponents`/
//! `CrawlRsState` 字面量零 cfg 分叉）；fluxweir/axum 依赖全部收敛在
//! `cfg(feature = "gateway")` 代码块内，默认构建零涉及。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::config::Settings;

/// readyz 探活总上限（指数退避起点 200ms、封顶 2s；超时即启动失败）
pub const GATEWAY_READYZ_TIMEOUT: Duration = Duration::from_secs(30);

/// 网关句柄：embedded 持有 fluxweir `ServerHandle`（停机 flush 收尾），
/// external 仅承载 base_url；disabled 形态为空句柄（零开销）。
#[derive(Clone, Default)]
pub struct FluxweirGatewayHandle {
    /// 网关数据面 base_url（SDK 回环调用与降级链消费；None = 未启用）
    base_url: Option<String>,
    /// `ServerHandle` 非 Clone（OnceCell 装配态），经 Arc 共享停机句柄
    #[cfg(feature = "gateway")]
    handle: Option<Arc<fluxweir::server::ServerHandle>>,
}

impl FluxweirGatewayHandle {
    /// 网关 base_url（None = 未启用；SDK 消费方据此装配客户端）
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    /// 网关是否启用（embedded 或 external 任一形态）
    pub fn enabled(&self) -> bool {
        self.base_url.is_some()
    }

    /// 优雅停机：embedded 形态先等 fluxweir flush 完成（pre-aggregator 最终
    /// flush + 配额账本 flush_now），再放行宿主侧池关闭（design D2 顺序钉死，
    /// 顺序颠倒会截断最终 flush）；external/disabled 形态为 no-op。
    pub async fn shutdown(&self) {
        #[cfg(feature = "gateway")]
        if let Some(handle) = &self.handle {
            log::info!("fluxweir gateway shutting down: flushing request logs and token usage");
            handle.shutdown().await;
            log::info!("fluxweir gateway shutdown complete (flush finished)");
        }
    }
}

/// 初始化 fluxweir 网关（`[fluxweir].enabled=false` → 空句柄零开销）。
///
/// * `shared_pool` - embedded 形态必需：crawlrs 的 dbnexus 连接池克隆
///   （单池共享，fluxweir 持久化面落同一 PostgreSQL）；external 形态忽略。
/// * `db_url` - 宿主数据库连接 URL：共享池形态下 Repository 面不经此建连
///   （连接由 shared_pool 承担），但 fluxweir 的 DI kit 池（inklog 存储
///   面）仍按此 URL 建连——必须真实可连；external 形态忽略。
///
/// `enabled=true` 但 gateway 未编译 → 启动 fail-fast（错误文案含 feature 指引）。
pub async fn init_fluxweir_gateway(
    settings: &Settings,
    shared_pool: Option<Arc<dbnexus::DbPool>>,
    db_url: &str,
) -> Result<FluxweirGatewayHandle> {
    let fw = &settings.fluxweir;
    if !fw.enabled {
        return Ok(FluxweirGatewayHandle::default());
    }

    #[cfg(not(feature = "gateway"))]
    {
        let _ = (shared_pool, db_url);
        Err(anyhow!(
            "fluxweir.enabled=true requires the gateway feature; rebuild with --features gateway"
        ))
    }

    #[cfg(feature = "gateway")]
    {
        fw.validate()
            .map_err(|e| anyhow!("fluxweir config invalid: {e}"))?;

        if fw.mode == "external" {
            let base_url = fw
                .external_base_url
                .trim()
                .trim_end_matches('/')
                .to_string();
            // 外部实例可用性不阻塞装配（spec R-gateway-lifecycle-001：
            // external 不起服、SDK 直连；连通性由首次请求的熔断/重试承载）
            log::info!("fluxweir gateway mode=external, base_url={base_url}");
            return Ok(FluxweirGatewayHandle {
                base_url: Some(base_url),
                handle: None,
            });
        }

        // ---- embedded：单池共享起服 ----
        let pool = shared_pool.ok_or_else(|| {
            anyhow!(
                "fluxweir.mode=embedded requires the crawlrs database pool (single-pool sharing)"
            )
        })?;
        // DI kit 池（inklog 存储面）按此 URL 真实建连——占位/空值一律 fail-fast，
        // 不留假绿（共享池只覆盖 Repository 面，kit 池独立建连）
        let db_url = db_url.trim().to_string();
        if db_url.is_empty() {
            return Err(anyhow!(
                "fluxweir.mode=embedded requires a real database URL \
                 (fluxweir DI kit connects via it; the shared pool only covers the repository face)"
            ));
        }

        let (host, port) = parse_listen_addr(&fw.listen_addr)?;
        // 非 loopback 绑定留下审计痕迹：数据面有摘要鉴权兜底，但暴露面
        // 由运维显式选择（0.0.0.0 为文档化逃生口）
        if !host.is_loopback() {
            log::warn!(
                "fluxweir gateway binding non-loopback address {host}:{port} — \
                 the gateway is reachable beyond this host"
            );
        }
        let config_manager = build_config_manager(fw, &db_url, host, port)?;

        log::info!(
            "starting embedded fluxweir gateway at {} (shared pool, db_url redacted: {})",
            fw.listen_addr,
            fluxweir::database::redact_db_url(&db_url)
        );

        let (router, handle) =
            fluxweir::server::build_app_with_pool(config_manager, (*pool).clone()).await?;

        let addr = std::net::SocketAddr::from((host, port));
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow!("failed to bind fluxweir gateway on {addr}: {e}"))?;
        let base_url = format!("http://{addr}");

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                log::error!("fluxweir gateway serve terminated with error: {e:#}");
            }
        });

        wait_for_ready(&base_url).await?;
        log::info!("fluxweir gateway ready at {base_url}");

        Ok(FluxweirGatewayHandle {
            base_url: Some(base_url),
            handle: Some(Arc::new(handle)),
        })
    }
}

/// 解析 `host:port` 监听地址（embedded 必需；`validate` 已保证非空）
#[cfg(feature = "gateway")]
fn parse_listen_addr(addr: &str) -> Result<(std::net::IpAddr, u16)> {
    let (host, port) = addr
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("invalid fluxweir.listen_addr \"{addr}\" (expected host:port)"))?;
    let port: u16 = port
        .parse()
        .map_err(|e| anyhow!("invalid fluxweir.listen_addr port in \"{addr}\": {e}"))?;
    let host: std::net::IpAddr = host
        .parse()
        .map_err(|e| anyhow!("invalid fluxweir.listen_addr host \"{host}\": {e}"))?;
    Ok((host, port))
}

/// fluxweir `Config` 装配（embedded）：
/// - `config_path` 非空 → 整文件加载（`[database].url` 须指向宿主库，仅作
///   内存库判定与日志脱敏；连接由共享池承担）
/// - 空 → 内联最小装配（`server`/`database`/`vendors`/`global_model` 四段
///   为 serde 必填；其余段缺省）
///
/// 共同覆盖：`server`（listen_addr 投影）、`database.url`（宿主库）、
/// `auth`（digest 形态登记网关 key——TrustedIdentity 直连缺失时回退该路径，
/// 防误暴露监听端口的兜底防线）。
#[cfg(feature = "gateway")]
fn build_config_manager(
    fw: &crate::config::settings::FluxweirSettings,
    db_url: &str,
    host: std::net::IpAddr,
    port: u16,
) -> Result<Arc<fluxweir::config::ConfigManager>> {
    let mut config: fluxweir::config::Config = if fw.config_path.trim().is_empty() {
        serde_json::from_value(serde_json::json!({
            "server": {"host": host.to_string(), "port": port},
            "database": {"url": db_url},
            // 探针 vendor：api_base 指向不可达回环地址。内联装配无业务上游，
            // 无它则所有 chat 请求在路由前 400（不落日志）；有它则请求走到
            // 出站失败 → log_chat_failure 落 request_logs（单池接管实证路径）。
            // 业务上游经 config_path 完整配置或 admin 端点登记。
            "vendors": [{
                "id": "probe-vendor",
                "name": "probe-vendor",
                "type_": "openai",
                "api_key_env": "FLUXWEIR_PROBE_VENDOR_KEY", // pragma: allowlist secret
                "api_base": "http://127.0.0.1:1",
                "enabled": true,
                "priority": 1,
                "models": [{"model_id": "integration-probe", "abstract_id": "integration-probe"}],
                "rate_limit": {"type": "interval", "hours": 1},
                "concurrency": {}
            }],
            "global_model": null,
        }))
        .map_err(|e| anyhow!("failed to build inline fluxweir config: {e}"))?
    } else {
        fluxweir::config::loader::load_config_from_path(std::path::Path::new(&fw.config_path))
            .map_err(|e| {
                anyhow!(
                    "failed to load fluxweir config file {}: {e}",
                    fw.config_path
                )
            })?
    };

    config.server.host = host.to_string();
    config.server.port = port;
    config.database.url = db_url.to_string();

    let key = fw.api_key().ok_or_else(|| {
        anyhow!("fluxweir enabled but no API key resolvable (api_key_env or direct value)")
    })?;
    config.auth = Some(fluxweir::config::AuthSection {
        enabled: true,
        api_keys: vec![sha256_hex(&key)],
        header: fluxweir::config::AuthHeaderSection::default(),
        keys: Vec::new(),
        oidc_rules: None,
        // 可信代理开关随宿主配置透传（缩围形态缺省 false：SDK 回环 + 摘要
        // 鉴权已闭环；用户显式开启供未来 HTTP 直连身份桥使用）
        trusted_proxy: fw.trusted_proxy,
    });

    Ok(Arc::new(fluxweir::config::ConfigManager::new(config)))
}

/// 网关 key 摘要（fluxweir `[auth].api_keys` 契约为 SHA256 hex 摘要形态）
#[cfg(feature = "gateway")]
fn sha256_hex(input: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(input.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// readyz 探活：指数退避（200ms 起步、2s 封顶），总上限
/// [`GATEWAY_READYZ_TIMEOUT`]，超时即结构化失败（spec R-gateway-lifecycle-001）
#[cfg(feature = "gateway")]
async fn wait_for_ready(base_url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{base_url}/readyz");
    let deadline = tokio::time::Instant::now() + GATEWAY_READYZ_TIMEOUT;
    let mut delay = Duration::from_millis(200);

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "fluxweir gateway readyz timeout after {:?} at {url}",
                GATEWAY_READYZ_TIMEOUT
            ));
        }
        match client
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => log::debug!("fluxweir readyz not ready yet: {}", resp.status()),
            Err(e) => log::debug!("fluxweir readyz probe failed: {e}"),
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gateway")]
    #[test]
    fn parse_listen_addr_accepts_loopback() {
        let (host, port) = parse_listen_addr("127.0.0.1:8900").expect("ok");
        assert_eq!(host.to_string(), "127.0.0.1");
        assert_eq!(port, 8900);
    }

    #[cfg(feature = "gateway")]
    #[test]
    fn parse_listen_addr_rejects_missing_port() {
        assert!(parse_listen_addr("127.0.0.1").is_err());
    }

    #[cfg(feature = "gateway")]
    #[test]
    fn parse_listen_addr_rejects_bad_port() {
        assert!(parse_listen_addr("127.0.0.1:notaport").is_err());
    }

    #[cfg(feature = "gateway")]
    #[test]
    fn sha256_hex_matches_known_vector() {
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex("abc"),
            // sha256("abc") 已知向量，假阳性豁免
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" // pragma: allowlist secret
        );
    }

    #[test]
    fn disabled_handle_is_empty_and_shutdown_is_noop() {
        let handle = FluxweirGatewayHandle::default();
        assert!(!handle.enabled());
        assert!(handle.base_url().is_none());
    }

    #[tokio::test]
    async fn disabled_handle_shutdown_returns() {
        let handle = FluxweirGatewayHandle::default();
        handle.shutdown().await;
    }
}
