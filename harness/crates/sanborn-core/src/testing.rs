//! Deterministic provider for tests and offline development.

use std::collections::VecDeque;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::message::{ContentBlock, Message, Usage};
use crate::provider::{CompletionRequest, CompletionResponse, Provider, StopReason};

#[derive(Default)]
pub struct ScriptedProvider {
    responses: Mutex<VecDeque<CompletionResponse>>,
    requests: Mutex<Vec<Vec<Message>>>,
}

impl ScriptedProvider {
    pub fn new(responses: impl IntoIterator<Item = CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Conversation snapshots received on each call.
    pub fn requests(&self) -> Vec<Vec<Message>> {
        self.requests
            .lock()
            .expect("requests lock poisoned")
            .clone()
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }

    async fn complete(&self, request: &CompletionRequest<'_>) -> Result<CompletionResponse> {
        self.requests
            .lock()
            .expect("requests lock poisoned")
            .push(request.messages.to_vec());
        self.responses
            .lock()
            .expect("responses lock poisoned")
            .pop_front()
            .ok_or_else(|| anyhow!("ScriptedProvider ran out of responses"))
    }
}

pub fn text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::EndTurn,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
    }
}

/// A response that calls each `(id, tool, input)` in order.
pub fn tool_response(calls: &[(&str, &str, Value)]) -> CompletionResponse {
    CompletionResponse {
        content: calls
            .iter()
            .map(|(id, name, input)| ContentBlock::ToolUse {
                id: (*id).to_string(),
                name: (*name).to_string(),
                input: input.clone(),
            })
            .collect(),
        stop_reason: StopReason::ToolUse,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
    }
}
