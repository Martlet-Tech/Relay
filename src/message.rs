use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "system")]
    System { content: String },

    #[serde(rename = "user")]
    User { content: String },

    #[serde(rename = "assistant")]
    Assistant {
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },

    #[serde(rename = "tool")]
    Tool {
        #[serde(rename = "tool_call_id")]
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

pub struct Session {
    pub messages: Vec<Message>,
    system_prompt: String,
    pub(crate) max_tokens: usize,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            system_prompt: String::new(),
            max_tokens: 128_000,
        }
    }
}

impl Session {
    pub fn new(cfg: &crate::config::Config, system_prompt: &str) -> Self {
        let max_tokens = (cfg.max_context_tokens.saturating_sub(cfg.context_safety_margin)) as usize;
        Self {
            messages: vec![Message::System {
                content: system_prompt.to_string(),
            }],
            system_prompt: system_prompt.to_string(),
            max_tokens,
        }
    }

    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message::User {
            content: content.to_string(),
        });
    }

    pub fn add_assistant_message(
        &mut self,
        content: Option<&str>,
        reasoning: Option<&str>,
        tool_calls: Option<Vec<ToolCall>>,
    ) {
        self.messages.push(Message::Assistant {
            content: content.map(|s| s.to_string()),
            reasoning_content: reasoning.map(|s| s.to_string()),
            tool_calls,
        });
    }

    pub fn add_tool_result(&mut self, tool_call_id: &str, content: &str) {
        self.messages.push(Message::Tool {
            tool_call_id: tool_call_id.to_string(),
            content: content.to_string(),
        });
    }

    pub fn inject_system_message(&mut self, content: &str) {
        self.messages.push(Message::System {
            content: content.to_string(),
        });
    }

    pub fn pop_last_user_message(&mut self) -> Option<Message> {
        let pos = self.messages.iter().rposition(|m| matches!(m, Message::User { .. }))?;
        let msg = self.messages.remove(pos);
        // Also remove any messages after it (assistant + tool calls from the interrupted turn)
        while self.messages.len() > pos {
            self.messages.pop();
        }
        Some(msg)
    }

    pub fn total_tokens(&self) -> usize {
        let mut total = 0usize;
        for msg in &self.messages {
            match msg {
                Message::System { content } | Message::User { content } => {
                    total += content.len() / 4 + 4;
                }
                Message::Assistant { content, reasoning_content, tool_calls } => {
                    if let Some(c) = content {
                        total += c.len() / 4 + 4;
                    }
                    if let Some(r) = reasoning_content {
                        total += r.len() / 4 + 4;
                    }
                    if let Some(tcs) = tool_calls {
                        for tc in tcs {
                            total += tc.function.name.len() / 4 + 4;
                            total += tc.function.arguments.len() / 4 + 4;
                        }
                    }
                }
                Message::Tool { content, .. } => {
                    total += content.len() / 4 + 4 + 10;
                }
            }
        }
        total
    }

    pub fn ensure_context_fit(&mut self) {
        if self.total_tokens() <= self.max_tokens {
            return;
        }

        // Phase 1: trim old tool messages before the last user message
        if let Some(last_user_pos) = self.messages.iter().rposition(|m| matches!(m, Message::User { .. })) {
            let mut kept: Vec<Message> = self.messages.drain(..last_user_pos).collect();
            kept.retain(|m| !matches!(m, Message::Tool { .. }));
            kept.extend(self.messages.drain(..));
            self.messages = kept;
        }

        // Phase 2: pop from front (keep at least system + 3 messages)
        while self.total_tokens() > self.max_tokens && self.messages.len() > 4 {
            self.messages.remove(1);
        }
    }

    pub fn compress_old_tool_results(&mut self) {
        // Find tool-use sequences: assistant + tool pairs
        let mut i = 0;
        while i + 1 < self.messages.len() {
            if matches!(&self.messages[i], Message::Assistant { tool_calls: Some(_), .. })
                && matches!(&self.messages[i + 1], Message::Tool { .. })
            {
                let mut seq_len = 0;
                let start = i;
                while i < self.messages.len() {
                    if matches!(&self.messages[i], Message::Assistant { tool_calls: Some(_), .. })
                        && i + 1 < self.messages.len()
                        && matches!(&self.messages[i + 1], Message::Tool { .. })
                    {
                        seq_len += 1;
                        i += 2;
                    } else if seq_len > 0 && matches!(&self.messages[i], Message::Assistant { tool_calls: None, .. }) {
                        break;
                    } else {
                        break;
                    }
                }
                // If more than 3 pairs (6 messages), keep last 2 pairs, summarize rest
                if seq_len > 3 {
                    let keep = 2; // keep last 2 pairs
                    let compress_end = start + (seq_len - keep) * 2;
                    let mut summaries: Vec<String> = Vec::new();
                    for j in (start..compress_end).step_by(2) {
                        if let Message::Assistant { tool_calls: Some(tcs), .. } = &self.messages[j] {
                            if let Some(tc) = tcs.first() {
                                summaries.push(tc.function.name.clone());
                            }
                        }
                    }
                    let summary_msg = Message::System {
                        content: format!(
                            "[summary: previously attempted {} tool calls: {}]",
                            seq_len - keep,
                            summaries.join(", ")
                        ),
                    };
                    // Remove old entries
                    self.messages.drain(start..compress_end);
                    // Insert summary
                    self.messages.insert(start, summary_msg);
                    break; // only compress one batch per call
                }
                i += 1;
            } else {
                i += 1;
            }
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.messages.push(Message::System {
            content: self.system_prompt.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> crate::config::Config {
        crate::config::Config {
            max_context_tokens: 128_000,
            context_safety_margin: 4000,
            ..crate::config::Config::default()
        }
    }

    #[test]
    fn test_add_and_count_messages() {
        let cfg = test_config();
        let mut s = Session::new(&cfg, "You are a helpful assistant.");
        s.add_user_message("hello");
        s.add_assistant_message(Some("hi"), None, None);
        assert_eq!(s.messages.len(), 3);
        assert!(s.total_tokens() > 0);
    }

    #[test]
    fn test_pop_last_user() {
        let cfg = test_config();
        let mut s = Session::new(&cfg, "system");
        s.add_user_message("first");
        s.add_assistant_message(Some("resp1"), None, None);
        s.add_user_message("second");
        s.add_assistant_message(Some("resp2"), None, None);

        let popped = s.pop_last_user_message();
        assert!(popped.is_some());
        assert_eq!(s.messages.len(), 3);
    }

    #[test]
    fn test_tool_result() {
        let cfg = test_config();
        let mut s = Session::new(&cfg, "system");
        s.add_user_message("run tool");
        s.add_tool_result("call_1", "result data");
        assert_eq!(s.messages.len(), 3);
    }

    #[test]
    fn test_inject_system() {
        let cfg = test_config();
        let mut s = Session::new(&cfg, "system");
        s.inject_system_message("reflect on your actions");
        assert_eq!(s.messages.len(), 2);
    }

    #[test]
    fn test_compress() {
        let cfg = test_config();
        let mut s = Session::new(&cfg, "system");
        s.add_user_message("do stuff");
        // Add 5 tool call sequences
        for i in 0..5 {
            s.add_assistant_message(
                None, None,
                Some(vec![ToolCall {
                    id: format!("call_{i}"),
                    type_: "function".into(),
                    function: ToolCallFunction {
                        name: format!("tool_{i}"),
                        arguments: "{}".into(),
                    },
                }]),
            );
            s.add_tool_result(&format!("call_{i}"), &format!("result_{i}"));
        }
        // Next assistant message with no tool calls (break sequence)
        s.add_assistant_message(Some("done"), None, None);

        let before = s.messages.len();
        s.compress_old_tool_results();
        assert!(s.messages.len() < before);
    }

    #[test]
    fn test_clear_keeps_system() {
        let cfg = test_config();
        let mut s = Session::new(&cfg, "system prompt");
        s.add_user_message("hello");
        s.clear();
        assert_eq!(s.messages.len(), 1);
    }
}
