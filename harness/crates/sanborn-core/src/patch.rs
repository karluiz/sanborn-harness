//! Run-local replacement proposals. Children never receive the writer.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolKind, ToolSpec};

const MAX_PATCH_BYTES: usize = 8_000;

#[derive(Default)]
pub(crate) struct Proposals(pub Mutex<HashMap<String, Proposal>>);

pub(crate) struct Proposal {
    pub path: PathBuf,
    pub workspace: PathBuf,
    pub expected: String,
    pub content: String,
    pub ready: bool,
}

pub(crate) struct ProposePatch {
    pub id: String,
    pub scope: PathBuf,
    pub observed: Arc<Mutex<HashMap<PathBuf, String>>>,
    pub proposals: Arc<Proposals>,
    pub reader: Arc<dyn Tool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchInput {
    path: String,
    expected_content: String,
    content: String,
}

#[async_trait]
impl Tool for ProposePatch {
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "propose_patch".into(),
            description: "Submit one existing-file replacement after read_file. No disk writes. Both old and new contents must be at most 8000 bytes; old content must match the observed file exactly. Use patch_proposal guard. Parent applies only by proposal ID after Laya evidence acceptance.".into(),
            input_schema: json!({"type":"object","additionalProperties":false,
                "required":["path","expected_content","content"],"properties":{
                    "path":{"type":"string"},"expected_content":{"type":"string"},
                    "content":{"type":"string"}}}),
        }
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<String> {
        let input: PatchInput = serde_json::from_value(input).context("invalid patch proposal")?;
        if input.content.len() > MAX_PATCH_BYTES || input.expected_content.len() > MAX_PATCH_BYTES {
            bail!("patch old/new contents exceed 8000-byte contract bound");
        }
        if input.content == input.expected_content {
            bail!("patch must change observed contents");
        }
        let path = ctx.resolve(&input.path)?;
        if !path.starts_with(&self.scope) {
            bail!("patch path is outside coder scope");
        }
        let observed = self
            .observed
            .lock()
            .expect("coder observations poisoned")
            .get(&path)
            .cloned()
            .context("coder must read_file before proposing this file")?;
        if observed != input.expected_content {
            bail!("patch expected_content differs from coder read evidence");
        }
        let current = self.reader.call(json!({"path":input.path}), ctx).await?;
        if current != observed {
            bail!("file changed since coder read; read and replan");
        }
        let id = format!("{}-patch", self.id);
        let mut proposals = self.proposals.0.lock().expect("proposals poisoned");
        if proposals.contains_key(&id) {
            bail!("coder contract allows one proposal per worker");
        }
        let output = json!({"version":1,"proposal_id":id,"validated":true,
            "path":ctx.display(&path),"content":input.content,"expected_bytes":observed.len(),
            "replacement_bytes":input.content.len(),"application":"parent_only",
            "meaning":"Structurally validated replacement, not semantic correctness or passing tests."}).to_string();
        proposals.insert(
            id,
            Proposal {
                path,
                workspace: ctx.workspace().into(),
                expected: observed,
                content: input.content,
                ready: false,
            },
        );
        Ok(output)
    }
}

pub struct ApplyPatch {
    pub(crate) proposals: Arc<Proposals>,
    pub(crate) writer: Arc<dyn Tool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyInput {
    proposal_id: String,
}

#[async_trait]
impl Tool for ApplyPatch {
    fn kind(&self) -> ToolKind {
        ToolKind::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:"apply_tadpole_patch".into(),
            description:"Parent-only guarded write of a coder proposal by ID. Requires write permission and Laya-accepted coder contract. Exact observed contents must still match. One application attempt per proposal; inspect errors and verify by read_file afterward. No execution or test certification.".into(),
            input_schema:json!({"type":"object","additionalProperties":false,
                "required":["proposal_id"],"properties":{"proposal_id":{"type":"string"}}}),
        }
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<String> {
        let input: ApplyInput =
            serde_json::from_value(input).context("invalid apply_tadpole_patch input")?;
        let proposal = {
            let mut proposals = self.proposals.0.lock().expect("proposals poisoned");
            let proposal = proposals
                .get(&input.proposal_id)
                .context("unknown proposal ID")?;
            if !proposal.ready {
                bail!("proposal has not passed its Laya-gated coder contract");
            }
            if proposal.workspace != ctx.workspace() {
                bail!("proposal belongs to another workspace");
            }
            proposals
                .remove(&input.proposal_id)
                .context("proposal disappeared")?
        };
        if ctx.resolve(&ctx.display(&proposal.path))? != proposal.path {
            bail!("proposal target resolution changed; summon a fresh coder");
        }
        self.writer
            .call(
                json!({"path":ctx.display(&proposal.path),
            "expected_content":proposal.expected,"content":proposal.content}),
                ctx,
            )
            .await
    }
}
