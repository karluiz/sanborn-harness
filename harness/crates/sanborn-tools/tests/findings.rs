use sanborn_core::tadpole::Tadpoles;
use sanborn_core::testing::{text_response, tool_response, ScriptedProvider};
use sanborn_core::{Tool, ToolContext, ToolKind, ToolRegistry};
use sanborn_laya::{Decision, DecisionEngine, DecisionRequest, LocalLaya};
use serde_json::{json, Value};
use std::sync::Arc;

struct DenyFindings;
impl DecisionEngine for DenyFindings {
    fn name(&self) -> &str {
        "deny-findings"
    }
    fn decide(&self, request: &DecisionRequest<'_>) -> Decision {
        if request
            .previous
            .as_ref()
            .is_some_and(|(_, guard)| matches!(guard, sanborn_laya::EvidenceGuard::CitedFindings))
        {
            Decision::Replan {
                reason: "fixture rejection".into(),
            }
        } else {
            LocalLaya.decide(request)
        }
    }
}

#[tokio::test]
async fn specialist_findings_validate_citations_not_claim_truth() {
    for case in [
        "valid",
        "inference",
        "no-read",
        "bad-excerpt",
        "zero-line",
        "reversed",
        "past-end",
        "outside",
        "protected",
        "duplicate",
        "oversized",
        "wrong-guard",
        "laya-denied",
        "prose-only",
        "crlf",
        "rust-excerpt",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("src/lib.rs"),
            if case == "crlf" {
                "first\r\nsecond\r\n"
            } else {
                "first\nsecond\n"
            },
        )
        .unwrap();
        std::fs::write(root.path().join("outside.rs"), "first\nsecond\n").unwrap();
        std::fs::write(root.path().join("src/.env"), "first\nsecond\n").unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        let all = sanborn_tools::workspace_tools();
        let mut reads = ToolRegistry::new();
        for tool in all.iter().filter(|tool| tool.kind() == ToolKind::Read) {
            reads.register(tool.clone());
        }
        let path = match case {
            "outside" => "outside.rs",
            "protected" => "src/.env",
            _ => "src/lib.rs",
        };
        let mut responses = Vec::new();
        if case != "no-read" && case != "prose-only" {
            responses.push(tool_response(&[(
                "read",
                "submit_plan",
                json!({"steps":[{
                "id":"read","tool":"read_file","input":{"path":path},"depends_on":[],
                "guard":{"kind":"tool_succeeded"}}]}),
            )]));
        }
        if case != "prose-only" {
            let mut finding = json!({"id":"fact","kind":if case=="inference" {"inference"} else {"observation"},
                "statement":"Model-authored claim; citation does not prove this sentence.",
                "path":path,"start_line":if case=="zero-line" {0} else if case=="reversed" {2} else {1},
                "end_line":if case=="past-end" {3} else {1},
                "excerpt":if case=="bad-excerpt" {"invented".into()} else if case=="oversized" {"x".repeat(1025)} else {"first".into()}});
            if case == "rust-excerpt" {
                finding.as_object_mut().unwrap().remove("excerpt");
            }
            let findings = if case == "duplicate" {
                vec![finding.clone(), finding]
            } else {
                vec![finding]
            };
            responses.push(tool_response(&[("findings","submit_plan",json!({"steps":[{
                "id":"findings","tool":"submit_findings","input":{"findings":findings},"depends_on":[],
                "guard":{"kind":if case=="wrong-guard" {"tool_succeeded"} else {"cited_findings"}}}]}))]));
        }
        responses.push(text_response("Confident claims without certification."));
        let provider = Arc::new(ScriptedProvider::new(responses));
        let laya: Arc<dyn DecisionEngine> = if case == "laya-denied" {
            Arc::new(DenyFindings)
        } else {
            Arc::new(LocalLaya)
        };
        let tadpoles = Tadpoles::new(provider, laya, reads, 1).unwrap();
        let output = tadpoles
            .call(
                json!({"tadpoles":[{"id":"scout","specialty":"repository_scout",
            "scope":"src","objective":"Cite first line"}]}),
                &ctx,
            )
            .await
            .unwrap();
        let result: Value = serde_json::from_str(&output).unwrap();
        let valid = matches!(case, "valid" | "inference" | "crlf" | "rust-excerpt");
        let croak = &result["croaks"][0];
        assert_eq!(
            croak["contract"]["citations_verified"], valid,
            "{case}: {output}"
        );
        if valid {
            assert_eq!(croak["findings"]["findings"][0]["excerpt"], "first");
            assert_eq!(croak["findings"]["claims_certified"], false);
            assert_eq!(
                croak["finding_status"],
                "citations_verified_claims_not_certified"
            );
        } else {
            assert!(croak["findings"].is_null(), "{case}");
            assert_eq!(croak["finding_status"], "unverified");
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("outside.rs")).unwrap(),
            "first\nsecond\n"
        );
    }
}

#[tokio::test]
async fn declared_subjects_require_complete_statements_and_immutable_citations() {
    for case in [
        "valid",
        "missing",
        "extra",
        "override",
        "array",
        "no-read",
        "prose-only",
        "laya-denied",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "first\nsecond\n").unwrap();
        let mut responses = Vec::new();
        if case != "no-read" {
            responses.push(tool_response(&[(
                "read",
                "submit_plan",
                json!({"steps":[{
                "id":"read","tool":"read_file","input":{"path":"src/lib.rs"},
                "depends_on":[],"guard":{"kind":"tool_succeeded"}}]}),
            )]));
        }
        if case != "prose-only" {
            let findings = match case {
                "missing" => json!({}),
                "extra" => {
                    json!({"fact":{"statement":"uncertified"},"extra":{"statement":"extra"}})
                }
                "override" => json!({"fact":{"statement":"uncertified","path":"outside.rs"}}),
                "array" => json!([{"id":"fact","statement":"uncertified"}]),
                _ => json!({"fact":{"statement":"uncertified model interpretation"}}),
            };
            responses.push(tool_response(&[(
                "cite",
                "submit_plan",
                json!({"steps":[{
                "id":"cite","tool":"submit_findings","input":{"findings":findings},
                "depends_on":[],"guard":{"kind":"cited_findings"}}]}),
            )]));
        }
        responses.push(text_response("Model report is not certification."));
        let provider = Arc::new(ScriptedProvider::new(responses));
        let laya: Arc<dyn DecisionEngine> = if case == "laya-denied" {
            Arc::new(DenyFindings)
        } else {
            Arc::new(LocalLaya)
        };
        let mut reads = ToolRegistry::new();
        for tool in sanborn_tools::workspace_tools()
            .iter()
            .filter(|tool| tool.kind() == ToolKind::Read)
        {
            reads.register(tool.clone());
        }
        let tadpoles = Tadpoles::new(provider, laya, reads, 1).unwrap();
        let output=tadpoles.call(json!({"tadpoles":[{"id":"scout","specialty":"repository_scout",
            "scope":"src","objective":"Interpret the declared source",
            "citation_subjects":[{"id":"fact","kind":"observation","path":"src/lib.rs","start_line":1,"end_line":2}]}]}),
            &ToolContext::new(root.path()).unwrap()).await.unwrap();
        let result: Value = serde_json::from_str(&output).unwrap();
        let croak = &result["croaks"][0];
        let valid = case == "valid";
        assert_eq!(
            croak["status"],
            if valid { "returned" } else { "blocked" },
            "{case}: {output}"
        );
        assert_eq!(croak["contract"]["citation_subjects_required"], 1);
        assert_eq!(croak["contract"]["citation_subjects_complete"], valid);
        assert_eq!(croak["contract"]["citations_verified"], valid);
        if valid {
            let findings = croak["findings"]["findings"].as_array().unwrap();
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0]["id"], "fact");
            assert_eq!(findings[0]["path"], "src/lib.rs");
            assert_eq!(findings[0]["start_line"], 1);
            assert_eq!(findings[0]["end_line"], 2);
            assert_eq!(findings[0]["kind"], "observation");
            assert_eq!(findings[0]["excerpt"], "first\nsecond");
            assert_eq!(croak["findings"]["claims_certified"], false);
        } else {
            assert!(croak["findings"].is_null());
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
            "first\nsecond\n"
        );
    }
}

#[tokio::test]
async fn invalid_subject_contracts_are_rejected_before_dispatch() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "source\n").unwrap();
    std::fs::write(root.path().join("outside.rs"), "outside\n").unwrap();
    let subject =
        json!({"id":"fact","kind":"observation","path":"src/lib.rs","start_line":1,"end_line":1});
    for case in [
        "general",
        "coder",
        "duplicate",
        "outside",
        "zero-line",
        "reversed",
        "oversized-id",
    ] {
        let mut modified = subject.clone();
        match case {
            "outside" => modified["path"] = json!("outside.rs"),
            "zero-line" => modified["start_line"] = json!(0),
            "reversed" => modified["start_line"] = json!(2),
            "oversized-id" => modified["id"] = json!("x".repeat(49)),
            _ => {}
        }
        let subjects = if case == "duplicate" {
            vec![modified.clone(), modified]
        } else {
            vec![modified]
        };
        let provider = Arc::new(ScriptedProvider::new([]));
        let mut reads = ToolRegistry::new();
        for tool in sanborn_tools::workspace_tools()
            .iter()
            .filter(|tool| tool.kind() == ToolKind::Read)
        {
            reads.register(tool.clone());
        }
        let tadpoles = Tadpoles::new(provider.clone(), Arc::new(LocalLaya), reads, 1).unwrap();
        let error = tadpoles
            .call(
                json!({"tadpoles":[{"id":"scout",
            "specialty":if matches!(case,"general"|"coder") {case} else {"repository_scout"},
            "scope":"src","objective":"invalid subject fixture","citation_subjects":subjects}]}),
                &ToolContext::new(root.path()).unwrap(),
            )
            .await;
        assert!(error.is_err(), "{case}");
        assert!(provider.requests().is_empty(), "{case}");
    }
}
