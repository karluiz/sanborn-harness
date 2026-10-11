use std::collections::HashSet;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Url};
use sanborn_core::{
    CompletionRequest, CompletionResponse, ContentBlock, Provider, Role, StopReason, Usage,
};
use serde_json::{json, Value};

const MAX_RESPONSE_BYTES: usize = 2_000_000;

pub struct AzureProvider {
    client: Client,
    url: Url,
}

impl AzureProvider {
    pub fn new(endpoint: &str, deployment: &str, api_version: &str, key: &str) -> Result<Self> {
        let mut url = Url::parse(endpoint).context("invalid Azure endpoint")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            bail!("Azure endpoint must be an HTTPS origin without credentials, query or path");
        }
        if deployment.is_empty()
            || !deployment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || matches!(deployment, "." | "..")
        {
            bail!("invalid Azure deployment name");
        }
        if api_version.is_empty() || key.trim().is_empty() {
            bail!("Azure API version and credential must not be empty");
        }
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Azure endpoint cannot be a base URL"))?
            .extend(["openai", "deployments", deployment, "chat", "completions"]);
        url.query_pairs_mut()
            .append_pair("api-version", api_version);
        let mut header = HeaderValue::from_str(key).context("invalid Azure credential header")?;
        header.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("api-key", header);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .context("cannot initialize Azure HTTP client")?;
        Ok(Self { client, url })
    }
}

fn encode(request: &CompletionRequest<'_>) -> Result<Value> {
    let mut messages = Vec::new();
    if !request.system.is_empty() {
        messages.push(json!({"role": "system", "content": request.system}));
    }
    for message in request.messages {
        let mut texts = Vec::new();
        let mut calls = Vec::new();
        let mut results = Vec::new();
        for block in &message.content {
            match block {
                ContentBlock::Text { text } => texts.push(text.as_str()),
                ContentBlock::ToolUse { id, name, input } => {
                    if message.role != Role::Assistant {
                        bail!("tool call must belong to an assistant message");
                    }
                    calls.push(json!({
                        "id": id, "type": "function",
                        "function": {"name": name, "arguments": serde_json::to_string(input)?}
                    }));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    if message.role != Role::User {
                        bail!("tool result must belong to a user message");
                    }
                    results.push(json!({
                        "role": "tool", "tool_call_id": tool_use_id,
                        "content": serde_json::to_string(&json!({
                            "content": content, "is_error": is_error
                        }))?
                    }));
                }
            }
        }
        messages.extend(results);
        if !texts.is_empty() || !calls.is_empty() {
            let mut wire = json!({
                "role": if message.role == Role::Assistant { "assistant" } else { "user" },
                "content": if texts.is_empty() { Value::Null } else { json!(texts.join("\n")) }
            });
            if !calls.is_empty() {
                wire["tool_calls"] = json!(calls);
            }
            messages.push(wire);
        }
    }
    let mut body = json!({"messages": messages, "max_completion_tokens": request.max_tokens});
    if !request.tools.is_empty() {
        body["tools"] = json!(request
            .tools
            .iter()
            .map(|tool| json!({
                "type": "function",
                "function": {
                    "name": tool.name, "description": tool.description,
                    "parameters": tool.input_schema
                }
            }))
            .collect::<Vec<_>>());
        body["tool_choice"] = json!("auto");
    }
    Ok(body)
}

fn decode(body: Value) -> Result<CompletionResponse> {
    let choices = body["choices"]
        .as_array()
        .context("Azure response has no choices")?;
    if choices.len() != 1 {
        bail!("expected exactly one Azure completion choice");
    }
    let choice = &choices[0];
    let message = &choice["message"];
    if message["role"] != "assistant" {
        bail!("Azure response has no assistant message");
    }
    if message.get("refusal").is_some_and(|v| !v.is_null()) {
        bail!("Azure model refused the request");
    }
    let mut content = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) if !text.trim().is_empty() => {
            content.push(ContentBlock::Text { text: text.clone() });
        }
        None | Some(Value::Null) | Some(Value::String(_)) => {}
        _ => bail!("unsupported Azure assistant content"),
    }
    let mut ids = HashSet::new();
    if let Some(calls) = message.get("tool_calls").filter(|v| !v.is_null()) {
        for call in calls.as_array().context("invalid Azure tool calls")? {
            if call["type"] != "function" {
                bail!("unsupported Azure tool call type");
            }
            let id = call["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .context("Azure tool call has no ID")?;
            if !ids.insert(id) {
                bail!("duplicate Azure tool call ID");
            }
            let name = call["function"]["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .context("Azure tool call has no name")?;
            let input: Value = serde_json::from_str(
                call["function"]["arguments"]
                    .as_str()
                    .context("missing Azure tool arguments")?,
            )
            .context("Azure returned malformed tool argument JSON")?;
            if !input.is_object() {
                bail!("Azure tool arguments must be a JSON object");
            }
            content.push(ContentBlock::ToolUse {
                id: id.to_owned(),
                name: name.to_owned(),
                input,
            });
        }
    }
    let has_tools = content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
    let stop_reason = match choice["finish_reason"].as_str() {
        Some("stop") if !has_tools => StopReason::EndTurn,
        Some("tool_calls") if has_tools => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some("content_filter") => bail!("Azure content filter blocked the completion"),
        _ => bail!("missing, unsupported or inconsistent Azure finish reason"),
    };
    if content.is_empty() && stop_reason != StopReason::MaxTokens {
        bail!("Azure returned an empty completion");
    }
    let input_tokens = body["usage"]["prompt_tokens"]
        .as_u64()
        .context("Azure response has no prompt token usage")?;
    let output_tokens = body["usage"]["completion_tokens"]
        .as_u64()
        .context("Azure response has no completion token usage")?;
    let cache_read_tokens = match body["usage"]["prompt_tokens_details"].get("cached_tokens") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().context("invalid Azure cached token usage")?,
    };
    Ok(CompletionResponse {
        content,
        stop_reason,
        usage: Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens: 0,
        },
    })
}

#[async_trait]
impl Provider for AzureProvider {
    fn name(&self) -> &str {
        "azure"
    }

    async fn complete(&self, request: &CompletionRequest<'_>) -> Result<CompletionResponse> {
        let mut response = self
            .client
            .post(self.url.clone())
            .json(&encode(request)?)
            .send()
            .await
            .context("Azure inference request failed")?;
        if !response.status().is_success() {
            // Response bodies can echo inputs. Never include them in diagnostics.
            bail!("Azure inference returned HTTP {}", response.status());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("reading Azure response failed")?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                bail!("Azure response exceeded {MAX_RESPONSE_BYTES} bytes");
            }
            bytes.extend_from_slice(&chunk);
        }
        decode(serde_json::from_slice(&bytes).context("invalid Azure response JSON")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sanborn_core::{Message, ToolSpec};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn response(message: Value, reason: &str) -> Value {
        json!({"choices": [{"message": message, "finish_reason": reason}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 7,
                "prompt_tokens_details": {"cached_tokens": 4}}})
    }

    #[test]
    fn translates_tool_roundtrip_and_usage() {
        let messages = vec![
            Message::user_text("read"),
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call-1".into(),
                    name: "read".into(),
                    input: json!({"path": "a.txt"}),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-1".into(),
                    content: "hello".into(),
                    is_error: false,
                }],
            },
        ];
        let tools = vec![ToolSpec {
            name: "read".into(),
            description: "read".into(),
            input_schema: json!({"type": "object"}),
        }];
        let encoded = encode(&CompletionRequest {
            system: "system",
            messages: &messages,
            tools: &tools,
            max_tokens: 100,
        })
        .unwrap();
        assert_eq!(
            encoded["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a.txt\"}"
        );
        assert_eq!(encoded["messages"][3]["role"], "tool");
        assert_eq!(encoded["max_completion_tokens"], 100);
        let decoded = decode(response(
            json!({"role": "assistant", "content": null,
            "tool_calls": [{"id": "call-2", "type": "function",
                "function": {"name": "read", "arguments": "{\"path\":\"a.txt\"}"}}]}),
            "tool_calls",
        ))
        .unwrap();
        assert_eq!(decoded.stop_reason, StopReason::ToolUse);
        assert_eq!(decoded.usage.cache_read_tokens, 4);
    }

    #[test]
    fn rejects_bad_completions_and_endpoint_configuration() {
        for message in [
            json!({"role": "assistant", "content": null}),
            json!({"role": "assistant", "content": "x", "refusal": "no"}),
            json!({"role": "assistant", "tool_calls": [{
                "id": "x", "type": "function",
                "function": {"name": "read", "arguments": "not-json"}}]}),
        ] {
            assert!(decode(response(message, "stop")).is_err());
        }
        for endpoint in [
            "http://example.com",
            "https://user:pass@example.com",
            "https://example.com/path",
            "https://example.com/?x=1",
        ] {
            assert!(AzureProvider::new(endpoint, "mini", "2024-10-21", "test").is_err());
        }
    }

    #[tokio::test]
    async fn mocked_http_checks_auth_path_body_and_errors() {
        for status in [200, 401] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (end, length) = loop {
                    let mut buf = [0; 4096];
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buf[..count]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < end + length {
                    let mut buf = [0; 4096];
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buf[..count]);
                }
                let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                assert!(headers.contains("api-key: synthetic-test-key"));
                assert!(headers
                    .contains("/openai/deployments/mini/chat/completions?api-version=2024-10-21"));
                let body: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
                assert_eq!(body["messages"][0]["content"], "ping");
                let body = if status == 200 {
                    response(json!({"role": "assistant", "content": "pong"}), "stop").to_string()
                } else {
                    "synthetic-test-key must not appear in the error".into()
                };
                write!(
                    socket,
                    "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let mut provider = AzureProvider::new(
                "https://example.com",
                "mini",
                "2024-10-21",
                "synthetic-test-key",
            )
            .unwrap();
            provider.url = Url::parse(&format!(
                "http://{address}/openai/deployments/mini/chat/completions?api-version=2024-10-21"
            ))
            .unwrap();
            let messages = [Message::user_text("ping")];
            let outcome = provider
                .complete(&CompletionRequest {
                    system: "",
                    messages: &messages,
                    tools: &[],
                    max_tokens: 64,
                })
                .await;
            server.join().unwrap();
            if status == 200 {
                assert_eq!(
                    outcome.unwrap().content[0],
                    ContentBlock::Text {
                        text: "pong".into()
                    }
                );
            } else {
                let error = outcome.unwrap_err().to_string();
                assert!(error.contains("401"));
                assert!(!error.contains("synthetic-test-key"));
            }
        }
    }
}
