use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use serde_json::Value;

#[test]
#[ignore = "real Azure inference; synthetic coder fixture only"]
fn coder_proposes_then_parent_applies_with_laya() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("greeting.txt"),
        include_str!("../../../examples/coder/greeting.txt"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args([
            "-p",
            include_str!("../../../examples/coder/task.txt"),
            "--allow-write",
            "--json",
            "--json-events",
            "--max-turns",
            "6",
            "--max-inference-calls",
            "12",
            "--timeout",
            "150",
            "-w",
        ])
        .arg(root.path())
        .output()
        .unwrap();
    let trace = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "sb failed: {trace}");
    let events: Vec<Value> = trace
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events
        .iter()
        .any(|e| e["event"] == "croak" && e["id"] == "code" && e["status"] == "returned"));
    assert!(events.iter().any(|e| e["event"] == "tadpole"
        && e["id"] == "code"
        && e["detail"]["event"] == "tool_finished"
        && e["detail"]["tool"] == "propose_patch"
        && e["detail"]["is_error"] == false));
    assert!(!events
        .iter()
        .any(|e| e["event"] == "tadpole" && e["detail"]["tool"] == "write_file"));
    let apply = events
        .iter()
        .position(|e| {
            e["event"] == "tool_finished"
                && e["tool"] == "apply_tadpole_patch"
                && e["is_error"] == false
        })
        .unwrap_or_else(|| {
            panic!(
                "No successful parent application. Synthetic trace: {trace}\nResult: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    assert!(events[apply + 1..]
        .iter()
        .any(|e| e["event"] == "tool_finished"
            && e["tool"] == "read_file"
            && e["is_error"] == false));
    assert_eq!(
        std::fs::read_to_string(root.path().join("greeting.txt")).unwrap(),
        "hello new\n",
        "Synthetic coder trace: {trace}\nResult: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["usage_complete"], true);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
#[ignore = "real Azure inference; only synthetic data is sent"]
fn four_tool_plan_with_laya_and_exact_acceptance() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "alpha\n").unwrap();
    std::fs::write(root.path().join("b.txt"), "beta\n").unwrap();
    let prompt = "This is a synthetic acceptance fixture. Submit exactly one four-step plan: \
        read a.txt and read b.txt independently with tool_succeeded guards; \
        write result.txt with exactly sanborn-ready followed by one newline, \
        expected_content=null, depending on both reads; then read result.txt depending \
        on the write, guarded with text_equals to sanborn-ready followed by one newline. \
        After inspecting the plan results, report the verified result. Do not skip tools.";
    let mut child = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args([
            "-p",
            "-",
            "--json",
            "--allow-write",
            "--json-events",
            "--max-turns",
            "4",
            "--max-tool-calls",
            "8",
            "--timeout",
            "90",
            "-w",
        ])
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(prompt.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let trace = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "sb failed: {trace}");
    assert_eq!(
        std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
        "sanborn-ready\n"
    );
    let events: Vec<Value> = trace
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let finished = events
        .iter()
        .find(|event| event["event"] == "finished")
        .unwrap();
    assert_eq!(finished["tool_calls"], 4);
    assert_eq!(finished["turns"], 2);
    let decisions: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "laya_decision")
        .collect();
    assert_eq!(decisions.len(), 8);
    assert!(decisions
        .iter()
        .all(|event| event["result"]["decision"] == "continue_planned_step"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    let final_result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(final_result["schema_version"], 1);
    assert_eq!(final_result["status"], "completed");
    assert_eq!(final_result["acceptance"], "not_certified");
    assert_eq!(final_result["turns"], 2);
    assert_eq!(final_result["tool_calls"], 4);
    assert_eq!(final_result["usage"], finished["usage"]);
    assert!(final_result["final_text"]
        .as_str()
        .unwrap()
        .contains("sanborn-ready"));
}

#[test]
#[ignore = "real Azure inference; only synthetic tadpole fixtures are sent"]
fn tadpoles_with_laya_shared_usage_and_parent_verification() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("north.txt"), "north-fixture-17\n").unwrap();
    std::fs::write(root.path().join("south.txt"), "south-fixture-29\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args([
            "-p",
            "Synthetic acceptance fixture: submit a one-step plan using summon_tadpoles \
            with two workers: id=north objective=read north.txt with a text_contains guard for \
            north-fixture-17 and report its exact content; id=south objective=read south.txt \
            with a text_contains guard for south-fixture-29 and report its exact content. \
            Use a tool_succeeded guard for summoning. After receiving croaks, independently \
            verify both files with parent read_file steps, then give both exact values. \
            Do not skip summoning or child tools.",
            "--json",
            "--json-events",
            "--max-turns",
            "5",
            "--max-inference-calls",
            "12",
            "--timeout",
            "150",
            "-w",
        ])
        .arg(root.path())
        .output()
        .unwrap();
    let trace = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "sb failed: {trace}");
    let events: Vec<Value> = trace
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events.iter().filter(|e| e["event"] == "summoned").count(),
        1
    );
    let croaks: Vec<_> = events.iter().filter(|e| e["event"] == "croak").collect();
    assert_eq!(croaks.len(), 2);
    assert!(croaks.iter().all(|e| e["status"] == "returned"));
    for id in ["north", "south"] {
        assert!(events.iter().any(|e| e["event"] == "tadpole"
            && e["id"] == id
            && e["detail"]["event"] == "laya_decision"
            && e["detail"]["stage"] == "evidence"));
    }

    let finished = events.iter().find(|e| e["event"] == "finished").unwrap();
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["usage"], finished["usage"]);
    assert_eq!(result["tool_calls"], finished["tool_calls"]);
    assert!(result["tool_calls"].as_u64().unwrap() >= 5);
    assert!(result["inference_calls"].as_u64().unwrap() <= 12);
    assert_eq!(result["usage_complete"], true);
    let final_text = result["final_text"].as_str().unwrap();
    assert!(final_text.contains("north-fixture-17") && final_text.contains("south-fixture-29"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
#[ignore = "real Azure inference; only synthetic specialist fixtures are sent"]
fn specialist_contracts_execute_baselines_before_child_reasoning() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("tests")).unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("README.md"),
        include_str!("../../../examples/specialists/fixture/README.md"),
    )
    .unwrap();
    std::fs::write(
        root.path().join("tests/greeting.rs"),
        include_str!("../../../examples/specialists/fixture/tests/greeting.rs"),
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/greeting.rs"),
        include_str!("../../../examples/specialists/fixture/src/greeting.rs"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args([
            "-p",
            include_str!("../../../examples/specialists/both.txt"),
            "--json",
            "--json-events",
            "--max-turns",
            "4",
            "--max-inference-calls",
            "12",
            "--timeout",
            "150",
            "-w",
        ])
        .arg(root.path())
        .output()
        .unwrap();
    let trace = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "sb failed: {trace}");
    let events: Vec<Value> = trace
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (id, specialty, count) in [
        ("scout", "repository_scout", 1),
        ("analyst", "test_analyst", 2),
    ] {
        let child_events: Vec<_> = events
            .iter()
            .filter(|e| e["event"] == "tadpole" && e["id"] == id)
            .collect();
        assert!(child_events
            .iter()
            .any(|e| e["detail"]["event"] == "contract_started"
                && e["detail"]["specialty"] == specialty));
        let reasoning = child_events
            .iter()
            .position(|e| e["detail"]["event"] == "reasoning_started")
            .unwrap();
        assert_eq!(
            child_events[..reasoning]
                .iter()
                .filter(|e| e["detail"]["event"] == "tool_finished")
                .count(),
            count
        );
        assert_eq!(
            child_events[..reasoning]
                .iter()
                .filter(|e| e["detail"]["event"] == "laya_decision"
                    && e["detail"]["stage"] == "evidence")
                .count(),
            count
        );
        assert!(
            events
                .iter()
                .any(|e| e["event"] == "croak" && e["id"] == id && e["status"] == "returned"),
            "missing returned {id} croak; synthetic output: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(result["usage_complete"], true);
    let croaks = result["tadpole_croaks"].as_array().unwrap();
    assert_eq!(croaks.len(), 2);
    for (id, expected) in [
        (
            "scout",
            vec![
                (
                    "README.md",
                    include_str!("../../../examples/specialists/fixture/README.md"),
                ),
                (
                    "src/greeting.rs",
                    include_str!("../../../examples/specialists/fixture/src/greeting.rs"),
                ),
                (
                    "tests/greeting.rs",
                    include_str!("../../../examples/specialists/fixture/tests/greeting.rs"),
                ),
            ],
        ),
        (
            "analyst",
            vec![(
                "tests/greeting.rs",
                include_str!("../../../examples/specialists/fixture/tests/greeting.rs"),
            )],
        ),
    ] {
        let croak = croaks.iter().find(|croak| croak["id"] == id).unwrap();
        assert_eq!(croak["contract"]["citations_verified"], true, "{croak}");
        assert_eq!(
            croak["contract"]["citation_subjects_required"],
            expected.len()
        );
        assert_eq!(croak["contract"]["citation_subjects_complete"], true);
        assert_eq!(croak["findings"]["claims_certified"], false);
        let findings = croak["findings"]["findings"].as_array().unwrap();
        assert_eq!(findings.len(), expected.len());
        let ids: std::collections::HashSet<_> = findings
            .iter()
            .map(|finding| finding["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), findings.len());
        for (path, text) in expected {
            let finding = findings
                .iter()
                .find(|finding| finding["path"] == path)
                .unwrap();
            assert_eq!(finding["path"], path);
            assert_eq!(finding["kind"], "observation");
            assert_eq!(finding["start_line"], 1);
            assert_eq!(finding["end_line"], text.lines().count());
            assert_eq!(
                finding["excerpt"],
                text.lines().collect::<Vec<_>>().join("\n")
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("tests/greeting.rs")).unwrap(),
        include_str!("../../../examples/specialists/fixture/tests/greeting.rs")
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("src/greeting.rs")).unwrap(),
        include_str!("../../../examples/specialists/fixture/src/greeting.rs")
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("README.md")).unwrap(),
        include_str!("../../../examples/specialists/fixture/README.md")
    );
}
