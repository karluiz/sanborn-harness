//! Fixed reasoning script for the offline demo. It replaces only the model:
//! Rust plan validation, Laya, Tadpole contracts, tools and budgets run for real.

use std::sync::Arc;

use sanborn_core::testing::{text_response, tool_response, ScriptedProvider};
use sanborn_core::CompletionResponse;
use serde_json::{json, Value};

use crate::bridge::{ProviderFactory, Reasoning};

pub const FIXTURE_FILE: &str = "greeting.txt";
pub const FIXTURE_BEFORE: &str = "hello old\n";
pub const FIXTURE_AFTER: &str = "hello new\n";

fn plan(steps: Value) -> CompletionResponse {
    tool_response(&[("plan", "submit_plan", json!({ "steps": steps }))])
}

/// Parent summons one coder; the coder reads and proposes; the parent reports.
/// Ignores the prompt. Works only against the `examples/app` greeting fixture.
pub fn coder_script() -> Vec<CompletionResponse> {
    vec![
        plan(json!([{"id": "summon", "tool": "summon_tadpoles", "depends_on": [],
            "guard": {"kind": "tool_succeeded"},
            "input": {"tadpoles": [{"id": "code", "specialty": "coder", "scope": ".",
                "objective": "Change greeting.txt so it reads hello new"}]}}])),
        plan(json!([{"id": "read", "tool": "read_file", "depends_on": [],
            "guard": {"kind": "tool_succeeded"}, "input": {"path": FIXTURE_FILE}}])),
        plan(json!([{"id": "propose", "tool": "propose_patch", "depends_on": [],
            "guard": {"kind": "patch_proposal"},
            "input": {"path": FIXTURE_FILE, "expected_content": FIXTURE_BEFORE,
                "content": FIXTURE_AFTER}}])),
        text_response("Proposed replacing `hello old` with `hello new` in greeting.txt. Not applied; no tests run."),
        text_response(
            "A coder tadpole proposed one replacement for greeting.txt (proposal code-patch). \
             This run was read-only, so nothing was written. Inspect the proposal and apply it \
             only if you grant write permission; no tests were run.",
        ),
    ]
}

pub fn factory() -> ProviderFactory {
    Arc::new(|| {
        Box::pin(async {
            Ok(Arc::new(ScriptedProvider::new(coder_script())) as Arc<dyn sanborn_core::Provider>)
        })
    })
}

pub fn reasoning() -> Reasoning {
    Reasoning {
        kind: "synthetic",
        label: "Synthetic script: a fixed reasoning script stands in for the model and ignores \
                your prompt. Rust plans, Laya gates, Tadpole contracts, tools and budgets run for real."
            .into(),
    }
}
