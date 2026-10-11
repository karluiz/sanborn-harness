use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Result};
use futures::future::join_all;
use serde::Serialize;
use serde_json::Value;

use crate::message::{ContentBlock, Message, Role, Usage};
use crate::provider::{CompletionRequest, Provider, StopReason};
use crate::tool::{Policy, ToolContext, ToolKind, ToolRegistry};

pub const MAX_TOOL_OUTPUT_BYTES: usize = 30_000;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub system_prompt: String,
    pub max_turns: usize,
    pub max_tokens: u32,
    pub policy: Policy,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            max_turns: 50,
            max_tokens: 8192,
            policy: Policy::full(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    TurnStarted {
        turn: usize,
    },
    AssistantText {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        id: String,
        name: String,
        is_error: bool,
        duration_ms: u128,
    },
    Finished {
        turns: usize,
        usage: Usage,
        duration_ms: u128,
    },
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub final_text: String,
    pub turns: usize,
    pub usage: Usage,
    pub stop_reason: StopReason,
    pub messages: Vec<Message>,
}

pub struct Agent {
    provider: Arc<dyn Provider>,
    tools: ToolRegistry,
    context: ToolContext,
    config: AgentConfig,
}

struct Call {
    id: String,
    name: String,
    input: Value,
}

impl Agent {
    pub fn new(
        provider: Arc<dyn Provider>,
        tools: ToolRegistry,
        context: ToolContext,
        config: AgentConfig,
    ) -> Self {
        Self {
            provider,
            tools,
            context,
            config,
        }
    }

    pub async fn run(
        &self,
        task: &str,
        mut on_event: impl FnMut(&Event) + Send,
    ) -> Result<RunOutcome> {
        let started = Instant::now();
        let specs = self.tools.specs();
        let mut messages = vec![Message::user_text(task)];
        let mut usage = Usage::default();

        for turn in 1..=self.config.max_turns {
            on_event(&Event::TurnStarted { turn });
            let request = CompletionRequest {
                system: &self.config.system_prompt,
                messages: &messages,
                tools: &specs,
                max_tokens: self.config.max_tokens,
            };
            let response = self.provider.complete(&request).await?;
            usage += response.usage;

            let mut calls = Vec::new();
            for block in &response.content {
                match block {
                    ContentBlock::Text { text } if !text.trim().is_empty() => {
                        on_event(&Event::AssistantText { text: text.clone() });
                    }
                    ContentBlock::ToolUse { id, name, input } => calls.push(Call {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }),
                    _ => {}
                }
            }

            let assistant = Message {
                role: Role::Assistant,
                content: response.content,
            };
            let final_text = assistant.text();
            messages.push(assistant);

            if calls.is_empty() {
                on_event(&Event::Finished {
                    turns: turn,
                    usage,
                    duration_ms: started.elapsed().as_millis(),
                });
                return Ok(RunOutcome {
                    final_text,
                    turns: turn,
                    usage,
                    stop_reason: response.stop_reason,
                    messages,
                });
            }

            for call in &calls {
                on_event(&Event::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                });
            }
            let results = self.execute(calls, &mut on_event).await;
            messages.push(Message {
                role: Role::User,
                content: results,
            });
        }

        bail!(
            "Stopped after reaching the maximum of {} turns",
            self.config.max_turns
        )
    }

    /// Runs tool calls in order, batching consecutive read-only calls concurrently.
    async fn execute(
        &self,
        calls: Vec<Call>,
        on_event: &mut (impl FnMut(&Event) + Send),
    ) -> Vec<ContentBlock> {
        let mut results = Vec::with_capacity(calls.len());
        let mut batch: Vec<Call> = Vec::new();

        for call in calls {
            if self.is_parallel_safe(&call.name) {
                batch.push(call);
                continue;
            }
            self.flush(&mut batch, &mut results, on_event).await;
            let (block, ms) = self.run_one(&call).await;
            emit_result(on_event, &call, &block, ms);
            results.push(block);
        }
        self.flush(&mut batch, &mut results, on_event).await;
        results
    }

    async fn flush(
        &self,
        batch: &mut Vec<Call>,
        results: &mut Vec<ContentBlock>,
        on_event: &mut (impl FnMut(&Event) + Send),
    ) {
        if batch.is_empty() {
            return;
        }
        let outputs = join_all(batch.iter().map(|call| self.run_one(call))).await;
        for (call, (block, ms)) in batch.drain(..).zip(outputs) {
            emit_result(on_event, &call, &block, ms);
            results.push(block);
        }
    }

    fn is_parallel_safe(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .is_none_or(|tool| tool.kind() == ToolKind::Read)
    }

    async fn run_one(&self, call: &Call) -> (ContentBlock, u128) {
        let started = Instant::now();
        let outcome = match self.tools.get(&call.name) {
            None => Err(format!("Error: unknown tool `{}`", call.name)),
            Some(tool) if !self.config.policy.permits(tool.kind()) => {
                let need = match tool.kind() {
                    ToolKind::Write => "write",
                    _ => "execute",
                };
                Err(format!(
                    "Permission denied: `{}` needs {need} access, which this session does not allow.",
                    call.name
                ))
            }
            Some(tool) => tool
                .call(call.input.clone(), &self.context)
                .await
                .map_err(|e| format!("Error: {e:#}")),
        };
        let (content, is_error) = match outcome {
            Ok(text) => (truncate(text, MAX_TOOL_OUTPUT_BYTES), false),
            Err(text) => (text, true),
        };
        let block = ContentBlock::ToolResult {
            tool_use_id: call.id.clone(),
            content,
            is_error,
        };
        (block, started.elapsed().as_millis())
    }
}

fn emit_result(on_event: &mut impl FnMut(&Event), call: &Call, block: &ContentBlock, ms: u128) {
    let is_error = matches!(block, ContentBlock::ToolResult { is_error: true, .. });
    on_event(&Event::ToolResult {
        id: call.id.clone(),
        name: call.name.clone(),
        is_error,
        duration_ms: ms,
    });
}

/// Truncates on a char boundary and notes how much was dropped.
pub fn truncate(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let dropped = text.len() - cut;
    text.truncate(cut);
    text.push_str(&format!("\n… [truncated {dropped} bytes]"));
    text
}
