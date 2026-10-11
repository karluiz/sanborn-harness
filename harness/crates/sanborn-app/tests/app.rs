//! Deterministic HTTP integration tests. Reasoning is scripted; Rust plans,
//! Laya, Tadpole contracts, tools and the loopback guards are real.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use sanborn_app::bridge::{Bridge, Limits, ProviderFactory, Reasoning};
use sanborn_app::{server, synthetic};
use sanborn_core::testing::{text_response, ScriptedProvider};
use sanborn_core::{CompletionRequest, CompletionResponse, Provider};
use sanborn_laya::LocalLaya;
use serde_json::{json, Value};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct App {
    base: String,
    port: u16,
    client: reqwest::Client,
    server: tokio::task::JoinHandle<()>,
    bridge: Arc<Bridge>,
    workspace: tempfile::TempDir,
}

impl Drop for App {
    fn drop(&mut self) {
        self.bridge.shutdown();
        self.server.abort();
    }
}

impl App {
    async fn launch(factory: ProviderFactory, limits: Limits) -> Self {
        let bridge = Bridge::new(
            factory,
            Arc::new(LocalLaya),
            Reasoning {
                kind: "synthetic",
                label: "test script".into(),
            },
            limits,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = server::router(bridge.clone(), TOKEN.into(), port);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join(synthetic::FIXTURE_FILE),
            synthetic::FIXTURE_BEFORE,
        )
        .unwrap();
        Self {
            base: format!("http://127.0.0.1:{port}"),
            port,
            client: reqwest::Client::new(),
            server,
            bridge,
            workspace,
        }
    }

    async fn call(&self, path: &str, body: Option<Value>) -> (u16, Value) {
        let url = format!("{}/api{path}", self.base);
        let request = match body {
            Some(body) => self.client.post(url).json(&body),
            None => self.client.get(url),
        };
        let response = request.bearer_auth(TOKEN).send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn select(&self) {
        let path = self.workspace.path().to_string_lossy().into_owned();
        let (status, body) = self.call("/workspace", Some(json!({"path": path}))).await;
        assert_eq!(status, 200, "{body}");
    }

    async fn settle(&self) -> Value {
        for _ in 0..400 {
            let (_, state) = self.call("/state", None).await;
            if state["run"]["status"] != "running" {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("run did not settle");
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.workspace.path().join(synthetic::FIXTURE_FILE)).unwrap()
    }

    fn file_count(&self) -> usize {
        std::fs::read_dir(self.workspace.path()).unwrap().count()
    }
}

fn scripted(script: fn() -> Vec<CompletionResponse>) -> ProviderFactory {
    Arc::new(move || {
        Box::pin(async move { Ok(Arc::new(ScriptedProvider::new(script())) as Arc<dyn Provider>) })
    })
}

/// Never answers, so a run stays in flight until cancelled.
struct Stalled;
#[async_trait]
impl Provider for Stalled {
    fn name(&self) -> &str {
        "stalled"
    }
    async fn complete(&self, _: &CompletionRequest<'_>) -> Result<CompletionResponse> {
        std::future::pending().await
    }
}

fn stalled() -> ProviderFactory {
    Arc::new(|| Box::pin(async { Ok(Arc::new(Stalled) as Arc<dyn Provider>) }))
}

async fn completed_coder_run(app: &App) -> Value {
    app.select().await;
    let (status, started) = app
        .call("/runs", Some(json!({"prompt": "Fix the greeting"})))
        .await;
    assert_eq!(status, 202, "{started}");
    let state = app.settle().await;
    assert_eq!(state["run"]["status"], "completed", "{state}");
    state
}

#[tokio::test]
async fn synthetic_task_proposes_read_only_then_applies_only_with_grant() {
    let app = App::launch(synthetic::factory(), Limits::default()).await;
    let state = completed_coder_run(&app).await;
    let run = &state["run"];
    assert_eq!(state["permission"]["runs"], "read_only");
    assert_eq!(run["acceptance"], "not_certified");
    assert_eq!(run["inference_calls"], 5);
    assert_eq!(run["usage_complete"], true);
    assert_eq!(run["croaks"][0]["status"], "returned");
    assert!(run["final_text"].as_str().unwrap().contains("read-only"));
    // Laya gated the parent and every child step; metadata never carries contents.
    let events = run["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e["event"] == "laya_decision"));
    assert!(events.iter().any(|e| e["event"] == "croak"));
    assert!(!run["events"].to_string().contains("hello"));
    let proposal = &run["proposals"][0];
    assert_eq!(proposal["proposal_id"], "code-patch");
    assert_eq!(proposal["path"], synthetic::FIXTURE_FILE);
    assert_eq!(proposal["expected_content"], synthetic::FIXTURE_BEFORE);
    assert_eq!(proposal["content"], synthetic::FIXTURE_AFTER);
    assert_eq!(proposal["ready"], true);
    assert_eq!(proposal["stale"], false);
    assert_eq!(app.file(), synthetic::FIXTURE_BEFORE, "run must not write");

    let apply = format!("/runs/{}/proposals/code-patch/apply", run["id"]);
    // Denied write: Rust policy rejects the plan; nothing is written or consumed.
    let (status, denied) = app.call(&apply, Some(json!({"grant_write": false}))).await;
    assert_eq!(status, 403, "{denied}");
    assert!(denied["error"]
        .as_str()
        .unwrap()
        .contains("policy denies tool `apply_tadpole_patch`"));
    assert_eq!(app.file(), synthetic::FIXTURE_BEFORE);
    let (_, state) = app.call("/state", None).await;
    assert_eq!(state["run"]["proposals"][0]["proposal_id"], "code-patch");

    let (status, applied) = app.call(&apply, Some(json!({"grant_write": true}))).await;
    assert_eq!(status, 200, "{applied}");
    assert_eq!(applied["status"], "applied_verified", "{applied}");
    assert_eq!(app.file(), synthetic::FIXTURE_AFTER);
    assert_eq!(app.file_count(), 1);
    let evidence = applied["events"].to_string();
    assert!(evidence.contains("apply_tadpole_patch") && evidence.contains("laya_decision"));

    // Duplicate write: one attempt per proposal, and the proposal is consumed.
    let (status, duplicate) = app.call(&apply, Some(json!({"grant_write": true}))).await;
    assert_eq!(status, 409, "{duplicate}");
    let (_, state) = app.call("/state", None).await;
    assert_eq!(state["run"]["proposals"], json!([]));
    assert_eq!(
        state["run"]["applications"]["code-patch"]["status"],
        "applied_verified"
    );
    assert_eq!(app.file(), synthetic::FIXTURE_AFTER);
}

#[tokio::test]
async fn stale_proposals_are_never_written() {
    let app = App::launch(synthetic::factory(), Limits::default()).await;
    let state = completed_coder_run(&app).await;
    let id = state["run"]["id"].clone();
    std::fs::write(
        app.workspace.path().join(synthetic::FIXTURE_FILE),
        "edited elsewhere\n",
    )
    .unwrap();
    let (_, state) = app.call("/state", None).await;
    assert_eq!(state["run"]["proposals"][0]["stale"], true);
    let (status, failed) = app
        .call(
            &format!("/runs/{id}/proposals/code-patch/apply"),
            Some(json!({"grant_write": true})),
        )
        .await;
    assert_eq!(status, 200, "{failed}");
    assert_eq!(failed["status"], "failed", "{failed}");
    assert_eq!(app.file(), "edited elsewhere\n");

    // A proposal from a replaced run is gone, even if the file matches again.
    std::fs::write(
        app.workspace.path().join(synthetic::FIXTURE_FILE),
        synthetic::FIXTURE_BEFORE,
    )
    .unwrap();
    let old = completed_coder_run(&app).await["run"]["id"].clone();
    let (status, _) = app.call("/runs", Some(json!({"prompt": "again"}))).await;
    assert_eq!(status, 202);
    app.settle().await;
    let (status, gone) = app
        .call(
            &format!("/runs/{old}/proposals/code-patch/apply"),
            Some(json!({"grant_write": true})),
        )
        .await;
    assert_eq!(status, 410, "{gone}");
    assert!(gone["error"].as_str().unwrap().contains("stale proposal"));

    // Changing the selected workspace also makes the current proposal stale.
    let current = app.call("/state", None).await.1["run"]["id"].clone();
    let other = tempfile::tempdir().unwrap();
    let (status, _) = app
        .call(
            "/workspace",
            Some(json!({"path": other.path().to_string_lossy()})),
        )
        .await;
    assert_eq!(status, 200);
    let (status, gone) = app
        .call(
            &format!("/runs/{current}/proposals/code-patch/apply"),
            Some(json!({"grant_write": true})),
        )
        .await;
    assert_eq!(status, 410, "{gone}");
    assert_eq!(app.file(), synthetic::FIXTURE_BEFORE);
}

#[tokio::test]
async fn failures_are_explicit_and_bounded() {
    // Provider setup failure (e.g. missing credentials) is reported, nothing runs.
    let broken: ProviderFactory = Arc::new(|| {
        Box::pin(async {
            Err(anyhow::anyhow!(
                "run sb init, or set AZURE_OPENAI_ENDPOINT or --endpoint"
            ))
        })
    });
    let app = App::launch(broken, Limits::default()).await;
    app.select().await;
    app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    let state = app.settle().await;
    assert_eq!(state["run"]["status"], "failed");
    assert!(state["run"]["error"]
        .as_str()
        .unwrap()
        .contains("reasoning provider is not ready"));
    assert_eq!(state["run"]["inference_calls"], Value::Null);

    // Exhausted script mid-run: the error is surfaced with observed usage.
    let app = App::launch(scripted(Vec::new), Limits::default()).await;
    app.select().await;
    app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    let state = app.settle().await;
    assert_eq!(state["run"]["status"], "failed");
    assert!(state["run"]["error"]
        .as_str()
        .unwrap()
        .contains("ran out of responses"));
    assert_eq!(state["run"]["inference_calls"], 1);
    assert_eq!(state["run"]["usage_complete"], false);

    // The shared inference budget is enforced across parent and tadpoles.
    let app = App::launch(
        synthetic::factory(),
        Limits {
            max_inference_calls: 2,
            ..Limits::default()
        },
    )
    .await;
    app.select().await;
    app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    let state = app.settle().await;
    assert_eq!(state["run"]["status"], "failed", "{state}");
    assert!(state["run"]["error"]
        .as_str()
        .unwrap()
        .contains("shared inference-call budget exhausted"));
    assert_eq!(state["run"]["inference_calls"], 2);
    assert_eq!(app.file(), synthetic::FIXTURE_BEFORE);

    // Deadline.
    let app = App::launch(
        stalled(),
        Limits {
            timeout_secs: 1,
            ..Limits::default()
        },
    )
    .await;
    app.select().await;
    app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let state = app.settle().await;
    assert_eq!(state["run"]["status"], "failed");
    assert!(state["run"]["error"].as_str().unwrap().contains("deadline"));
}

#[tokio::test]
async fn cancellation_and_duplicate_runs() {
    let app = App::launch(stalled(), Limits::default()).await;
    let (status, _) = app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    assert_eq!(status, 400, "workspace selection is required");
    app.select().await;
    for prompt in ["", "   ", "nul\0byte"] {
        let (status, _) = app.call("/runs", Some(json!({"prompt": prompt}))).await;
        assert_eq!(status, 400);
    }
    let (status, started) = app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    assert_eq!(status, 202);
    let id = started["run"].clone();
    let (status, duplicate) = app.call("/runs", Some(json!({"prompt": "inspect"}))).await;
    assert_eq!(status, 409, "{duplicate}");
    let (status, _) = app.call("/workspace", Some(json!({"path": "/"}))).await;
    assert_eq!(status, 409, "workspace is locked during a run");

    let (status, _) = app
        .call(&format!("/runs/{id}/cancel"), Some(json!({})))
        .await;
    assert_eq!(status, 200);
    let (_, state) = app.call("/state", None).await;
    assert_eq!(state["run"]["status"], "cancelled");
    assert!(state["run"]["error"]
        .as_str()
        .unwrap()
        .contains("Cancelled"));
    let (status, _) = app
        .call(&format!("/runs/{id}/cancel"), Some(json!({})))
        .await;
    assert_eq!(status, 409);
    // The cancelled task never overwrites the cancelled state.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        app.call("/state", None).await.1["run"]["status"],
        "cancelled"
    );
    let (status, _) = app.call("/runs", Some(json!({"prompt": "again"}))).await;
    assert_eq!(status, 202);
}

#[tokio::test]
async fn loopback_guards_refuse_untrusted_requests() {
    let app = App::launch(
        scripted(|| vec![text_response("unused")]),
        Limits::default(),
    )
    .await;
    let client = &app.client;
    let state = format!("{}/api/state", app.base);

    let page = client.get(format!("{}/", app.base)).send().await.unwrap();
    assert_eq!(page.status(), 200);
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("default-src 'none'") && csp.contains("frame-ancestors 'none'"));
    let html = page.text().await.unwrap();
    assert!(!html.contains(TOKEN));

    let status = |r: reqwest::Response| r.status().as_u16();
    assert_eq!(status(client.get(&state).send().await.unwrap()), 401);
    assert_eq!(
        status(
            client
                .get(&state)
                .bearer_auth("wrong")
                .send()
                .await
                .unwrap()
        ),
        401
    );
    // DNS rebinding: a foreign Host header is refused even with the token.
    assert_eq!(
        status(
            client
                .get(&state)
                .bearer_auth(TOKEN)
                .header("host", format!("attacker.example:{}", app.port))
                .send()
                .await
                .unwrap()
        ),
        421
    );
    assert_eq!(
        status(
            client
                .post(format!("{}/api/runs", app.base))
                .bearer_auth(TOKEN)
                .header("origin", "https://attacker.example")
                .json(&json!({"prompt": "x"}))
                .send()
                .await
                .unwrap()
        ),
        403
    );
    assert_eq!(
        status(
            client
                .get(&state)
                .bearer_auth(TOKEN)
                .header("origin", format!("http://localhost:{}", app.port))
                .send()
                .await
                .unwrap()
        ),
        200
    );
    let (code, _) = app
        .call("/workspace", Some(json!({"path": "relative/path"})))
        .await;
    assert_eq!(code, 400);
    let (code, _) = app
        .call(
            "/workspace",
            Some(json!({"path": "/path/that-does-not-exist-sanborn"})),
        )
        .await;
    assert_eq!(code, 400);
    let oversized = "x".repeat(sanborn_app::bridge::MAX_PROMPT_BYTES + 1);
    app.select().await;
    let (code, _) = app.call("/runs", Some(json!({"prompt": oversized}))).await;
    assert_eq!(code, 400);
}
