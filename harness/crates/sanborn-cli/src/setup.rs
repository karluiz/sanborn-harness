use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;
use sanborn_laya::{Decision, DecisionEngine, DecisionRequest, Evidence, EvidenceGuard, LocalLaya};
use serde::{Deserialize, Serialize};

pub const DEFAULT_DEPLOYMENT: &str = "gpt-5.4-mini";
pub const DEFAULT_API_VERSION: &str = "2024-10-21";

#[derive(Args)]
pub struct InitArgs {
    /// HTTPS inference origin. Existing configuration or keyring is used if omitted.
    #[arg(long, env = "AZURE_OPENAI_ENDPOINT")]
    pub endpoint: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_DEPLOYMENT")]
    pub deployment: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_API_VERSION")]
    pub api_version: Option<String>,
    /// Send a synthetic inference probe. No workspace data is sent.
    #[arg(long)]
    pub check_inference: bool,
    /// Replace an existing user configuration after all checks pass.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub reasoning: Reasoning,
    pub laya: Laya,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reasoning {
    pub endpoint: String,
    pub deployment: String,
    pub api_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Laya {
    pub backend: String,
}

impl Config {
    pub fn new(endpoint: String, deployment: String, api_version: String) -> Self {
        Self {
            version: 1,
            reasoning: Reasoning {
                endpoint,
                deployment,
                api_version,
            },
            laya: Laya {
                backend: "local-evidence".into(),
            },
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported Sanborn configuration version");
        }
        if self.laya.backend != "local-evidence" {
            bail!("unsupported Laya backend; this build supports only local-evidence");
        }
        // Shared provider validation; no request is sent and no credential is stored.
        sanborn_models::AzureProvider::new(
            &self.reasoning.endpoint,
            &self.reasoning.deployment,
            &self.reasoning.api_version,
            "configuration-validation",
        )?;
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            bail!("XDG_CONFIG_HOME must be an absolute path");
        }
        return Ok(root.join("sanborn/config.local"));
    }
    let home = std::env::var_os("HOME").context("HOME is unset; configure XDG_CONFIG_HOME")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        bail!("HOME must be an absolute path");
    }
    Ok(home.join(".config/sanborn/config.local"))
}

pub fn load() -> Result<Option<Config>> {
    let path = config_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot read Sanborn user configuration"),
    };
    let config: Config = serde_json::from_slice(&bytes)
        .context("invalid Sanborn user configuration; repair it with sb init --force")?;
    config.validate()?;
    Ok(Some(config))
}

pub fn save(config: &Config, force: bool) -> Result<PathBuf> {
    config.validate()?;
    let path = config_path()?;
    let parent = path.parent().context("configuration has no parent")?;
    std::fs::create_dir_all(parent).context("cannot create Sanborn configuration directory")?;
    let bytes = serde_json::to_vec_pretty(config)?;
    match std::fs::read(&path) {
        Ok(existing) if existing == bytes => return Ok(path),
        Ok(_) if !force => {
            bail!("Sanborn configuration already exists; use sb init --force to replace it")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("cannot inspect existing Sanborn configuration"),
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    if force {
        file.persist(&path)
            .map_err(|error| error.error)
            .context("cannot atomically replace Sanborn configuration")?;
    } else {
        file.persist_noclobber(&path)
            .map_err(|error| error.error)
            .context("cannot create Sanborn configuration without overwriting")?;
    }
    Ok(path)
}

pub fn check_laya() -> Result<()> {
    let equals = EvidenceGuard::TextEquals {
        value: "ready".into(),
    };
    for (output, truncated, is_error, expected) in [
        ("ready", false, false, "continue"),
        ("unexpected", false, false, "replan"),
        ("ready", true, false, "absent"),
        ("ready", false, true, "replan"),
    ] {
        let decision = LocalLaya.decide(&DecisionRequest {
            next_step_id: "setup-probe",
            plan_valid: true,
            dependencies_satisfied: true,
            previous: Some((
                Evidence {
                    output,
                    truncated,
                    is_error,
                },
                &equals,
            )),
        });
        let passed = match (&decision, expected) {
            (Decision::ContinuePlannedStep { step_id }, "continue") => step_id == "setup-probe",
            (Decision::Replan { .. }, "replan") | (Decision::Absent { .. }, "absent") => true,
            _ => false,
        };
        if !passed {
            bail!("Laya readiness self-check failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_local_laya_and_configuration_contract() {
        check_laya().unwrap();
        let mut config = Config::new(
            "https://example.com/".into(),
            "mini".into(),
            DEFAULT_API_VERSION.into(),
        );
        config.validate().unwrap();
        config.laya.backend = "disabled".into();
        assert!(config.validate().is_err());
        config.laya.backend = "local-evidence".into();
        config.version = 2;
        assert!(config.validate().is_err());
    }
}
