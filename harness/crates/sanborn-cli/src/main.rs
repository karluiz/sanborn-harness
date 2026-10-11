use std::io::{IsTerminal, Read, Write};
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

mod terminal;

use sanborn_cli::{credential, doctor, keyring_value, setup};

#[derive(Subcommand)]
enum Command {
    /// Initialize Sanborn inference configuration and its required Laya layer.
    Init(setup::InitArgs),
    /// Check local Sanborn readiness without inference requests or configuration changes.
    Doctor(doctor::DoctorArgs),
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
    /// Task; use "-" to read a UTF-8 prompt from stdin. Selected tool results go to Azure.
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
    /// Emit one versioned JSON final result on stdout (does not certify acceptance).
    #[arg(long)]
    json: bool,
    /// Maximum read-only tadpoles summoned across a run (at most three per group).
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=8))]
    max_tadpoles: u32,
    /// Shared inference-call ceiling for the parent and every tadpole.
    #[arg(long, default_value_t = 24, value_parser = clap::value_parser!(u32).range(1..=200))]
    max_inference_calls: u32,
    /// Disable terminal color and animation (also useful for accessibility).
    #[arg(long, global = true)]
    plain: bool,
}

async fn run(
    args: Args,
    ledger: &Mutex<Option<Arc<sanborn_core::provider::BudgetedProvider>>>,
) -> Result<()> {
    let diagnostic_json = matches!(&args.command, Some(Command::Doctor(options)) if options.json);
    let style =
        terminal::Style::detect(args.plain || args.json, args.json_events || diagnostic_json);
    let feedback = Mutex::new(terminal::Feedback::new(
        std::io::stderr(),
        style,
        args.json_events || diagnostic_json,
    ));
    feedback
        .lock()
        .expect("feedback lock poisoned")
        .phase("Sanborn - preparing Rust + Laya");
    let operation = run_with_feedback(args, &feedback, ledger);
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
    if let Some(value) = result? {
        write_json(&value)?;
    }
    Ok(())
}

async fn run_with_feedback(
    args: Args,
    feedback: &Mutex<terminal::Feedback<std::io::Stderr>>,
    ledger: &Mutex<Option<Arc<sanborn_core::provider::BudgetedProvider>>>,
) -> Result<Option<serde_json::Value>> {
    if let Some(command) = args.command {
        return match command {
            Command::Init(init) => initialize(init, feedback).await.map(|_| None),
            Command::Doctor(options) => {
                let report = doctor::inspect(
                    options.workspace,
                    options.endpoint,
                    options.deployment,
                    options.api_version,
                )
                .await;
                feedback.lock().expect("feedback lock poisoned").stop();
                if options.json {
                    write_json(&report)?;
                } else {
                    for check in &report.checks {
                        println!(
                            "{} {}: {}",
                            if check.ready { "+" } else { "!" },
                            check.name,
                            check.detail
                        );
                    }
                    println!("Inference connectivity: not checked (doctor is offline).");
                }
                if !report.ready {
                    bail!("local readiness checks failed; see doctor report");
                }
                Ok(None)
            }
        };
    }
    let prompt = tokio::time::timeout(
        Duration::from_secs(args.timeout),
        read_prompt(args.prompt.context("provide a task with sb -p")?),
    )
    .await
    .context("stdin prompt deadline exceeded")??;
    let context = ToolContext::new(&args.workspace)?;
    let azure =
        sanborn_cli::configured_azure(args.endpoint, args.deployment, args.api_version).await?;
    let provider = Arc::new(sanborn_core::provider::BudgetedProvider::new(
        Arc::new(azure),
        args.max_inference_calls as usize,
    ));
    *ledger.lock().expect("ledger slot poisoned") = Some(provider.clone());
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
    let laya = Arc::new(LocalLaya);
    let (tools, _) =
        sanborn_cli::harness_tools(provider.clone(), laya.clone(), args.max_tadpoles as usize)?;
    let agent = PlannedAgent::new(provider.clone(), laya, tools, context, config);
    let started = std::time::Instant::now();
    let mut tool_calls = 0usize;
    let operation = agent.run(&prompt, |event| {
        tool_calls += count_tools(event);
        let enriched;
        let event = if let sanborn_core::plan::PlanEvent::Finished {
            turns, duration_ms, ..
        } = event
        {
            enriched = sanborn_core::plan::PlanEvent::Finished {
                turns: *turns,
                tool_calls,
                usage: provider.stats().usage,
                duration_ms: *duration_ms,
            };
            &enriched
        } else {
            event
        };
        feedback
            .lock()
            .expect("feedback lock poisoned")
            .event(event)
    });
    let outcome = tokio::time::timeout(Duration::from_secs(args.timeout), operation)
        .await
        .context("run deadline exceeded; completed writes are not rolled back")??;
    if args.json {
        return Ok(Some(serde_json::json!({
            "schema_version": 1, "status": "completed", "acceptance": "not_certified",
            "final_text": outcome.final_text, "turns": outcome.turns, "tool_calls": tool_calls,
            "usage": provider.stats().usage, "inference_calls": provider.stats().calls,
            "usage_complete": provider.stats().calls == provider.stats().completed_calls,
            "duration_ms": started.elapsed().as_millis(),
            "stop_reason": "end_turn", "laya_backend": "local-evidence"
        })));
    } else {
        println!("{}", outcome.final_text);
    }
    Ok(None)
}

fn count_tools(event: &sanborn_core::plan::PlanEvent) -> usize {
    match event {
        sanborn_core::plan::PlanEvent::ToolFinished { .. } => 1,
        sanborn_core::plan::PlanEvent::Tadpole { event, .. } => count_tools(event),
        _ => 0,
    }
}

const MAX_PROMPT_BYTES: usize = 128_000;

async fn read_prompt(prompt: String) -> Result<String> {
    if prompt == "-" {
        if std::io::stdin().is_terminal() {
            bail!("sb -p - requires piped stdin or file redirection");
        }
        // A detached input thread avoids Tokio's blocking-stdin shutdown wait on cancellation.
        let (sender, receiver) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("sb-stdin".into())
            .spawn(move || {
                let _receiver_closed = sender.send(read_stdin(std::io::stdin().lock()));
            })
            .context("cannot start stdin reader")?;
        receiver
            .await
            .context("stdin reader terminated without a result")?
    } else {
        validate_prompt(prompt)
    }
}

fn validate_prompt(prompt: String) -> Result<String> {
    if prompt.len() > MAX_PROMPT_BYTES {
        bail!("prompt exceeds {MAX_PROMPT_BYTES} bytes");
    }
    if prompt.trim().is_empty() {
        bail!("prompt must not be empty");
    }
    if prompt.contains('\0') {
        bail!("prompt must not contain NUL bytes");
    }
    Ok(prompt)
}

fn read_stdin(reader: impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_PROMPT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("cannot read prompt from stdin")?;
    validate_prompt(String::from_utf8(bytes).context("stdin prompt is not UTF-8")?)
}

fn write_json(value: &impl serde::Serialize) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).context("cannot write JSON output")?;
    writeln!(stdout).context("cannot finish JSON output")?;
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
    let args = Args::parse();
    let json_error = args.json && args.command.is_none();
    let ledger = Mutex::new(None);
    match run(args, &ledger).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if json_error {
                let stats = ledger
                    .lock()
                    .expect("ledger slot poisoned")
                    .as_ref()
                    .map(|p| p.stats());
                if let Err(output_error) = write_json(&serde_json::json!({
                    "schema_version": 1, "status": "error", "acceptance": "not_certified",
                    "error": format!("{error:#}"),
                    "inference_calls": stats.map(|s| s.calls),
                    "usage": stats.map(|s| s.usage),
                    "usage_complete": stats.map(|s| s.calls == s.completed_calls),
                    "note": "Completed writes are not rolled back. Usage counts received responses only; failed or cancelled requests may still incur provider charges."
                })) {
                    eprintln!("sb: {output_error:#}");
                }
            }
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
        assert!(Args::try_parse_from(["sb", "-p", "inspect", "--max-tadpoles", "9"]).is_err());
        assert!(
            Args::try_parse_from(["sb", "-p", "inspect", "--max-inference-calls", "0"]).is_err()
        );
        assert!(Args::try_parse_from(["sb"]).is_err());
        assert!(Args::try_parse_from(["sb", "init"]).is_ok());
        assert!(Args::try_parse_from(["sb", "init", "--check-inference"]).is_ok());
        assert!(Args::try_parse_from(["sb", "-p", "inspect", "init"]).is_err());
        assert!(Args::try_parse_from(["sb", "doctor", "--json"]).is_ok());
        assert!(Args::try_parse_from(["sb", "-p", "-", "--json", "--json-events"]).is_ok());
    }

    #[tokio::test]
    async fn stdin_preserves_text_and_rejects_invalid_prompts() {
        assert_eq!(
            read_stdin("hello\nworld\n".as_bytes()).unwrap(),
            "hello\nworld\n"
        );
        for bytes in [
            vec![],
            vec![b' '; 8],
            vec![0xff],
            vec![0],
            vec![b'x'; MAX_PROMPT_BYTES + 1],
        ] {
            assert!(read_stdin(bytes.as_slice()).is_err());
        }
        assert!(validate_prompt(" ".into()).is_err());
        assert!(validate_prompt("x".repeat(MAX_PROMPT_BYTES + 1)).is_err());
    }
}
