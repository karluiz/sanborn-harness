//! Laya's local decision contract. Backend transport belongs outside this crate.
//! The initial evaluator uses exact evidence guards, not learned confidence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceGuard {
    ToolSucceeded,
    TextEquals { value: String },
    TextContains { value: String },
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
