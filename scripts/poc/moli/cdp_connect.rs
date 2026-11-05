//! moli CDP 指令面最小验证程序（chromiumoxide 0.9 直连 `moli serve`）。
//!
//! 逐项验证四类指令，每项输出三态结果（pass / fail + 报错原文）：
//! 1. Page.navigate            —— `page.goto`（底层发 Page.navigate）
//! 2. 渲染后 outerHTML         —— `page.evaluate`（底层发 Runtime.evaluate）
//! 3. Runtime.evaluate 表达式  —— 计算型表达式取返回值
//! 4. Page.captureScreenshot   —— `page.screenshot`（底层发 Page.captureScreenshot）
//!
//! 用法：cargo run --bin cdp_connect -- [ws_url]
//! 缺省 ws_url 时经 `curl http://127.0.0.1:9222/json/version` 自动发现。
//! 退出码 = 失败指令数（0 = 全部通过）。

use chromiumoxide::Browser;
use futures::StreamExt;

const HTTP_CDP_BASE: &str = "http://127.0.0.1:9222";
const TARGET_URL: &str = "https://example.com";

struct StepResult {
    name: &'static str,
    cdp_command: &'static str,
    passed: bool,
    detail: String,
}

fn discover_ws_url() -> Result<String, String> {
    let out = std::process::Command::new("curl")
        .args(["-sf", "--max-time", "5", &format!("{}/json/version", HTTP_CDP_BASE)])
        .output()
        .map_err(|e| format!("curl 调用失败: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "GET /json/version 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("/json/version 响应非合法 JSON: {}", e))?;
    parsed
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 webSocketDebuggerUrl 字段".to_string())
}

fn report(results: &[StepResult]) {
    println!("\n===== moli CDP 指令面验证结果 =====");
    for r in results {
        let status = if r.passed { "pass" } else { "FAIL" };
        println!("[{}] {:<24} ({}) {}", status, r.name, r.cdp_command, r.detail);
    }
    let failed = results.iter().filter(|r| !r.passed).count();
    println!(
        "===== {} 项通过 / {} 项失败 =====",
        results.len() - failed,
        failed
    );
}

#[tokio::main]
async fn main() {
    let ws_url = match std::env::args().nth(1) {
        Some(u) => u,
        None => match discover_ws_url() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("ws URL 发现失败: {}", e);
                std::process::exit(4);
            }
        },
    };
    println!("连接 moli CDP: {}", ws_url);

    let (browser, mut handler) = match Browser::connect(ws_url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Browser::connect 失败: {}", e);
            std::process::exit(4);
        }
    };
    let handler_task = tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });

    let mut results: Vec<StepResult> = Vec::new();

    let page = match browser.new_page("about:blank").await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("new_page 失败: {}", e);
            handler_task.abort();
            std::process::exit(4);
        }
    };

    // 1. Page.navigate
    let mut nav = StepResult {
        name: "navigate",
        cdp_command: "Page.navigate",
        passed: false,
        detail: String::new(),
    };
    match page.goto(TARGET_URL).await {
        Ok(_) => {
            nav.passed = true;
            nav.detail = "goto Ok（Page.navigate 无异常返回）".to_string();
        }
        Err(e) => nav.detail = format!("报错原文: {}", e),
    }
    results.push(nav);

    // 2. 渲染后 outerHTML（Runtime.evaluate）
    let mut html_check = StepResult {
        name: "outerHTML",
        cdp_command: "Runtime.evaluate",
        passed: false,
        detail: String::new(),
    };
    match page
        .evaluate("() => document.documentElement.outerHTML")
        .await
    {
        Ok(v) => match v.into_value::<String>() {
            Ok(html) => {
                let len = html.len();
                let has_root = html.contains("<html") && html.contains("example");
                if len > 200 && has_root {
                    html_check.passed = true;
                    html_check.detail = format!("len={} 且含 <html>/正文特征", len);
                } else {
                    html_check.detail = format!("内容可疑: len={}, has_root={}", len, has_root);
                }
            }
            Err(e) => html_check.detail = format!("返回值反序列化失败: {}", e),
        },
        Err(e) => html_check.detail = format!("报错原文: {}", e),
    }
    results.push(html_check);

    // 3. Runtime.evaluate 计算型表达式
    let mut eval_check = StepResult {
        name: "evaluate",
        cdp_command: "Runtime.evaluate",
        passed: false,
        detail: String::new(),
    };
    match page
        .evaluate("() => { const t = document.title; return JSON.stringify({len: t.length, ua: navigator.userAgent.slice(0, 40)}); }")
        .await
    {
        Ok(v) => match v.into_value::<String>() {
            Ok(s) if s.contains("\"len\"") => {
                eval_check.passed = true;
                eval_check.detail = s;
            }
            Ok(s) => eval_check.detail = format!("返回值不含预期键: {}", s),
            Err(e) => eval_check.detail = format!("返回值反序列化失败: {}", e),
        },
        Err(e) => eval_check.detail = format!("报错原文: {}", e),
    }
    results.push(eval_check);

    // 4. Page.captureScreenshot
    let mut shot = StepResult {
        name: "screenshot",
        cdp_command: "Page.captureScreenshot",
        passed: false,
        detail: String::new(),
    };
    let params = chromiumoxide::page::ScreenshotParams::builder()
        .format(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
        .build();
    match page.screenshot(params).await {
        Ok(bytes) => {
            let is_png = bytes.len() > 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G']);
            let out_path = std::env::temp_dir().join(format!(
                "moli-poc-screenshot-{}.png",
                std::process::id()
            ));
            if let Err(e) = std::fs::write(&out_path, &bytes) {
                shot.detail = format!("截图字节有效但写盘失败 {}: {}", out_path.display(), e);
            }
            if is_png {
                shot.passed = true;
                shot.detail = format!("{} bytes PNG → {:?}", bytes.len(), out_path);
            } else {
                shot.detail = format!("返回 {} bytes，非 PNG 魔数", bytes.len());
            }
        }
        Err(e) => shot.detail = format!("报错原文: {}", e),
    }
    results.push(shot);

    report(&results);
    let failed = results.iter().filter(|r| !r.passed).count();
    // 不发 Browser.close：serve 型端点收到该指令会整个退出（moli 与 Chrome 同语义）。
    drop(browser);
    handler_task.abort();
    std::process::exit(failed as i32);
}
