//! In-process bridge between the local UI and the Sanborn harness.
//!
//! The UI never runs a model or tool loop. Runs use `PlannedAgent`, required
//! Laya and the same tool assembly as `sb`, always read-only. A coder proposal
//! is applied only by a separate explicit request that executes a Rust-authored
//! plan (`apply_tadpole_patch` then an exact `read_file`) through Laya gates.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use sanborn_core::plan::{PlanConfig, PlanEvent, PlannedAgent};
use sanborn_core::provider::BudgetedProvider;
use sanborn_core::tadpole::{croaks_from_messages, ProposalView, Tadpoles};
use sanborn_core::{Policy, Provider, ToolContext, ToolRegistry};
use sanborn_laya::DecisionEngine;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::task::AbortHandle;

pub const MAX_PROMPT_BYTES: usize = 128_000;
const MAX_EVENTS: usize = 4_000;
const NOT_ROLLED_BACK: &str = "Completed writes are not rolled back.";

/// Builds a fresh inference provider for one run. Credentials stay inside it.
pub type ProviderFactory =
    Arc<dyn Fn() -> BoxFuture<'static, Result<Arc<dyn Provider>>> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct Limits {
    pub max_turns: usize,
    pub max_tokens: u32,
    pub max_tool_calls: usize,
    pub max_tadpoles: usize,
    pub max_inference_calls: usize,
    pub timeout_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_turns: 12,
            max_tokens: 4096,
            max_tool_calls: 64,
            max_tadpoles: 3,
            max_inference_calls: 24,
            timeout_secs: 180,
        }
    }
}

/// What stands behind reasoning, shown verbatim in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct Reasoning {
    pub kind: &'static str,
    pub label: String,
}

#[derive(Debug)]
pub enum BridgeError {
    BadRequest(String),
    Forbidden(String),
    Conflict(String),
    Gone(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::BadRequest(m) | Self::Forbidden(m) | Self::Conflict(m) | Self::Gone(m)) = self;
        f.write_str(m)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationStatus {
    Applying,
    /// Write succeeded and an exact reread matched the proposal under Laya gates.
    AppliedVerified,
    /// Write succeeded, but the reread did not match or could not be checked.
    AppliedUnverified,
    /// The guarded write did not happen (stale file, unready proposal, Laya stop).
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Application {
    pub status: ApplicationStatus,
    pub detail: String,
    pub events: Vec<Value>,
}

struct Run {
    id: u64,
    workspace: ToolContext,
    status: RunStatus,
    started: Instant,
    duration_ms: Option<u128>,
    events: Vec<Value>,
    dropped_events: usize,
    error: Option<String>,
    final_text: Option<String>,
    turns: Option<usize>,
    tool_calls: usize,
    croaks: Vec<Value>,
    task: Option<AbortHandle>,
    ledger: Option<Arc<BudgetedProvider>>,
    tools: Option<ToolRegistry>,
    tadpoles: Option<Arc<Tadpoles>>,
    applications: BTreeMap<String, Application>,
}

#[derive(Default)]
struct State {
    workspace: Option<ToolContext>,
    run: Option<Run>,
    next_id: u64,
    applying: bool,
}

pub struct Bridge {
    factory: ProviderFactory,
    laya: Arc<dyn DecisionEngine>,
    reasoning: Reasoning,
    limits: Limits,
    state: Mutex<State>,
}

impl Bridge {
    pub fn new(
        factory: ProviderFactory,
        laya: Arc<dyn DecisionEngine>,
        reasoning: Reasoning,
        limits: Limits,
    ) -> Arc<Self> {
        Arc::new(Self {
            factory,
            laya,
            reasoning,
            limits,
            state: Mutex::new(State::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("app state poisoned")
    }

    fn busy(state: &State) -> Option<&'static str> {
        if state.applying {
            Some("a proposal application is in progress")
        } else if state
            .run
            .as_ref()
            .is_some_and(|run| run.status == RunStatus::Running)
        {
            Some("a run is already in progress; cancel it or wait for it to finish")
        } else {
            None
        }
    }

    /// Explicit workspace selection. Rust canonicalizes and validates the path.
    pub fn select_workspace(&self, path: &str) -> Result<String, BridgeError> {
        if !std::path::Path::new(path).is_absolute() {
            return Err(BridgeError::BadRequest(
                "enter an absolute workspace directory path".into(),
            ));
        }
        let context = ToolContext::new(path)
            .map_err(|error| BridgeError::BadRequest(format!("{error:#}")))?;
        let mut state = self.lock();
        if let Some(reason) = Self::busy(&state) {
            return Err(BridgeError::Conflict(reason.into()));
        }
        let display = context.workspace().display().to_string();
        state.workspace = Some(context);
        Ok(display)
    }

    /// Starts one bounded, read-only run. Only one run or application at a time.
    pub fn start(self: &Arc<Self>, prompt: String) -> Result<u64, BridgeError> {
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err(BridgeError::BadRequest(format!(
                "prompt exceeds {MAX_PROMPT_BYTES} bytes"
            )));
        }
        if prompt.trim().is_empty() {
            return Err(BridgeError::BadRequest("prompt must not be empty".into()));
        }
        if prompt.contains('\0') {
            return Err(BridgeError::BadRequest(
                "prompt must not contain NUL bytes".into(),
            ));
        }
        let mut state = self.lock();
        if let Some(reason) = Self::busy(&state) {
            return Err(BridgeError::Conflict(reason.into()));
        }
        let workspace = state.workspace.clone().ok_or_else(|| {
            BridgeError::BadRequest("select a workspace before submitting a task".into())
        })?;
        state.next_id += 1;
        let id = state.next_id;
        // Replacing the previous run drops its tools and run-local proposals.
        state.run = Some(Run {
            id,
            workspace: workspace.clone(),
            status: RunStatus::Running,
            started: Instant::now(),
            duration_ms: None,
            events: Vec::new(),
            dropped_events: 0,
            error: None,
            final_text: None,
            turns: None,
            tool_calls: 0,
            croaks: Vec::new(),
            task: None,
            ledger: None,
            tools: None,
            tadpoles: None,
            applications: BTreeMap::new(),
        });
        let bridge = self.clone();
        let task = tokio::spawn(async move { bridge.execute(id, workspace, prompt).await });
        if let Some(run) = state.run.as_mut() {
            run.task = Some(task.abort_handle());
        }
        Ok(id)
    }

    fn with_run<T>(&self, id: u64, update: impl FnOnce(&mut Run) -> T) -> Option<T> {
        let mut state = self.lock();
        state
            .run
            .as_mut()
            .filter(|run| run.id == id && run.status == RunStatus::Running)
            .map(update)
    }

    fn record(&self, id: u64, event: &PlanEvent) {
        let Ok(value) = serde_json::to_value(event) else {
            return;
        };
        self.with_run(id, |run| {
            if run.events.len() < MAX_EVENTS {
                run.events.push(value);
            } else {
                run.dropped_events += 1;
            }
            run.tool_calls += count_tools(event);
        });
    }

    async fn execute(self: Arc<Self>, id: u64, workspace: ToolContext, prompt: String) {
        let limits = self.limits.clone();
        let operation = async {
            let inner = (self.factory)()
                .await
                .context("reasoning provider is not ready")?;
            let provider = Arc::new(BudgetedProvider::new(inner, limits.max_inference_calls));
            let (tools, tadpoles) = sanborn_cli::harness_tools(
                provider.clone(),
                self.laya.clone(),
                limits.max_tadpoles,
            )?;
            self.with_run(id, |run| {
                run.ledger = Some(provider.clone());
                run.tools = Some(tools.clone());
                run.tadpoles = Some(tadpoles);
            });
            let agent = PlannedAgent::new(
                provider.clone(),
                self.laya.clone(),
                tools,
                workspace,
                PlanConfig {
                    max_turns: limits.max_turns,
                    max_tokens: limits.max_tokens,
                    max_tool_calls: limits.max_tool_calls,
                    policy: Policy::read_only(),
                    ..PlanConfig::default()
                },
            );
            agent
                .run(&prompt, |event| match event {
                    // Like `sb`, report shared parent + Tadpole usage on completion.
                    PlanEvent::Finished {
                        turns, duration_ms, ..
                    } => self.record(
                        id,
                        &PlanEvent::Finished {
                            turns: *turns,
                            tool_calls: self.with_run(id, |run| run.tool_calls).unwrap_or(0),
                            usage: provider.stats().usage,
                            duration_ms: *duration_ms,
                        },
                    ),
                    _ => self.record(id, event),
                })
                .await
        };
        let result = tokio::time::timeout(Duration::from_secs(limits.timeout_secs), operation)
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("run deadline exceeded")));
        self.with_run(id, |run| {
            run.task = None;
            run.duration_ms = Some(run.started.elapsed().as_millis());
            match result {
                Ok(outcome) => {
                    run.status = RunStatus::Completed;
                    run.croaks = croaks_from_messages(&outcome.messages);
                    run.final_text = Some(outcome.final_text);
                    run.turns = Some(outcome.turns);
                }
                Err(error) => {
                    run.status = RunStatus::Failed;
                    run.error = Some(format!("{error:#}"));
                }
            }
        });
    }

    /// Cooperative cancellation: drops the run future, including Tadpole requests.
    pub fn cancel(&self, id: u64) -> Result<(), BridgeError> {
        let mut state = self.lock();
        let run = state
            .run
            .as_mut()
            .filter(|run| run.id == id)
            .ok_or_else(|| BridgeError::Gone("run is no longer current".into()))?;
        if run.status != RunStatus::Running {
            return Err(BridgeError::Conflict("run is not in progress".into()));
        }
        if let Some(task) = run.task.take() {
            task.abort();
        }
        run.status = RunStatus::Cancelled;
        run.duration_ms = Some(run.started.elapsed().as_millis());
        run.error = Some(
            "Cancelled by user. In-flight provider requests may still be billed; the run was read-only."
                .into(),
        );
        Ok(())
    }

    /// Aborts any active run; used on shutdown.
    pub fn shutdown(&self) {
        let id = self.lock().run.as_ref().map(|run| run.id);
        if let Some(id) = id {
            let _ = self.cancel(id);
        }
    }

    /// Applies one inspected proposal. `grant_write` is the explicit per-action
    /// write permission; without it Rust policy rejects the plan before any tool runs.
    pub async fn apply(
        &self,
        run_id: u64,
        proposal_id: &str,
        grant_write: bool,
    ) -> Result<Application, BridgeError> {
        let (workspace, tools, tadpoles, provider) = {
            let mut state = self.lock();
            if state.applying {
                return Err(BridgeError::Conflict(
                    "a proposal application is already in progress".into(),
                ));
            }
            let selected = state.workspace.as_ref().map(|c| c.workspace().to_owned());
            let run = state
                .run
                .as_mut()
                .filter(|run| run.id == run_id)
                .ok_or_else(|| {
                    BridgeError::Gone(
                        "stale proposal: it belonged to an earlier run and was discarded".into(),
                    )
                })?;
            if run.status != RunStatus::Completed {
                return Err(BridgeError::Conflict(
                    "proposals can only be applied after the run completes".into(),
                ));
            }
            if selected.as_deref() != Some(run.workspace.workspace()) {
                return Err(BridgeError::Gone(
                    "stale proposal: the selected workspace changed since this run".into(),
                ));
            }
            if run.applications.contains_key(proposal_id) {
                return Err(BridgeError::Conflict(
                    "this proposal was already attempted; one application attempt is allowed"
                        .into(),
                ));
            }
            let (Some(tools), Some(tadpoles), Some(provider)) =
                (run.tools.clone(), run.tadpoles.clone(), run.ledger.clone())
            else {
                return Err(BridgeError::Gone("run has no proposals".into()));
            };
            (run.workspace.clone(), tools, tadpoles, provider)
        };
        let proposal = tadpoles
            .proposals(&workspace)
            .into_iter()
            .find(|p| p.proposal_id == proposal_id)
            .ok_or_else(|| BridgeError::Gone("unknown or already consumed proposal".into()))?;
        let mut registry = ToolRegistry::new();
        for name in ["apply_tadpole_patch", "read_file"] {
            registry.register(
                tools
                    .get(name)
                    .ok_or_else(|| BridgeError::Gone(format!("{name} is unavailable")))?
                    .clone(),
            );
        }
        // No reasoning turn exists here, so write permission cannot reach a model plan.
        let agent = PlannedAgent::new(
            provider,
            self.laya.clone(),
            registry,
            workspace,
            PlanConfig {
                max_tool_calls: 2,
                policy: Policy {
                    allow_write: grant_write,
                    allow_execute: false,
                },
                ..PlanConfig::default()
            },
        );
        let plan = json!({"steps": [
            {"id": "apply", "tool": "apply_tadpole_patch",
             "input": {"proposal_id": proposal.proposal_id},
             "depends_on": [], "guard": {"kind": "tool_succeeded"}},
            {"id": "verify", "tool": "read_file", "input": {"path": proposal.path},
             "depends_on": ["apply"], "guard": {"kind": "text_equals", "value": proposal.content}}
        ]});
        {
            let mut state = self.lock();
            if let Some(reason) = Self::busy(&state) {
                return Err(BridgeError::Conflict(reason.into()));
            }
            state.applying = true;
        }
        let mut events = Vec::new();
        let result = agent
            .execute_trusted_plan(plan, |event| {
                if let Ok(value) = serde_json::to_value(event) {
                    events.push(value);
                }
            })
            .await;
        let mut state = self.lock();
        state.applying = false;
        let application = match result {
            // Validation failed before any tool ran: nothing consumed, nothing written.
            Err(error) if events.is_empty() => {
                return Err(BridgeError::Forbidden(format!(
                    "Rust denied the write: {error:#}. Grant write permission for this proposal to apply it."
                )));
            }
            Err(error) => Application {
                status: ApplicationStatus::Failed,
                detail: format!("{error:#}. {NOT_ROLLED_BACK}"),
                events,
            },
            Ok(output) => classify(&output, events),
        };
        if let Some(run) = state.run.as_mut().filter(|run| run.id == run_id) {
            run.applications
                .insert(proposal_id.to_owned(), application.clone());
        }
        Ok(application)
    }

    /// UI snapshot. Events are metadata-only; final text and croaks may quote
    /// workspace content and are served only to the token-holding local page.
    pub async fn snapshot(&self, after: usize) -> Value {
        let (mut value, pending) = {
            let state = self.lock();
            let run = state.run.as_ref().map(|run| {
                let stats = run.ledger.as_ref().map(|ledger| ledger.stats());
                let start = after.min(run.events.len());
                let proposals = match (&run.tadpoles, run.status) {
                    (Some(tadpoles), RunStatus::Completed) => tadpoles.proposals(&run.workspace),
                    _ => Vec::new(),
                };
                let value = json!({
                    "id": run.id,
                    "status": run.status,
                    "workspace": run.workspace.workspace().display().to_string(),
                    "elapsed_ms": run.duration_ms.unwrap_or_else(|| run.started.elapsed().as_millis()),
                    "events": &run.events[start..],
                    "next_event": run.events.len(),
                    "dropped_events": run.dropped_events,
                    "error": run.error,
                    "final_text": run.final_text,
                    "turns": run.turns,
                    "tool_calls": run.tool_calls,
                    "acceptance": "not_certified",
                    "inference_calls": stats.map(|s| s.calls),
                    "usage": stats.map(|s| s.usage),
                    "usage_complete": stats.map(|s| s.calls == s.completed_calls),
                    "croaks": run.croaks,
                    "applications": run.applications,
                });
                (value, (proposals, run.workspace.clone(), run.tools.clone()))
            });
            let (run, pending) = match run {
                Some((value, pending)) => (value, Some(pending)),
                None => (Value::Null, None),
            };
            (
                json!({
                    "reasoning": self.reasoning,
                    "limits": self.limits,
                    "laya_backend": self.laya.name(),
                    "workspace": state.workspace.as_ref().map(|c| c.workspace().display().to_string()),
                    "permission": {
                        "runs": "read_only",
                        "writes": "per_proposal_grant",
                        "applying": state.applying,
                    },
                    "run": run,
                }),
                pending,
            )
        };
        if let Some((proposals, workspace, Some(tools))) = pending {
            value["run"]["proposals"] = json!(proposal_views(proposals, &workspace, &tools).await);
        } else if !value["run"].is_null() {
            value["run"]["proposals"] = json!([]);
        }
        value
    }
}

/// Adds a Rust-observed staleness check: does the file still hold the bytes the coder read?
async fn proposal_views(
    proposals: Vec<ProposalView>,
    workspace: &ToolContext,
    tools: &ToolRegistry,
) -> Vec<Value> {
    let mut views = Vec::new();
    for proposal in proposals {
        let current = match tools.get("read_file") {
            Some(reader) => reader.call(json!({"path": proposal.path}), workspace).await,
            None => Err(anyhow::anyhow!("read_file unavailable")),
        };
        let mut view = json!(proposal);
        view["stale"] = json!(current
            .as_ref()
            .map_or(true, |c| *c != proposal.expected_content));
        views.push(view);
    }
    views
}

fn classify(output: &str, events: Vec<Value>) -> Application {
    let plan: Value = serde_json::from_str(output).unwrap_or(Value::Null);
    let results = plan["results"].as_array().cloned().unwrap_or_default();
    let step = |id: &str| results.iter().find(|r| r["step"] == id).cloned();
    let (status, detail) = match (step("apply"), step("verify")) {
        _ if plan["plan_complete"] == true => (
            ApplicationStatus::AppliedVerified,
            "Written through apply_tadpole_patch; an exact reread matched the proposal. \
             This verifies file bytes only, not behavior or tests."
                .to_owned(),
        ),
        (Some(apply), _) if apply["is_error"] == true => (
            ApplicationStatus::Failed,
            format!(
                "Guarded application failed; the file was not changed by this attempt. {} \
                 The proposal is consumed; summon a fresh coder.",
                apply["output"].as_str().unwrap_or("")
            ),
        ),
        (Some(_), _) => (
            ApplicationStatus::AppliedUnverified,
            format!(
                "The write succeeded but the exact reread did not pass ({}). Inspect the file. {NOT_ROLLED_BACK}",
                plan["interrupted"]
            ),
        ),
        (None, _) => (
            ApplicationStatus::Failed,
            format!(
                "Laya stopped the application before writing ({}). The proposal was not consumed by a write.",
                plan["interrupted"]
            ),
        ),
    };
    Application {
        status,
        detail,
        events,
    }
}

fn count_tools(event: &PlanEvent) -> usize {
    match event {
        PlanEvent::ToolFinished { .. } => 1,
        PlanEvent::Tadpole { event, .. } => count_tools(event),
        _ => 0,
    }
}
