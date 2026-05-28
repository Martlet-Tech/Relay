use crate::config::Config;
use crate::error::RelayError;
use crate::message::Message;
use crate::tools::ToolDef;
use tokio::sync::mpsc;

pub enum StreamEvent {
    Content(String),
    Reasoning(String),
    ToolCall { id: String, name: String, args: String },
    Usage { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },
    Warning(String),
    Error(String),
}

pub struct ApiClient {
    config: Config,
    pub(crate) http: reqwest::Client,
}

impl ApiClient {
    pub fn new(config: &Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs_f64(config.request_timeout))
            .build()
            .unwrap_or_default();
        Self { config: config.clone(), http }
    }

    pub fn stream_chat_completion(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDef>,
    ) -> mpsc::UnboundedReceiver<Result<StreamEvent, RelayError>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.http.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            if let Err(e) = run_stream(client, &config, messages, tools, tx.clone()).await {
                let _ = tx.send(Err(e));
            }
        });

        rx
    }
}

async fn run_stream(
    client: reqwest::Client,
    config: &Config,
    messages: Vec<Message>,
    tools: Vec<ToolDef>,
    tx: mpsc::UnboundedSender<Result<StreamEvent, RelayError>>,
) -> Result<(), RelayError> {
    let body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "tools": tools,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": config.max_tokens,
    });

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).unwrap_or_default())
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                RelayError::Network("request timed out".into())
            } else {
                RelayError::Network(format!("{e}"))
            }
        })?;

    let status = response.status();
    if status == 401 {
        return Err(RelayError::Auth("invalid API key".into()));
    }
    if status == 429 {
        return Err(RelayError::RateLimit("rate limited".into()));
    }
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(RelayError::Api {
            status: status.as_u16(),
            message: "API error".into(),
            body: Some(body_text),
        });
    }

    let mut partial_tools: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| RelayError::Network(format!("stream error: {e}")))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || !line.starts_with("data:") {
                continue;
            }

            let data = line[5..].trim();
            if data == "[DONE]" {
                return Ok(());
            }

            match parse_sse_event(data, &mut partial_tools) {
                Ok(Some(event)) => {
                    if tx.send(Ok(event)).is_err() {
                        return Ok(());
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        }
    }

    Ok(())
}

fn parse_sse_event(
    data: &str,
    partials: &mut std::collections::HashMap<String, serde_json::Value>,
) -> Result<Option<StreamEvent>, RelayError> {
    let json: serde_json::Value = serde_json::from_str(data).map_err(|e| {
        RelayError::Api {
            status: 0,
            message: format!("parse SSE: {e}"),
            body: Some(data.to_string()),
        }
    })?;

    let choices = json["choices"].as_array();
    let delta = choices
        .and_then(|c| c.first())
        .and_then(|c| c.get("delta"));

    // Process delta content
    let delta = match delta {
        Some(d) => d,
        None => return Ok(None),
    };

    // Content
    if let Some(content) = delta["content"].as_str() {
        if !content.is_empty() {
            return Ok(Some(StreamEvent::Content(content.to_string())));
        }
    }

    // Reasoning
    if let Some(reasoning) = delta["reasoning_content"].as_str() {
        if !reasoning.is_empty() {
            return Ok(Some(StreamEvent::Reasoning(reasoning.to_string())));
        }
    }

    // Tool call deltas
    if let Some(tool_calls) = delta["tool_calls"].as_array() {
        for tc in tool_calls {
            let index = tc["index"].as_u64().unwrap_or(0);
            let id = tc["id"].as_str().unwrap_or("");
            let name = tc["function"]["name"].as_str().unwrap_or("");
            let args = tc["function"]["arguments"].as_str().unwrap_or("");

            let key = format!("tool_{index}");
            let entry = partials.entry(key.clone()).or_insert_with(|| {
                serde_json::json!({"id": "", "name": "", "args": ""})
            });

            if !id.is_empty() {
                entry["id"] = serde_json::Value::String(id.to_string());
            }
            if !name.is_empty() {
                entry["name"] = serde_json::Value::String(name.to_string());
            }
            if !args.is_empty() {
                let current = entry["args"].as_str().unwrap_or("");
                entry["args"] = serde_json::Value::String(format!("{current}{args}"));
            }
        }
    }

    // Check finish_reason — must be OUTSIDE tool_calls block since the final
    // chunk may have finish_reason but no tool_calls in its delta (DeepSeek).
    if let Some(finish) = json["choices"][0]["finish_reason"].as_str() {
        if finish == "tool_calls" && !partials.is_empty() {
            // Flush all accumulated partial tools
            let results: Vec<StreamEvent> = partials.drain().map(|(_, p)| {
                let id = p["id"].as_str().unwrap_or("call_unknown").to_string();
                let name = p["name"].as_str().unwrap_or("unknown").to_string();
                let args_str = p["args"].as_str().unwrap_or("{}").to_string();
                StreamEvent::ToolCall { id, name, args: args_str }
            }).collect();
            // Return the last one — the sender handles multiple tool calls
            if let Some(tc) = results.into_iter().next() {
                return Ok(Some(tc));
            }
        }
        if finish == "length" {
            return Ok(Some(StreamEvent::Warning("response truncated (max_tokens)".into())));
        }
    }

    // Usage info — check last since API may include it in the final chunk
    if let Some(usage) = json.get("usage") {
        if usage.as_object().map_or(false, |o| o.contains_key("prompt_tokens")) {
            return Ok(Some(StreamEvent::Usage {
                prompt_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: usage["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: usage["total_tokens"].as_u64().unwrap_or(0) as u32,
            }));
        }
    }

    Ok(None)
}

/// Simple non-streaming completion for testing and plan mode.
pub async fn simple_chat_completion(
    client: &ApiClient,
    messages: &[Message],
    tools: &[ToolDef],
) -> Result<String, RelayError> {
    let body = serde_json::json!({
        "model": client.config.model,
        "messages": messages,
        "tools": tools,
        "max_tokens": client.config.max_tokens,
    });

    let url = format!("{}/chat/completions", client.config.base_url.trim_end_matches('/'));

    let resp = client.http
        .post(&url)
        .header("Authorization", format!("Bearer {}", client.config.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                RelayError::Network("request timed out".into())
            } else {
                RelayError::Network(format!("{e}"))
            }
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return match status.as_u16() {
            401 => Err(RelayError::Auth("invalid API key".into())),
            429 => Err(RelayError::RateLimit(body_text)),
            _ => Err(RelayError::Api {
                status: status.as_u16(),
                message: "API error".into(),
                body: Some(body_text),
            }),
        };
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| RelayError::Api {
            status: 0,
            message: format!("parse response: {e}"),
            body: None,
        })?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(content)
}
