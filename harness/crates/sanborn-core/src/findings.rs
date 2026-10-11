//! Citation integrity checks, not semantic certification of model-authored claims.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolKind, ToolSpec};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    findings: Vec<Finding>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    id: String,
    kind: Kind,
    statement: String,
    path: String,
    start_line: usize,
    end_line: usize,
    excerpt: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Observation,
    Inference,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CitationSubject {
    pub id: String,
    pub kind: Kind,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequiredInput {
    findings: BTreeMap<String, Claim>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    statement: String,
}

pub(crate) struct SubmitFindings {
    pub scope: PathBuf,
    pub observed: Arc<Mutex<HashMap<PathBuf, String>>>,
    pub reader: Arc<dyn Tool>,
    pub subjects: Vec<CitationSubject>,
}

#[async_trait]
impl Tool for SubmitFindings {
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn spec(&self) -> ToolSpec {
        let mut spec = ToolSpec {
            name:"submit_findings".into(),
            description:"Return 1-6 findings after read_file in an earlier plan. Use submit_plan steps with tool=\"submit_findings\", input={\"findings\":[...]}, depends_on=[] and guard={\"kind\":\"cited_findings\"}. Each finding requires id (unique ASCII), kind (observation/inference), statement (max512 bytes), path, start_line and end_line (inclusive 1-based, counting blank lines). Omit excerpt: Rust attaches exact lines from observed and freshly checked file contents (max1024 bytes, LF without trailing LF). If supplied, excerpt must match exactly. Correct errors using prior read evidence; do not reread unchanged files. Citation validity never certifies interpretation.".into(),
            input_schema:json!({"type":"object","additionalProperties":false,"required":["findings"],
                "properties":{"findings":{"type":"array","minItems":1,"maxItems":6,
                    "items":{"type":"object","additionalProperties":false,
                        "required":["id","kind","statement","path","start_line","end_line"],
                        "properties":{"id":{"type":"string","maxLength":48},
                            "kind":{"enum":["observation","inference"]},"statement":{"type":"string","maxLength":512},
                            "path":{"type":"string","maxLength":4096},"start_line":{"type":"integer","minimum":1},
                            "end_line":{"type":"integer","minimum":1},"excerpt":{"type":["string","null"],"maxLength":1024}}}}}}),
        };
        if !self.subjects.is_empty() {
            let properties:serde_json::Map<String,Value>=self.subjects.iter().map(|subject|(
                subject.id.clone(),json!({"type":"object","additionalProperties":false,
                    "required":["statement"],"properties":{"statement":{"type":"string","maxLength":512}}})
            )).collect();
            let required: Vec<_> = self.subjects.iter().map(|subject| &subject.id).collect();
            spec.input_schema["properties"]["findings"] = json!({"type":"object",
                "additionalProperties":false,"properties":properties,"required":required});
            spec.description=format!("Submit all declared citation subjects after reading their paths in an earlier plan. \
                Use tool=submit_findings in submit_plan, with guard={{\"kind\":\"cited_findings\"}}. \
                Input findings is an object keyed by these exact subject IDs; each value contains statement only (max512 bytes). \
                Rust owns kinds, paths, ranges and exact excerpts; do not repeat those fields or add extra subjects. \
                After acceptance, return a short report. Statements remain uncertified. Subjects: {}",
                serde_json::to_string(&self.subjects).expect("citation subjects serialize"));
        }
        spec
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<String> {
        let mut findings = if self.subjects.is_empty() {
            serde_json::from_value::<Input>(input)
                .context("invalid submit_findings input")?
                .findings
        } else {
            let mut input:RequiredInput=serde_json::from_value(input)
                .context("declared subjects require findings object keyed by subject ID, each with statement only")?;
            if input.findings.len() != self.subjects.len()
                || self
                    .subjects
                    .iter()
                    .any(|subject| !input.findings.contains_key(&subject.id))
            {
                bail!("submit exactly the declared citation subject IDs; no missing or extra findings");
            }
            self.subjects
                .iter()
                .map(|subject| Finding {
                    id: subject.id.clone(),
                    kind: subject.kind.clone(),
                    statement: input
                        .findings
                        .remove(&subject.id)
                        .expect("subject checked")
                        .statement,
                    path: subject.path.clone(),
                    start_line: subject.start_line,
                    end_line: subject.end_line,
                    excerpt: None,
                })
                .collect()
        };
        if findings.is_empty() || findings.len() > 6 {
            bail!("submit 1 to 6 findings");
        }
        let mut ids = HashSet::new();
        let mut fresh = HashMap::new();
        for finding in &mut findings {
            if finding.id.is_empty()
                || finding.id.len() > 48
                || !finding
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                || !ids.insert(finding.id.clone())
            {
                bail!("finding IDs must be unique short ASCII identifiers");
            }
            if finding.statement.trim().is_empty()
                || finding.statement.len() > 512
                || finding.statement.contains('\0')
                || finding
                    .excerpt
                    .as_ref()
                    .is_some_and(|excerpt| excerpt.is_empty() || excerpt.len() > 1024)
            {
                bail!("finding text is empty or exceeds contract bounds");
            }
            if finding.path.is_empty() || finding.path.len() > 4096 {
                bail!("citation path must be nonempty and at most 4096 bytes");
            }
            let path = ctx.resolve(&finding.path)?;
            if !path.starts_with(&self.scope) {
                bail!("citation path is outside specialist scope");
            }
            let observed = self
                .observed
                .lock()
                .expect("observations poisoned")
                .get(&path)
                .cloned()
                .context("read_file evidence required for cited path")?;
            if !fresh.contains_key(&path) {
                let current = self.reader.call(json!({"path":finding.path}), ctx).await?;
                fresh.insert(path.clone(), current);
            }
            if fresh.get(&path) != Some(&observed) {
                bail!("cited file changed since read; read and replan");
            }
            let lines: Vec<_> = observed.lines().collect();
            if finding.start_line == 0
                || finding.end_line < finding.start_line
                || finding.end_line > lines.len()
            {
                bail!("citation line range is invalid for {}: file has {} lines; use inclusive 1-based range, no trailing empty line", finding.path,lines.len());
            }
            let expected = lines[finding.start_line - 1..finding.end_line].join("\n");
            if expected.is_empty() || expected.len() > 1024 {
                bail!(
                    "selected citation must contain 1 to 1024 bytes; choose a suitable line range"
                );
            }
            if finding
                .excerpt
                .as_ref()
                .is_some_and(|excerpt| expected != *excerpt)
            {
                bail!("citation excerpt does not match {} lines {}-{} (file has {} lines, including blanks). Expected excerpt as JSON: {}. Correct the range for your intended citation and reuse prior read evidence.",finding.path,finding.start_line,finding.end_line,lines.len(),serde_json::to_string(&expected)?);
            }
            finding.excerpt = Some(expected);
            finding.path = ctx.display(&path);
        }
        Ok(json!({"version":1,"citations_verified":true,"claims_certified":false,
            "findings":findings,
            "meaning":"Paths, line ranges and excerpts match read evidence at submission; statements and inferences remain model-authored."}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Reader;
    #[async_trait]
    impl Tool for Reader {
        fn kind(&self) -> ToolKind {
            ToolKind::Read
        }
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "read_file".into(),
                description: "fixture".into(),
                input_schema: json!({}),
            }
        }
        async fn call(&self, input: Value, ctx: &ToolContext) -> Result<String> {
            Ok(std::fs::read_to_string(
                ctx.resolve(input["path"].as_str().unwrap())?,
            )?)
        }
    }
    #[tokio::test]
    async fn stale_citations_and_duplicate_evidence_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.rs");
        std::fs::write(&path, "observed\n").unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        let tool = SubmitFindings {
            scope: root.path().into(),
            observed: Arc::new(Mutex::new(HashMap::from([(
                path.clone(),
                "observed\n".into(),
            )]))),
            reader: Arc::new(Reader),
            subjects: Vec::new(),
        };
        let finding = json!({"id":"source","kind":"observation","statement":"Model claim",
            "path":"source.rs","start_line":1,"end_line":1,"excerpt":"observed"});
        std::fs::write(&path, "changed\n").unwrap();
        let error = tool
            .call(json!({"findings":[finding.clone()]}), &ctx)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("changed since read"));
        std::fs::write(&path, "observed\n").unwrap();
        assert!(tool
            .call(json!({"findings":[finding.clone(),finding.clone()]}), &ctx)
            .await
            .is_err());
        let output = tool
            .call(json!({"findings":[finding]}), &ctx)
            .await
            .unwrap();
        let guard = sanborn_laya::EvidenceGuard::CitedFindings;
        assert_eq!(
            guard.check(&sanborn_laya::Evidence {
                is_error: false,
                output: &output,
                truncated: false
            }),
            sanborn_laya::GuardCheck::Passed
        );
        assert_eq!(
            guard.check(&sanborn_laya::Evidence {
                is_error: false,
                output: &output,
                truncated: true
            }),
            sanborn_laya::GuardCheck::Insufficient
        );
        let mut malformed: Value = serde_json::from_str(&output).unwrap();
        malformed["claims_certified"] = json!(true);
        assert_eq!(
            guard.check(&sanborn_laya::Evidence {
                is_error: false,
                output: &malformed.to_string(),
                truncated: false
            }),
            sanborn_laya::GuardCheck::Failed
        );
    }

    #[test]
    fn cited_findings_guard_rejects_malformed_or_failed_evidence() {
        let output = json!({"version":1,"citations_verified":true,"claims_certified":false,
            "findings":[{"id":"fact","kind":"observation","statement":"uncertified",
                "path":"source.rs","start_line":1,"end_line":1,"excerpt":"source"}]});
        let guard = sanborn_laya::EvidenceGuard::CitedFindings;
        for (field, value) in [
            ("id", json!("bad id")),
            ("kind", json!("certified")),
            ("statement", json!("")),
            ("path", json!("x".repeat(4097))),
            ("start_line", json!(0)),
            ("end_line", json!(0)),
            ("excerpt", json!("x".repeat(1025))),
        ] {
            let mut invalid = output.clone();
            invalid["findings"][0][field] = value;
            assert_eq!(
                guard.check(&sanborn_laya::Evidence {
                    is_error: false,
                    output: &invalid.to_string(),
                    truncated: false
                }),
                sanborn_laya::GuardCheck::Failed,
                "{field}"
            );
        }
        assert_eq!(
            guard.check(&sanborn_laya::Evidence {
                is_error: true,
                output: &output.to_string(),
                truncated: false
            }),
            sanborn_laya::GuardCheck::Failed
        );
    }
}
