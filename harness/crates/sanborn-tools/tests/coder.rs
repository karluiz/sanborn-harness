use std::sync::Arc;

use sanborn_core::plan::{PlanConfig, PlanEvent, PlannedAgent};
use sanborn_core::tadpole::Tadpoles;
use sanborn_core::testing::{text_response, tool_response, ScriptedProvider};
use sanborn_core::{Policy, Tool, ToolContext, ToolKind, ToolRegistry};
use sanborn_laya::{Decision, DecisionEngine, DecisionRequest, LocalLaya};
use serde_json::{json, Value};

fn step(id: &str, tool: &str, input: Value, guard: &str) -> Value {
    json!({"id":id,"tool":tool,"input":input,"depends_on":[],"guard":{"kind":guard}})
}

struct DenyPatch;
impl DecisionEngine for DenyPatch {
    fn name(&self) -> &str {
        "deny-patch-fixture"
    }
    fn decide(&self, request: &DecisionRequest<'_>) -> Decision {
        if request
            .previous
            .as_ref()
            .is_some_and(|(_, guard)| matches!(guard, sanborn_laya::EvidenceGuard::PatchProposal))
        {
            Decision::Replan {
                reason: "fixture denies patch evidence".into(),
            }
        } else {
            LocalLaya.decide(request)
        }
    }
}

struct DenyInventory;
impl DecisionEngine for DenyInventory {
    fn name(&self) -> &str {
        "deny-inventory-fixture"
    }
    fn decide(&self, request: &DecisionRequest<'_>) -> Decision {
        if request.previous.as_ref().is_some_and(|(_, guard)| {
            matches!(guard, sanborn_laya::EvidenceGuard::Discovery { .. })
        }) {
            Decision::Replan {
                reason: "fixture denies baseline".into(),
            }
        } else {
            LocalLaya.decide(request)
        }
    }
}

#[tokio::test]
async fn coder_contract_and_parent_guarded_application() {
    for case in [
        "apply",
        "stale",
        "policy",
        "no-read",
        "wrong-old",
        "outside",
        "protected",
        "oversized",
        "no-change",
        "wrong-guard",
        "laya-denied",
        "no-proposal",
        "baseline-denied",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "old\n").unwrap();
        std::fs::write(root.path().join("outside.rs"), "old\n").unwrap();
        std::fs::write(root.path().join("src/.env"), "private fixture\n").unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        let path = match case {
            "outside" => "outside.rs",
            "protected" => "src/.env",
            _ => "src/lib.rs",
        };
        let mut responses = Vec::new();
        if case != "no-read" && case != "no-proposal" {
            responses.push(tool_response(&[(
                "read",
                "submit_plan",
                json!({"steps":[
                step("read","read_file",json!({"path":path}),"tool_succeeded")]}),
            )]));
        }
        if case != "no-proposal" {
            responses.push(tool_response(&[("patch","submit_plan",json!({"steps":[
                step("patch","propose_patch",json!({"path":path,
                    "expected_content":if case=="wrong-old" {"invented\n"} else {"old\n"},
                    "content":if case=="oversized" {"x".repeat(8001)} else if case=="no-change" {"old\n".into()} else {"new\n".into()}
                }),if case=="wrong-guard" {"tool_succeeded"} else {"patch_proposal"})]}))]));
        }
        responses.push(text_response("Proposal report; no execution."));
        let provider = Arc::new(ScriptedProvider::new(responses));
        let all = sanborn_tools::workspace_tools();
        let mut reads = ToolRegistry::new();
        for tool in all.iter().filter(|t| t.kind() == ToolKind::Read) {
            reads.register(tool.clone());
        }
        let laya: Arc<dyn DecisionEngine> = if case == "baseline-denied" {
            Arc::new(DenyInventory)
        } else if case == "laya-denied" {
            Arc::new(DenyPatch)
        } else {
            Arc::new(LocalLaya)
        };
        let tadpoles = Tadpoles::new(provider.clone(), laya, reads, 1).unwrap();
        let apply = Arc::new(
            tadpoles
                .patch_application_tool(all.get("write_file").unwrap().clone())
                .unwrap(),
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let output=tadpoles.call_with_events(json!({"tadpoles":[
            {"id":"code","specialty":"coder","scope":"src","objective":"Replace old with new in lib.rs"}
        ]}),&ctx,tx).await.unwrap();
        let croaks: Value = serde_json::from_str(&output).unwrap();
        let valid = matches!(case, "apply" | "stale" | "policy");
        assert_eq!(
            croaks["croaks"][0]["status"],
            if valid { "returned" } else { "blocked" },
            "{case}: {output}"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
            "old\n"
        );
        let events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
        assert!(events.iter().any(|e| matches!(e, PlanEvent::Croak { .. })));
        let requests = provider.requests();
        assert!(requests[0].iter().flat_map(|m|&m.content).any(|block|
            matches!(block,sanborn_core::ContentBlock::ToolResult{tool_use_id,..} if tool_use_id=="sanborn_contract_baseline")));
        let mut parent_tools = ToolRegistry::new();
        parent_tools.register(apply.clone());
        if case == "policy" {
            let parent_provider = Arc::new(ScriptedProvider::new([tool_response(&[(
                "apply",
                "submit_plan",
                json!({"steps":[
                step("apply","apply_tadpole_patch",json!({"proposal_id":"code-patch"}),"tool_succeeded")]}),
            )])]));
            let agent = PlannedAgent::new(
                parent_provider,
                Arc::new(LocalLaya),
                parent_tools,
                ctx.clone(),
                PlanConfig {
                    policy: Policy::read_only(),
                    ..PlanConfig::default()
                },
            );
            assert!(agent.run("Apply proposal", |_| {}).await.is_err());
            assert_eq!(
                std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
                "old\n"
            );
        } else {
            if case == "stale" {
                std::fs::write(root.path().join("src/lib.rs"), "user edit\n").unwrap();
            }
            let applied = apply.call(json!({"proposal_id":"code-patch"}), &ctx).await;
            assert_eq!(applied.is_ok(), case == "apply", "{case}: {applied:?}");
            if case == "apply" {
                assert_eq!(
                    std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
                    "new\n"
                );
            }
            if case == "stale" {
                assert_eq!(
                    std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
                    "user edit\n"
                );
            }
            assert!(apply
                .call(json!({"proposal_id":"code-patch"}), &ctx)
                .await
                .is_err());
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("outside.rs")).unwrap(),
            "old\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/.env")).unwrap(),
            "private fixture\n"
        );
    }
}
