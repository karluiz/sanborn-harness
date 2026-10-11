//! Shared Sanborn wiring for `sb` and local front ends such as `sanborn-app`.
//!
//! Configuration precedence, credential lookup, the required Laya check and the
//! tool/Tadpole assembly live here so every front end runs the same harness.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use sanborn_core::tadpole::Tadpoles;
use sanborn_core::{Provider, ToolKind, ToolRegistry};
use sanborn_laya::DecisionEngine;

pub use sanborn_core::ToolContext;
pub use sanborn_models::AzureProvider;

pub mod doctor;
pub mod setup;

pub async fn keyring_value(service: &str) -> Result<String> {
    let mut command = tokio::process::Command::new("secret-tool");
    command
        .args(["lookup", "application", "sanborn", "service", service])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .context("system keyring lookup timed out")?
        .context(
            "cannot run secret-tool; use Azure environment configuration or install a Secret Service client",
        )?;
    if !output.status.success() {
        bail!("system keyring lookup failed; unlock it or use Azure environment configuration");
    }
    let value = String::from_utf8(output.stdout).context("keyring value is not UTF-8")?;
    if value.trim().is_empty() {
        bail!("Sanborn keyring entry is missing; use Azure environment configuration");
    }
    Ok(value.trim().to_owned())
}

pub async fn credential() -> Result<String> {
    match std::env::var("AZURE_OPENAI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => Ok(key),
        Ok(_) => bail!("AZURE_OPENAI_API_KEY is empty"),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("AZURE_OPENAI_API_KEY is not valid Unicode")
        }
        Err(std::env::VarError::NotPresent) => keyring_value("azure-inference").await,
    }
}

pub async fn resolve_reasoning(
    endpoint: Option<String>,
    deployment: Option<String>,
    api_version: Option<String>,
    saved: Option<&setup::Config>,
) -> Result<setup::Reasoning> {
    let endpoint = match endpoint.or_else(|| saved.map(|c| c.reasoning.endpoint.clone())) {
        Some(value) => value,
        None => keyring_value("azure-inference-endpoint")
            .await
            .context("run sb init, or set AZURE_OPENAI_ENDPOINT or --endpoint")?,
    };
    Ok(setup::Reasoning {
        endpoint,
        deployment: deployment
            .or_else(|| saved.map(|c| c.reasoning.deployment.clone()))
            .unwrap_or_else(|| setup::DEFAULT_DEPLOYMENT.into()),
        api_version: api_version
            .or_else(|| saved.map(|c| c.reasoning.api_version.clone()))
            .unwrap_or_else(|| setup::DEFAULT_API_VERSION.into()),
    })
}

/// Resolves saved/flag/environment configuration, runs Laya readiness and builds
/// the Azure provider. The credential stays inside the returned provider.
pub async fn configured_azure(
    endpoint: Option<String>,
    deployment: Option<String>,
    api_version: Option<String>,
) -> Result<AzureProvider> {
    let saved = setup::load()?;
    let reasoning = resolve_reasoning(endpoint, deployment, api_version, saved.as_ref()).await?;
    setup::check_laya()?;
    let key = credential().await?;
    AzureProvider::new(
        &reasoning.endpoint,
        &reasoning.deployment,
        &reasoning.api_version,
        &key,
    )
}

/// The parent tool registry every front end uses: workspace tools, read-only
/// Tadpoles sharing `provider`, and parent-only `apply_tadpole_patch`.
/// The returned `Tadpoles` handle lets a front end inspect run-local proposals.
pub fn harness_tools(
    provider: Arc<dyn Provider>,
    laya: Arc<dyn DecisionEngine>,
    max_tadpoles: usize,
) -> Result<(ToolRegistry, Arc<Tadpoles>)> {
    let mut tools = sanborn_tools::workspace_tools();
    let mut read_tools = ToolRegistry::new();
    for tool in tools.iter().filter(|tool| tool.kind() == ToolKind::Read) {
        read_tools.register(tool.clone());
    }
    let tadpoles = Arc::new(Tadpoles::new(provider, laya, read_tools, max_tadpoles)?);
    tools.register(Arc::new(
        tadpoles.patch_application_tool(
            tools
                .get("write_file")
                .context("guarded writer missing")?
                .clone(),
        )?,
    ));
    tools.register(tadpoles.clone());
    Ok((tools, tadpoles))
}
