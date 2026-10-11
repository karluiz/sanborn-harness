use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use serde_json::Value;

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
