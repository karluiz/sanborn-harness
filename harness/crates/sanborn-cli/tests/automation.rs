use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn invoke(args: &[&str], stdin: &[u8], root: &std::path::Path, credential: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args(args)
        .env("XDG_CONFIG_HOME", root)
        .env("AZURE_OPENAI_API_KEY", credential)
        .env("AZURE_OPENAI_ENDPOINT", "https://example.invalid/")
        .env_remove("AZURE_OPENAI_DEPLOYMENT")
        .env_remove("AZURE_OPENAI_API_VERSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut pipe) = child.stdin.take() {
        // The CLI may stop reading as soon as the size boundary is crossed.
        match pipe.write_all(stdin) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(error) => panic!("writing fixture prompt failed: {error}"),
        }
    }
    child.wait_with_output().unwrap()
}

#[test]
fn doctor_is_offline_read_only_and_reports_all_checks() {
    let root = tempfile::tempdir().unwrap();
    let output = invoke(
        &["doctor", "--json", "-w", root.path().to_str().unwrap()],
        b"",
        root.path(),
        "synthetic-private-value",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["ready"], true);
    assert_eq!(report["network_checked"], false);
    assert_eq!(report["checks"].as_array().unwrap().len(), 5);
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-private-value"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("example.invalid"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);

    let failures = invoke(
        &[
            "doctor",
            "--json",
            "-w",
            "/path/that-does-not-exist-sanborn",
        ],
        b"",
        root.path(),
        "",
    );
    assert!(!failures.status.success());
    let report: Value = serde_json::from_slice(&failures.stdout).unwrap();
    assert_eq!(report["ready"], false);
    assert_eq!(report["checks"].as_array().unwrap().len(), 5);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "credential" && check["ready"] == false));
}

#[test]
fn doctor_exposes_broken_config_without_repairing_or_hiding_it() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("sanborn")).unwrap();
    let path = root.path().join("sanborn/config.local");
    std::fs::write(&path, "broken").unwrap();
    let output = invoke(&["doctor", "--json"], b"", root.path(), "synthetic");
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ready"], false);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "configuration" && check["ready"] == false));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
}

#[test]
fn invalid_stdin_prompts_produce_single_json_error_without_inference() {
    let root = tempfile::tempdir().unwrap();
    for (input, message) in [
        (vec![], "prompt must not be empty"),
        (b" \n".to_vec(), "prompt must not be empty"),
        (vec![0xff], "not UTF-8"),
        (vec![0], "NUL bytes"),
        (vec![b'x'; 128_001], "exceeds 128000 bytes"),
    ] {
        let output = invoke(
            &["-p", "-", "--json", "--json-events"],
            &input,
            root.path(),
            "synthetic",
        );
        assert!(!output.status.success());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], "error");
        assert_eq!(report["acceptance"], "not_certified");
        assert!(
            report["error"].as_str().unwrap().contains(message),
            "{report}"
        );
        assert!(!output.stdout.contains(&0x1b));
        assert!(!output.stderr.contains(&0x1b));
    }
}

#[test]
fn piped_prompt_is_consumed_before_workspace_validation_and_plain_errors_stay_stderr() {
    let root = tempfile::tempdir().unwrap();
    let output = invoke(
        &["-p", "-", "--json", "-w", "/missing-sanborn-fixture"],
        b"valid synthetic task\n",
        root.path(),
        "synthetic",
    );
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["error"].as_str().unwrap().contains("workspace"));
    let output = invoke(&["-p", "-", "--plain"], b"", root.path(), "synthetic");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("prompt must not be empty"));
}

#[test]
fn stalled_stdin_has_a_deadline_instead_of_hanging() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sb"))
        .args(["-p", "-", "--json", "--timeout", "1"])
        .env("XDG_CONFIG_HOME", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = child.stdin.take().unwrap();
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > std::time::Duration::from_secs(5) {
            child.kill().unwrap();
            panic!("stdin deadline did not stop the process");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(pipe);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["error"]
        .as_str()
        .unwrap()
        .contains("stdin prompt deadline exceeded"));
}
