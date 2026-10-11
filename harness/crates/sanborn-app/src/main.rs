use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use sanborn_app::bridge::{Bridge, Limits, ProviderFactory, Reasoning};
use sanborn_app::{server, synthetic};
use sanborn_laya::LocalLaya;

#[derive(Parser)]
#[command(
    name = "sanborn-app",
    version,
    about = "Local web app for Sanborn. Binds 127.0.0.1 only; do not expose it."
)]
struct Args {
    /// Preselect a workspace; it can still be changed explicitly in the app.
    #[arg(short = 'w', long)]
    workspace: Option<PathBuf>,
    /// Loopback port; 0 picks a free port.
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Replace the model with a fixed offline script (demo and tests only).
    #[arg(long)]
    synthetic: bool,
    #[arg(long, env = "AZURE_OPENAI_ENDPOINT")]
    endpoint: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_DEPLOYMENT")]
    deployment: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_API_VERSION")]
    api_version: Option<String>,
    /// Shared inference-call ceiling for the parent and every tadpole.
    #[arg(long, default_value_t = 24, value_parser = clap::value_parser!(u32).range(1..=200))]
    max_inference_calls: u32,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=8))]
    max_tadpoles: u32,
    /// Overall run deadline in seconds.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let (factory, reasoning): (ProviderFactory, Reasoning) = if args.synthetic {
        (synthetic::factory(), synthetic::reasoning())
    } else {
        let (endpoint, deployment, api_version) =
            (args.endpoint, args.deployment, args.api_version);
        (
            Arc::new(move || {
                let (endpoint, deployment, api_version) =
                    (endpoint.clone(), deployment.clone(), api_version.clone());
                Box::pin(async move {
                    let azure =
                        sanborn_cli::configured_azure(endpoint, deployment, api_version).await?;
                    Ok(Arc::new(azure) as Arc<dyn sanborn_core::Provider>)
                })
            }),
            Reasoning {
                kind: "azure",
                label: "Azure inference from sb init, environment or keyring. Resolved by Rust at \
                        each run start; the credential never reaches this page. The prompt and \
                        selected tool results are sent to the endpoint."
                    .into(),
            },
        )
    };
    sanborn_cli::setup::check_laya().context("required Laya readiness check failed")?;
    let bridge = Bridge::new(
        factory,
        Arc::new(LocalLaya),
        reasoning,
        Limits {
            max_tadpoles: args.max_tadpoles as usize,
            max_inference_calls: args.max_inference_calls as usize,
            timeout_secs: args.timeout,
            ..Limits::default()
        },
    );
    if let Some(path) = args.workspace {
        let absolute = std::path::absolute(&path).context("cannot resolve workspace path")?;
        bridge
            .select_workspace(&absolute.to_string_lossy())
            .map_err(|error| anyhow::anyhow!("{error}"))?;
    }
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, args.port)))
            .await
            .context("cannot bind 127.0.0.1")?;
    let port = listener.local_addr()?.port();
    let token = server::launch_token()?;
    println!("Sanborn app: http://127.0.0.1:{port}/#token={token}");
    println!(
        "Loopback only. Anyone with this link can drive Sanborn as you; do not share or expose it."
    );
    println!("Ctrl+C stops the app and cancels an active run.");
    let shutdown_bridge = bridge.clone();
    axum::serve(listener, server::router(bridge, token, port))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_bridge.shutdown();
        })
        .await
        .context("server failed")
}
