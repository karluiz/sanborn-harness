use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use sanborn_core::plan::{PlanConfig, PlannedAgent};
use sanborn_core::{Policy, ToolContext};
use sanborn_laya::LocalLaya;
use sanborn_models::AzureProvider;

mod setup;
mod terminal;

#[derive(Subcommand)]
enum Command {
    /// Initialize Sanborn inference configuration and its required Laya layer.
    Init(setup::InitArgs),
}

#[derive(Parser)]
#[command(
    name = "sb",
    version,
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true,
    about = "Sanborn: bounded Rust plans with Laya typed decisions"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// Task. Tool results from the selected workspace are sent to Azure.
    #[arg(short = 'p', long, required = true)]
    prompt: Option<String>,
    /// Workspace boundary for file tools.
    #[arg(short = 'w', long, default_value = ".")]
    workspace: PathBuf,
    /// Allow file writes. Default is read-only; shell execution is not implemented.
    #[arg(long)]
    allow_write: bool,
    /// HTTPS origin. Falls back to the local Sanborn system keyring entry.
    #[arg(long, env = "AZURE_OPENAI_ENDPOINT")]
    endpoint: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_DEPLOYMENT")]
    deployment: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_API_VERSION")]
    api_version: Option<String>,
    #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=100))]
    max_turns: u32,
    #[arg(long, default_value_t = 4096, value_parser = clap::value_parser!(u32).range(1..=32768))]
    max_tokens: u32,
    #[arg(long, default_value_t = 64, value_parser = clap::value_parser!(u32).range(1..=1000))]
    max_tool_calls: u32,
    /// Overall run deadline in seconds.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
    /// Emit metadata-only JSON events on stderr. File contents and tool inputs are not logged.
    #[arg(long)]
    json_events: bool,
    /// Disable terminal color and animation (also useful for accessibility).
    #[arg(long, global = true)]
    plain: bool,
}

async fn keyring_value(service: &str) -> Result<String> {
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

async fn credential() -> Result<String> {
    match std::env::var("AZURE_OPENAI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => Ok(key),
        Ok(_) => bail!("AZURE_OPENAI_API_KEY is empty"),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("AZURE_OPENAI_API_KEY is not valid Unicode")
        }
        Err(std::env::VarError::NotPresent) => keyring_value("azure-inference").await,
    }
}

async fn run(args: Args) -> Result<()> {
    let style = terminal::Style::detect(args.plain, args.json_events);
    let feedback = Mutex::new(terminal::Feedback::new(
        std::io::stderr(),
        style,
        args.json_events,
    ));
    feedback
        .lock()
        .expect("feedback lock poisoned")
        .phase("Sanborn - preparing Rust + Laya");
    let operation = run_with_feedback(args, &feedback);
    let cancellable = async {
        tokio::select! {
            result = operation => result,
            signal = tokio::signal::ctrl_c() => {
                signal.context("cannot listen for cancellation")?;
                bail!("run cancelled; completed writes are not rolled back");
            }
        }
    };
    tokio::pin!(cancellable);
    let result = if style.animate {
        let cadence = Duration::from_millis(125);
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + cadence, cadence);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                result = &mut cancellable => break result,
                _ = ticks.tick() => feedback.lock().expect("feedback lock poisoned").tick(),
            }
        }
    } else {
        cancellable.await
    };
    let rendering = feedback.lock().expect("feedback lock poisoned").finish();
    if let Err(error) = rendering {
        if result.is_err() {
            eprintln!("sb: terminal feedback failed: {error}");
        } else {
            return Err(error).context("terminal feedback failed");
        }
    }
    result
}

async fn run_with_feedback(
    args: Args,
    feedback: &Mutex<terminal::Feedback<std::io::Stderr>>,
) -> Result<()> {
    if let Some(Command::Init(init)) = args.command {
        return initialize(init, feedback).await;
    }
    let prompt = args.prompt.context("provide a task with sb -p")?;
    if prompt.trim().is_empty() {
        bail!("prompt must not be empty");
    }
    let context = ToolContext::new(&args.workspace)?;
    let saved = setup::load()?;
    let endpoint = match args
        .endpoint
        .or_else(|| saved.as_ref().map(|c| c.reasoning.endpoint.clone()))
    {
        Some(endpoint) => endpoint,
        None => keyring_value("azure-inference-endpoint")
            .await
            .context("run sb init, or set AZURE_OPENAI_ENDPOINT or --endpoint")?,
    };
    let deployment = args
        .deployment
        .or_else(|| saved.as_ref().map(|c| c.reasoning.deployment.clone()))
        .unwrap_or_else(|| setup::DEFAULT_DEPLOYMENT.into());
    let api_version = args
        .api_version
        .or_else(|| saved.as_ref().map(|c| c.reasoning.api_version.clone()))
        .unwrap_or_else(|| setup::DEFAULT_API_VERSION.into());
    setup::check_laya()?;
    let key = credential().await?;
    let provider = Arc::new(AzureProvider::new(
        &endpoint,
        &deployment,
        &api_version,
        &key,
    )?);
    let config = PlanConfig {
        max_turns: args.max_turns as usize,
        max_tokens: args.max_tokens,
        max_tool_calls: args.max_tool_calls as usize,
        policy: Policy {
            allow_write: args.allow_write,
            allow_execute: false,
        },
        ..PlanConfig::default()
    };
    let agent = PlannedAgent::new(
        provider,
        Arc::new(LocalLaya),
        sanborn_tools::workspace_tools(),
        context,
        config,
    );
    let operation = agent.run(&prompt, |event| {
        feedback
            .lock()
            .expect("feedback lock poisoned")
            .event(event)
    });
    let outcome = tokio::time::timeout(Duration::from_secs(args.timeout), operation)
        .await
        .context("run deadline exceeded; completed writes are not rolled back")??;
    println!("{}", outcome.final_text);
    Ok(())
}

async fn initialize(
    args: setup::InitArgs,
    feedback: &Mutex<terminal::Feedback<std::io::Stderr>>,
) -> Result<()> {
    let saved = if args.force { None } else { setup::load()? };
    let endpoint = match args.endpoint.or_else(|| saved.as_ref().map(|c| c.reasoning.endpoint.clone())) {
        Some(endpoint) => endpoint,
        None => keyring_value("azure-inference-endpoint").await
            .context("sb init needs --endpoint or AZURE_OPENAI_ENDPOINT, or a configured endpoint keyring entry")?,
    };
    let deployment = args
        .deployment
        .or_else(|| saved.as_ref().map(|c| c.reasoning.deployment.clone()))
        .unwrap_or_else(|| setup::DEFAULT_DEPLOYMENT.into());
    let api_version = args
        .api_version
        .or_else(|| saved.as_ref().map(|c| c.reasoning.api_version.clone()))
        .unwrap_or_else(|| setup::DEFAULT_API_VERSION.into());
    let config = setup::Config::new(endpoint, deployment, api_version);
    config.validate()?;
    setup::check_laya()?;
    let key = credential()
        .await
        .context("Sanborn setup needs a credential from the environment or system keyring")?;
    let provider = AzureProvider::new(
        &config.reasoning.endpoint,
        &config.reasoning.deployment,
        &config.reasoning.api_version,
        &key,
    )?;
    if args.check_inference {
        feedback
            .lock()
            .expect("feedback lock poisoned")
            .phase("Sanborn init - checking inference");
        use sanborn_core::{CompletionRequest, ContentBlock, Message, Provider, StopReason};
        let messages = [Message::user_text("Reply with exactly: sanborn-init-ok")];
        let response = provider
            .complete(&CompletionRequest {
                system: "",
                messages: &messages,
                tools: &[],
                max_tokens: 64,
            })
            .await
            .context("synthetic inference readiness check failed; configuration was not saved")?;
        if response.stop_reason != StopReason::EndTurn
            || response.content.len() != 1
            || !matches!(&response.content[0], ContentBlock::Text { text } if text.trim() == "sanborn-init-ok")
        {
            bail!("inference readiness check returned an unexpected result; configuration was not saved");
        }
    }
    let path = setup::save(&config, args.force)?;
    feedback.lock().expect("feedback lock poisoned").stop();
    println!("Sanborn initialized: {}", path.display());
    println!("Laya: required local-evidence backend; Continue/Replan/Absent self-check passed.");
    println!("Credential: available; never stored in configuration.");
    if args.check_inference {
        println!("Reasoning: synthetic chat probe passed; tool compatibility is not certified by this probe.");
    } else {
        println!("Reasoning: configuration validated locally; network connectivity not checked (use --check-inference).");
    }
    println!("Run sb -p \"your task\" from a workspace approved for the inference endpoint. Writes remain opt-in.");
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sb: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sb_prompt_and_rejects_unbounded_limits() {
        let args =
            Args::try_parse_from(["sb", "-p", "inspect", "-w", "/tmp", "--allow-write"]).unwrap();
        assert_eq!(args.prompt.as_deref(), Some("inspect"));
        assert!(args.allow_write);
        assert!(Args::try_parse_from(["sb", "-p", "inspect", "--max-turns", "0"]).is_err());
        assert!(Args::try_parse_from(["sb", "-p", "inspect", "--timeout", "0"]).is_err());
        assert!(Args::try_parse_from(["sb"]).is_err());
        assert!(Args::try_parse_from(["sb", "init"]).is_ok());
        assert!(Args::try_parse_from(["sb", "init", "--check-inference"]).is_ok());
        assert!(Args::try_parse_from(["sb", "-p", "inspect", "init"]).is_err());
    }
}
