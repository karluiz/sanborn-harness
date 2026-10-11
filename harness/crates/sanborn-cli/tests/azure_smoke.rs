use std::process::Command;

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
    let output = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args([
            "-p",
            prompt,
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
        .output()
        .unwrap();
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
}
