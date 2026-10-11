//! Loopback HTTP surface. Not safe to expose: there is no user authentication
//! beyond a per-launch token, and the bridge can write to the selected workspace.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::bridge::{Bridge, BridgeError, MAX_PROMPT_BYTES};

const INDEX: &str = include_str!("../assets/index.html");
const SCRIPT: &str = include_str!("../assets/app.js");
const STYLE: &str = include_str!("../assets/app.css");
const ICON: &str = include_str!("../assets/favicon.svg");
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; \
                   connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone)]
struct Shared {
    bridge: Arc<Bridge>,
    token: Arc<str>,
    hosts: Arc<[String]>,
    origins: Arc<[String]>,
}

/// 32 bytes of OS randomness, hex-encoded. Generated per launch; never persisted.
pub fn launch_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| anyhow::anyhow!("cannot generate session token: {error}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn router(bridge: Arc<Bridge>, token: String, port: u16) -> Router {
    let names = ["127.0.0.1", "localhost"];
    let shared = Shared {
        bridge,
        token: token.into(),
        hosts: names.iter().map(|n| format!("{n}:{port}")).collect(),
        origins: names.iter().map(|n| format!("http://{n}:{port}")).collect(),
    };
    let api = Router::new()
        .route("/state", get(state))
        .route("/workspace", post(workspace))
        .route("/runs", post(start))
        .route("/runs/{id}/cancel", post(cancel))
        .route("/runs/{id}/proposals/{proposal}/apply", post(apply))
        .layer(middleware::from_fn_with_state(shared.clone(), authorize))
        .layer(DefaultBodyLimit::max(MAX_PROMPT_BYTES + 16 * 1024));
    Router::new()
        .route("/", get(|| asset(INDEX, "text/html; charset=utf-8")))
        .route(
            "/app.js",
            get(|| asset(SCRIPT, "text/javascript; charset=utf-8")),
        )
        .route("/app.css", get(|| asset(STYLE, "text/css; charset=utf-8")))
        .route("/favicon.svg", get(|| asset(ICON, "image/svg+xml")))
        .nest("/api", api)
        .layer(middleware::from_fn_with_state(shared.clone(), loopback))
        .with_state(shared)
}

async fn asset(body: &'static str, kind: &'static str) -> Response {
    ([(header::CONTENT_TYPE, kind)], body).into_response()
}

fn reject(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

/// DNS-rebinding and framing defenses for every response.
async fn loopback(State(shared): State<Shared>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok());
    if !host.is_some_and(|h| shared.hosts.iter().any(|allowed| allowed == h)) {
        return reject(
            StatusCode::MISDIRECTED_REQUEST,
            "Sanborn app only answers loopback host names",
        );
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, CSP),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::CACHE_CONTROL, "no-store"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// API requests need the launch token header; cross-origin browsers are refused.
async fn authorize(State(shared): State<Shared>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if let Some(origin) = headers.get(header::ORIGIN) {
        if !origin
            .to_str()
            .is_ok_and(|o| shared.origins.iter().any(|allowed| allowed == o))
        {
            return reject(StatusCode::FORBIDDEN, "cross-origin request refused");
        }
    }
    if !token_matches(headers, &shared.token) {
        return reject(
            StatusCode::UNAUTHORIZED,
            "missing or invalid session token; reopen the link printed by sanborn-app",
        );
    }
    next.run(request).await
}

fn token_matches(headers: &HeaderMap, token: &str) -> bool {
    let Some(given) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

impl IntoResponse for BridgeError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Gone(_) => StatusCode::GONE,
        };
        reject(status, &self.to_string())
    }
}

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: usize,
}

async fn state(State(shared): State<Shared>, Query(query): Query<After>) -> Response {
    Json(shared.bridge.snapshot(query.after).await).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceInput {
    path: String,
}

async fn workspace(
    State(shared): State<Shared>,
    Json(input): Json<WorkspaceInput>,
) -> Result<Response, BridgeError> {
    let path = shared.bridge.select_workspace(&input.path)?;
    Ok(Json(json!({"workspace": path})).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunInput {
    prompt: String,
}

async fn start(
    State(shared): State<Shared>,
    Json(input): Json<RunInput>,
) -> Result<Response, BridgeError> {
    let id = shared.bridge.start(input.prompt)?;
    Ok((StatusCode::ACCEPTED, Json(json!({"run": id}))).into_response())
}

async fn cancel(
    State(shared): State<Shared>,
    Path(id): Path<u64>,
) -> Result<Response, BridgeError> {
    shared.bridge.cancel(id)?;
    Ok(Json(json!({"cancelled": id})).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyInput {
    grant_write: bool,
}

async fn apply(
    State(shared): State<Shared>,
    Path((id, proposal)): Path<(u64, String)>,
    Json(input): Json<ApplyInput>,
) -> Result<Response, BridgeError> {
    let application = shared
        .bridge
        .apply(id, &proposal, input.grant_write)
        .await?;
    Ok(Json(application).into_response())
}
