use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn invoke(root: &Path, args: &[&str], key: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sb"))
        .args(args)
        .env("XDG_CONFIG_HOME", root)
        .env("AZURE_OPENAI_API_KEY", key)
        .env_remove("AZURE_OPENAI_ENDPOINT")
        .env_remove("AZURE_OPENAI_DEPLOYMENT")
        .env_remove("AZURE_OPENAI_API_VERSION")
        .output()
        .unwrap()
}

#[test]
fn init_is_private_idempotent_and_protects_existing_configuration() {
    let root = tempfile::tempdir().unwrap();
    let args = [
        "init",
        "--endpoint",
        "https://example.com/",
        "--deployment",
        "fixture",
    ];
    let first = invoke(root.path(), &args, "synthetic-secret-never-store");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains("network connectivity not checked"));
    assert!(!first.stderr.contains(&0x1b));
    assert!(!first.stdout.contains(&0x1b));
    let path = root.path().join("sanborn/config.local");
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-secret"));
    assert!(!String::from_utf8_lossy(&first.stdout).contains("synthetic-secret"));
    let config: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(config["laya"]["backend"], "local-evidence");
    assert_eq!(config["reasoning"]["deployment"], "fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(invoke(root.path(), &["init"], "synthetic-secret")
        .status
        .success());
    let plain = invoke(root.path(), &["init", "--plain"], "synthetic-secret");
    assert!(plain.status.success());
    assert!(!plain.stderr.contains(&0x1b));
    let conflict = invoke(
        root.path(),
        &["init", "--endpoint", "https://other.example/"],
        "synthetic-secret",
    );
    assert!(!conflict.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(invoke(
        root.path(),
        &["init", "--force", "--endpoint", "https://other.example/"],
        "synthetic-secret"
    )
    .status
    .success());
}

#[test]
fn failed_checks_do_not_save_configuration_and_bad_configuration_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    for (endpoint, key) in [
        ("http://example.com", "synthetic"),
        ("https://example.com", ""),
    ] {
        assert!(!invoke(root.path(), &["init", "--endpoint", endpoint], key)
            .status
            .success());
        assert!(!root.path().join("sanborn/config.local").exists());
    }
    std::fs::create_dir(root.path().join("sanborn")).unwrap();
    std::fs::write(root.path().join("sanborn/config.local"), "not-json").unwrap();
    let bad = invoke(root.path(), &["-p", "test"], "synthetic");
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("invalid Sanborn user configuration"));
    assert!(invoke(
        root.path(),
        &["init", "--force", "--endpoint", "https://example.com/"],
        "synthetic"
    )
    .status
    .success());
}

#[test]
fn saved_configuration_is_used_and_cli_override_takes_precedence() {
    let root = tempfile::tempdir().unwrap();
    assert!(invoke(
        root.path(),
        &["init", "--endpoint", "https://example.com/"],
        "synthetic"
    )
    .status
    .success());
    // Endpoint validation occurs before networking, so this cannot send a request.
    let bad_override = invoke(
        root.path(),
        &["-p", "test", "--endpoint", "http://example.com/"],
        "synthetic",
    );
    assert!(!bad_override.status.success());
    assert!(String::from_utf8_lossy(&bad_override.stderr).contains("HTTPS origin"));
    let invalid_laya = root.path().join("sanborn/config.local");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&invalid_laya).unwrap()).unwrap();
    config["laya"]["backend"] = serde_json::json!("disabled");
    std::fs::write(invalid_laya, config.to_string()).unwrap();
    let disabled = invoke(root.path(), &["-p", "test"], "synthetic");
    assert!(!disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("unsupported Laya backend"));
}
