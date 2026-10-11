use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::message::{ContentBlock, Message, Usage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A single model call. Borrows the conversation so the loop never clones history per turn.
#[derive(Debug, Clone, Copy)]
pub struct CompletionRequest<'a> {
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [ToolSpec],
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResponse {
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, request: &CompletionRequest<'_>) -> Result<CompletionResponse>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InferenceStats {
    pub calls: usize,
    pub completed_calls: usize,
    pub usage: Usage,
}

/// One shared budget and usage ledger for the parent and all tadpoles.
pub struct BudgetedProvider {
    inner: Arc<dyn Provider>,
    limit: usize,
    stats: Mutex<InferenceStats>,
}

impl BudgetedProvider {
    pub fn new(inner: Arc<dyn Provider>, limit: usize) -> Self {
        Self {
            inner,
            limit,
            stats: Mutex::new(InferenceStats::default()),
        }
    }

    pub fn stats(&self) -> InferenceStats {
        *self.stats.lock().expect("inference ledger poisoned")
    }
}

#[async_trait]
impl Provider for BudgetedProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn complete(&self, request: &CompletionRequest<'_>) -> Result<CompletionResponse> {
        {
            let mut stats = self.stats.lock().expect("inference ledger poisoned");
            if stats.calls >= self.limit {
                bail!("shared inference-call budget exhausted");
            }
            stats.calls += 1;
        }
        let response = self.inner.complete(request).await?;
        let mut stats = self.stats.lock().expect("inference ledger poisoned");
        stats.usage += response.usage;
        stats.completed_calls += 1;
        Ok(response)
    }
}
