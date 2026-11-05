//! moli CDP Network 域事件探针（T023 前置实验，不入 connector 构建面）：
//! 监听 EventResponseReceived，导航后 8s 内统计到达事件数。
use chromiumoxide::Browser;
use chromiumoxide::cdp::browser_protocol::network::EventResponseReceived;
use futures::StreamExt;
use std::time::Duration;

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let out = std::process::Command::new("curl")
            .args(["-sf", "--max-time", "5", "http://127.0.0.1:9222/json/version"])
            .output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let ws = v["webSocketDebuggerUrl"].as_str().unwrap().to_string();
        let (browser, mut handler) = Browser::connect(ws).await.unwrap();
        let ht = tokio::spawn(async move { while let Some(h) = handler.next().await { if h.is_err() { break; } } });
        let page = browser.new_page("about:blank").await.unwrap();
        use chromiumoxide::cdp::browser_protocol::network::EnableParams as NetworkEnableParams;
        if let Err(e) = page.execute(NetworkEnableParams::default()).await {
            println!("Network.enable failed: {}", e);
        } else {
            println!("Network.enable ok");
        }
        let mut events = page.event_listener::<EventResponseReceived>().await.unwrap();
        page.goto(std::env::var("PROBE_URL").unwrap_or_else(|_| "https://www.baidu.com".into()))
            .await.unwrap();
        let mut count = 0usize;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        loop {
            if tokio::time::Instant::now() >= deadline {
                println!("deadline reached");
                break;
            }
            match tokio::time::timeout(Duration::from_secs(2), events.next()).await {
                Ok(Some(ev)) => {
                    count += 1;
                    if count <= 3 { println!("event: {} {}", ev.response.status, ev.response.url); }
                }
                Ok(None) => { println!("stream ended"); break; }
                Err(_) => { println!("(2s no-event window)"); }
            }
        }
        println!("TOTAL ResponseReceived events: {}", count);
        let _ = page.close().await;
        drop(browser);
        ht.abort();
    });
}
