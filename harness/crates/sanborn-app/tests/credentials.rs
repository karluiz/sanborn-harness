//! The real Azure configuration path resolves the credential in Rust; no
//! response the page can read ever contains it. Uses an unroutable endpoint.

use std::sync::Arc;
use std::time::Duration;

use sanborn_app::bridge::{Bridge, Limits, ProviderFactory, Reasoning};
use sanborn_app::server;
use sanborn_laya::LocalLaya;
use serde_json::json;

const SECRET: &str = "synthetic-private-credential-value";
const TOKEN: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

#[tokio::test]
async fn azure_credentials_never_reach_the_page() {
    let config = tempfile::tempdir().unwrap();
    // Single test in this binary, so mutating the process environment is safe.
    std::env::set_var("XDG_CONFIG_HOME", config.path());
    std::env::set_var("AZURE_OPENAI_API_KEY", SECRET);
    std::env::set_var("AZURE_OPENAI_ENDPOINT", "https://example.invalid/");
    let factory: ProviderFactory = Arc::new(|| {
        Box::pin(async {
            let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT").ok();
            let azure = sanborn_cli::configured_azure(endpoint, None, None).await?;
            Ok(Arc::new(azure) as Arc<dyn sanborn_core::Provider>)
        })
    });
    let bridge = Bridge::new(
        factory,
        Arc::new(LocalLaya),
        Reasoning {
            kind: "azure",
            label: "test".into(),
        },
        Limits {
            timeout_secs: 20,
            ..Limits::default()
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = server::router(bridge.clone(), TOKEN.into(), port);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let workspace = tempfile::tempdir().unwrap();

    let mut seen = String::new();
    for asset in ["/", "/app.js", "/app.css", "/favicon.svg"] {
        seen += &client
            .get(format!("{base}{asset}"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
    }
    let post = |path: &str, body: serde_json::Value| {
        client
            .post(format!("{base}/api{path}"))
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
    };
    seen += &post(
        "/workspace",
        json!({"path": workspace.path().to_string_lossy()}),
    )
    .await
    .unwrap()
    .text()
    .await
    .unwrap();
    seen += &post("/runs", json!({"prompt": "inspect"}))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let mut state = serde_json::Value::Null;
    for _ in 0..200 {
        let text = client
            .get(format!("{base}/api/state"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        seen += &text;
        state = serde_json::from_str(&text).unwrap();
        if state["run"]["status"] != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(state["run"]["status"], "failed", "{state}");
    // The provider was built from the credential and attempted one request.
    assert_eq!(state["run"]["inference_calls"], 1, "{state}");
    assert_eq!(state["run"]["usage_complete"], false);
    assert!(!seen.contains(SECRET));
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(config.path()).unwrap().count(), 0);
    server.abort();
}
