//! Laya's local decision contract. Backend transport belongs outside this crate.
//! The initial evaluator uses exact evidence guards, not learned confidence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceGuard {
    ToolSucceeded,
    TextEquals { value: String },
    TextContains { value: String },
    Discovery { shape: DiscoveryShape },
    PatchProposal,
    CitedFindings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryShape {
    FileList,
    SearchMatches,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    ContinuePlannedStep { step_id: String },
    Replan { reason: String },
    Absent { reason: String },
}

pub struct Evidence<'a> {
    pub is_error: bool,
    pub output: &'a str,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardCheck {
    Passed,
    Failed,
    Insufficient,
}

impl EvidenceGuard {
    pub fn check(&self, evidence: &Evidence<'_>) -> GuardCheck {
        if evidence.is_error {
            return GuardCheck::Failed;
        }
        if evidence.truncated && !matches!(self, Self::ToolSucceeded) {
            return GuardCheck::Insufficient;
        }
        let matches = match self {
            Self::ToolSucceeded => true,
            Self::TextEquals { value } => evidence.output == value,
            Self::TextContains { value } if !value.is_empty() => evidence.output.contains(value),
            Self::TextContains { .. } => return GuardCheck::Insufficient,
            Self::CitedFindings => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(evidence.output) else {
                    return GuardCheck::Failed;
                };
                let mut ids = std::collections::HashSet::new();
                value["version"] == 1
                    && value["citations_verified"] == true
                    && value["claims_certified"] == false
                    && value["findings"].as_array().is_some_and(|items| {
                        !items.is_empty()
                            && items.len() <= 6
                            && items.iter().all(|item| {
                                item["id"].as_str().is_some_and(|id| {
                                    !id.is_empty()
                                        && id.len() <= 48
                                        && id.bytes().all(|b| {
                                            b.is_ascii_alphanumeric() || b"-_".contains(&b)
                                        })
                                        && ids.insert(id)
                                }) && matches!(
                                    item["kind"].as_str(),
                                    Some("observation" | "inference")
                                ) && item["statement"].as_str().is_some_and(|text| {
                                    !text.trim().is_empty()
                                        && text.len() <= 512
                                        && !text.contains('\0')
                                }) && item["path"]
                                    .as_str()
                                    .is_some_and(|path| !path.is_empty() && path.len() <= 4096)
                                    && item["start_line"].as_u64().is_some_and(|start| {
                                        start > 0
                                            && item["end_line"]
                                                .as_u64()
                                                .is_some_and(|end| end >= start)
                                    })
                                    && item["excerpt"]
                                        .as_str()
                                        .is_some_and(|text| !text.is_empty() && text.len() <= 1024)
                            })
                    })
            }
            Self::PatchProposal => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(evidence.output) else {
                    return GuardCheck::Failed;
                };
                value["version"] == 1
                    && value["validated"] == true
                    && value["application"] == "parent_only"
                    && value["proposal_id"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty())
                    && value["path"].as_str().is_some_and(|path| !path.is_empty())
                    && value["content"].as_str().is_some_and(|text| {
                        text.len() <= 8_000
                            && value["replacement_bytes"].as_u64() == Some(text.len() as u64)
                    })
                    && value["expected_bytes"]
                        .as_u64()
                        .is_some_and(|bytes| bytes <= 8_000)
            }
            Self::Discovery { shape } => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(evidence.output) else {
                    return GuardCheck::Failed;
                };
                if value["limited"].as_bool() != Some(false) {
                    return GuardCheck::Insufficient;
                }
                if value["scope"].as_str().is_none() {
                    return GuardCheck::Failed;
                }
                match shape {
                    DiscoveryShape::FileList => {
                        value["entries"].as_array().is_some_and(|entries| {
                            entries.iter().all(|entry| entry.as_str().is_some())
                        })
                    }
                    DiscoveryShape::SearchMatches => {
                        if value["skipped_binary"].as_u64() != Some(0)
                            || value["skipped_oversized"].as_u64() != Some(0)
                        {
                            return GuardCheck::Insufficient;
                        }
                        value["matches"].as_array().is_some_and(|matches| {
                            matches.iter().all(|item| {
                                item["path"].as_str().is_some()
                                    && item["line"].as_u64().is_some_and(|line| line > 0)
                                    && item["text"].as_str().is_some()
                            })
                        })
                    }
                }
            }
        };
        if matches {
            GuardCheck::Passed
        } else {
            GuardCheck::Failed
        }
    }
}

pub struct DecisionRequest<'a> {
    pub next_step_id: &'a str,
    pub plan_valid: bool,
    pub dependencies_satisfied: bool,
    pub previous: Option<(Evidence<'a>, &'a EvidenceGuard)>,
}

pub trait DecisionEngine: Send + Sync {
    fn name(&self) -> &str;
    fn decide(&self, request: &DecisionRequest<'_>) -> Decision;
}

/// Deterministic bootstrap implementation. Semantic decisions require a later backend.
pub struct LocalLaya;

impl DecisionEngine for LocalLaya {
    fn name(&self) -> &str {
        "laya-local-evidence"
    }

    fn decide(&self, request: &DecisionRequest<'_>) -> Decision {
        if !request.plan_valid || request.next_step_id.is_empty() {
            return Decision::Replan {
                reason: "invalid or missing planned step".into(),
            };
        }
        if !request.dependencies_satisfied {
            return Decision::Replan {
                reason: "step dependencies are not satisfied".into(),
            };
        }
        if let Some((evidence, guard)) = &request.previous {
            match guard.check(evidence) {
                GuardCheck::Passed => {}
                GuardCheck::Failed => {
                    return Decision::Replan {
                        reason: "tool failed or result did not satisfy its evidence guard".into(),
                    }
                }
                GuardCheck::Insufficient => {
                    return Decision::Absent {
                        reason: "evidence is incomplete or the guard provides no evidence".into(),
                    }
                }
            }
        }
        Decision::ContinuePlannedStep {
            step_id: request.next_step_id.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_requires_typed_complete_evidence_not_just_success() {
        use serde_json::json;
        for (shape, value, expected) in [
            (
                DiscoveryShape::FileList,
                json!({"entries":["src/"],"limited":false,"scope":"visible"}),
                GuardCheck::Passed,
            ),
            (
                DiscoveryShape::FileList,
                json!({"entries":[],"limited":true,"scope":"visible"}),
                GuardCheck::Insufficient,
            ),
            (
                DiscoveryShape::FileList,
                json!({"entries":[1],"limited":false,"scope":"visible"}),
                GuardCheck::Failed,
            ),
            (
                DiscoveryShape::SearchMatches,
                json!({"matches":[],"limited":false,"scope":"visible","skipped_binary":0,"skipped_oversized":0}),
                GuardCheck::Passed,
            ),
            (
                DiscoveryShape::SearchMatches,
                json!({"matches":[],"limited":false,"scope":"visible","skipped_binary":1,"skipped_oversized":0}),
                GuardCheck::Insufficient,
            ),
            (
                DiscoveryShape::SearchMatches,
                json!({"matches":[{"path":"test.rs","line":0,"text":"test"}],"limited":false,"scope":"visible","skipped_binary":0,"skipped_oversized":0}),
                GuardCheck::Failed,
            ),
        ] {
            let output = value.to_string();
            let guard = EvidenceGuard::Discovery { shape };
            assert_eq!(
                guard.check(&Evidence {
                    is_error: false,
                    output: &output,
                    truncated: false
                }),
                expected
            );
            assert_eq!(
                guard.check(&Evidence {
                    is_error: false,
                    output: &output,
                    truncated: true
                }),
                GuardCheck::Insufficient
            );
        }
    }

    #[test]
    fn continues_only_with_valid_plan_dependencies_and_evidence() {
        let guard = EvidenceGuard::TextEquals {
            value: "hello".into(),
        };
        let mut request = DecisionRequest {
            next_step_id: "second",
            plan_valid: true,
            dependencies_satisfied: true,
            previous: Some((
                Evidence {
                    is_error: false,
                    output: "hello",
                    truncated: false,
                },
                &guard,
            )),
        };
        assert_eq!(
            LocalLaya.decide(&request),
            Decision::ContinuePlannedStep {
                step_id: "second".into()
            }
        );
        request.dependencies_satisfied = false;
        assert!(matches!(
            LocalLaya.decide(&request),
            Decision::Replan { .. }
        ));
        request.dependencies_satisfied = true;
        request.plan_valid = false;
        assert!(matches!(
            LocalLaya.decide(&request),
            Decision::Replan { .. }
        ));
    }

    #[test]
    fn failures_mismatches_and_insufficient_evidence_never_continue() {
        let guard = EvidenceGuard::TextContains { value: "ok".into() };
        for (is_error, output, truncated, absent) in [
            (true, "ok", false, false),
            (false, "no", false, false),
            (false, "ok", true, true),
        ] {
            let request = DecisionRequest {
                next_step_id: "next",
                plan_valid: true,
                dependencies_satisfied: true,
                previous: Some((
                    Evidence {
                        is_error,
                        output,
                        truncated,
                    },
                    &guard,
                )),
            };
            let decision = LocalLaya.decide(&request);
            if absent {
                assert!(matches!(decision, Decision::Absent { .. }));
            } else {
                assert!(matches!(decision, Decision::Replan { .. }));
            }
        }
    }
}
#[test]
fn patch_evidence_must_be_complete_and_typed() {
    let guard = EvidenceGuard::PatchProposal;
    let output = serde_json::json!({"version":1,"validated":true,"application":"parent_only",
            "proposal_id":"code-patch","path":"src/lib.rs","content":"new\n",
            "expected_bytes":4,"replacement_bytes":4})
    .to_string();
    assert_eq!(
        guard.check(&Evidence {
            is_error: false,
            output: &output,
            truncated: false
        }),
        GuardCheck::Passed
    );
    for (text, truncated, expected) in [
        (output.as_str(), true, GuardCheck::Insufficient),
        ("confident proposal", false, GuardCheck::Failed),
        ("{}", false, GuardCheck::Failed),
    ] {
        assert_eq!(
            guard.check(&Evidence {
                is_error: false,
                output: text,
                truncated
            }),
            expected
        );
    }
    assert_eq!(
        guard.check(&Evidence {
            is_error: true,
            output: &output,
            truncated: false
        }),
        GuardCheck::Failed
    );
}
