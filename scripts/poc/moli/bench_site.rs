//! moli/Chrome 站点对照测量的单页驱动器（bench.sh 逐行调用）。
//!
//! 用法：bench_site <endpoint_http_base> <url> <timeout_ms>
//! 对任意暴露 CDP /json/version 的端点（moli serve 或 Chrome --remote-debugging-port）
//! 建连、开页、导航、取 outerHTML，输出单行 JSON：
//! {"ok":bool,"ms":u128,"html_len":usize,"error":string}

use chromiumoxide::Browser;
use futures::StreamExt;
use std::time::{Duration, Instant};

fn discover_ws_url(base: &str) -> Result<String, String> {
    let out = std::process::Command::new("curl")
        .args(["-sf", "--max-time", "5", &format!("{}/json/version", base)])
        .output()
        .map_err(|e| format!("curl 调用失败: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "GET {}/json/version 失败: {}",
            base,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("响应非合法 JSON: {}", e))?;
    parsed
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 webSocketDebuggerUrl".to_string())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("用法: bench_site <endpoint_http_base> <url> <timeout_ms>");
        std::process::exit(2);
    }
    let base = args[1].clone();
    let url = args[2].clone();
    let timeout = Duration::from_millis(args[3].parse::<u64>().unwrap_or(30_000));

    let started = Instant::now();
    let result = tokio::time::timeout(timeout, async {
        let ws_url = discover_ws_url(&base)?;
        let (browser, mut handler) = Browser::connect(ws_url)
            .await
            .map_err(|e| format!("connect: {}", e))?;
        let handler_task = tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if h.is_err() {
                    break;
                }
            }
        });
        let res = async {
            let page = browser
                .new_page("about:blank")
                .await
                .map_err(|e| format!("new_page: {}", e))?;
            page.goto(url.as_str())
                .await
                .map_err(|e| format!("goto: {}", e))?;
            let html = page
                .evaluate("() => document.documentElement.outerHTML")
                .await
                .map_err(|e| format!("outerHTML: {}", e))?
                .into_value::<String>()
                .map_err(|e| format!("deserialize: {}", e))?;
            let ok = html.len() > 500 && html.to_lowercase().contains("<html");
            // 只关页面 target，不发 Browser.close——serve 型端点收到 Browser.close
            // 会整个退出（moli 与 Chrome 同语义），杀掉基准目标进程。
            let _ = page.close().await;
            Ok::<(bool, usize), String>((ok, html.len()))
        }
        .await;
        drop(browser);
        handler_task.abort();
        res
    })
    .await;

    let ms = started.elapsed().as_millis();
    let (ok, html_len, error) = match result {
        Ok(Ok((ok, len))) => (ok, len, String::new()),
        Ok(Err(e)) => (false, 0, e),
        Err(_) => (false, 0, "timeout".to_string()),
    };
    println!(
        "{}",
        serde_json::json!({"ok": ok, "ms": ms, "html_len": html_len, "error": error})
    );
}
