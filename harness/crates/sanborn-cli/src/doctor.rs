use std::path::PathBuf;

use clap::Args;
use serde::Serialize;

use crate::{credential, resolve_reasoning, setup, AzureProvider, ToolContext};

#[derive(Args)]
pub struct DoctorArgs {
    #[arg(short = 'w', long, default_value = ".")]
    pub workspace: PathBuf,
    #[arg(long, env = "AZURE_OPENAI_ENDPOINT")]
    pub endpoint: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_DEPLOYMENT")]
    pub deployment: Option<String>,
    #[arg(long, env = "AZURE_OPENAI_API_VERSION")]
    pub api_version: Option<String>,
    /// Emit a single report on stdout; exit nonzero when readiness checks fail.
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub ready: bool,
    pub network_checked: bool,
    pub checks: Vec<Check>,
}

#[derive(Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ready: bool,
    pub detail: String,
}

pub async fn inspect(
    workspace: PathBuf,
    endpoint: Option<String>,
    deployment: Option<String>,
    api_version: Option<String>,
) -> Report {
    let mut checks = Vec::new();
    let workspace = ToolContext::new(workspace);
    checks.push(Check {
        name: "workspace",
        ready: workspace.is_ok(),
        detail: if workspace.is_ok() {
            "Accessible directory; path checks are not an OS sandbox.".into()
        } else {
            "Workspace is missing, inaccessible or not a directory.".into()
        },
    });
    let saved = setup::load();
    checks.push(Check {
        name: "configuration",
        ready: saved.is_ok(),
        detail: match &saved {
            Ok(Some(_)) => {
                "Saved configuration is valid; values are omitted from this report.".into()
            }
            Ok(None) => {
                "No saved configuration; environment/keyring-only setup remains supported.".into()
            }
            Err(error) => format!("{error:#}"),
        },
    });
    let reasoning = match &saved {
        Ok(config) => resolve_reasoning(endpoint, deployment, api_version, config.as_ref()).await,
        Err(_) => Err(anyhow::anyhow!(
            "repair saved configuration with sb init --force"
        )),
    };
    let key = credential().await;
    checks.push(Check {
        name: "credential",
        ready: key.is_ok(),
        detail: match &key {
            Ok(_) => "Credential available locally; authentication has not been checked.".into(),
            Err(error) => format!("{error:#}"),
        },
    });
    let provider = match reasoning {
        Ok(config) => AzureProvider::new(
            &config.endpoint,
            &config.deployment,
            &config.api_version,
            key.as_deref().unwrap_or("local-validation-only"),
        )
        .map(|_| ()),
        Err(error) => Err(error),
    };
    checks.push(Check {
        name: "inference_configuration",
        ready: provider.is_ok(),
        detail: match provider {
            Ok(()) => {
                "HTTPS origin, deployment, API version and header shape validated; no request sent."
                    .into()
            }
            Err(error) => format!("{error:#}"),
        },
    });
    let laya = setup::check_laya();
    checks.push(Check {
        name: "laya", ready: laya.is_ok(),
        detail: match laya {
            Ok(()) => "Required local-evidence engine passed Continue/Replan/Absent checks; not a learned model.".into(),
            Err(error) => format!("{error:#}"),
        },
    });
    Report {
        schema_version: 1,
        ready: checks.iter().all(|check| check.ready),
        network_checked: false,
        checks,
    }
}
