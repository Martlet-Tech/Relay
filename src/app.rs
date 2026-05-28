use crate::client::{ApiClient, StreamEvent};
use crate::config::Config;
use crate::message::{Session, ToolCall, ToolCallFunction};
use crate::mode::{AgentMode, ModeState};
use crate::reflect::ReflectState;
use crate::supervisor::Supervisor;
use crate::tools;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum TurnEvent {
    Content(String),
    Thinking(String),
    ToolCallProposed {
        name: String,
        args: String,
        display: String,
    },
    ToolCallExecuting {
        name: String,
        args: String,
    },
    ToolCallResult {
        display: String,
        success: bool,
    },
    Stats {
        elapsed: f64,
        tokens: Option<u32>,
        ctx_pct: Option<u32>,
    },
    Warning(String),
    Error(String),
    NeedClarification {
        question: String,
    },
    PlanReady {
        plan: String,
    },
    ModeChanged {
        from: AgentMode,
        to: AgentMode,
    },
    Done,
}

pub async fn process_turn(
    cfg: &Config,
    session: &mut Session,
    client: &ApiClient,
    supervisor: &dyn Supervisor,
    mode: &mut ModeState,
    user_goal: &str,
    event_tx: mpsc::UnboundedSender<TurnEvent>,
    confirm_rx: &mut mpsc::UnboundedReceiver<bool>,
) {
    let mut reflect = ReflectState::new();
    let turn_start = std::time::Instant::now();

    for _turn in 0..cfg.max_tool_turns {
        session.ensure_context_fit();

        // Reflect: should we block or inject reflection?
        if let Some(block_msg) = reflect.should_block(cfg) {
            session.inject_system_message(&block_msg);
            let _ = event_tx.send(TurnEvent::Warning("Call blocked: repeating same tool+args".into()));
        }
        if let Some(reflect_msg) = reflect.should_reflect(cfg) {
            session.inject_system_message(&reflect_msg);
            let _ = event_tx.send(TurnEvent::Warning("Reflection requested by anti-stuck system".into()));
        }

        // Supervisor: review before turn
        if let Some(supervisor_msg) = supervisor.review_before_turn(&session.messages, user_goal) {
            session.inject_system_message(&supervisor_msg);
        }

        // Stream the response
        let tool_defs = tools::active_tool_defs(mode.current);
        let mut stream_rx = client.stream_chat_completion(
            session.messages.clone(),
            tool_defs.to_vec(),
        );

        let mut content_chunks: Vec<String> = Vec::new();
        let mut reasoning_chunks: Vec<String> = Vec::new();
        let mut partial_tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage_data: Option<(u32, u32, u32)> = None;
        let mut got_events = false;

        while let Some(event) = stream_rx.recv().await {
            got_events = true;
            match event {
                Ok(StreamEvent::Content(text)) => {
                    content_chunks.push(text.clone());
                    let _ = event_tx.send(TurnEvent::Content(text));
                }
                Ok(StreamEvent::Reasoning(text)) => {
                    reasoning_chunks.push(text.clone());
                    let _ = event_tx.send(TurnEvent::Thinking(text));
                }
                Ok(StreamEvent::ToolCall { id, name, args }) => {
                    partial_tool_calls.push(ToolCall {
                        id,
                        type_: "function".into(),
                        function: ToolCallFunction {
                            name: name.clone(),
                            arguments: args.clone(),
                        },
                    });
                }
                Ok(StreamEvent::Usage { prompt_tokens, completion_tokens, total_tokens }) => {
                    usage_data = Some((prompt_tokens, completion_tokens, total_tokens));
                }
                Ok(StreamEvent::Warning(msg)) => {
                    let _ = event_tx.send(TurnEvent::Warning(msg));
                }
                Ok(StreamEvent::Error(msg)) => {
                    session.pop_last_user_message();
                    let _ = event_tx.send(TurnEvent::Error(msg));
                    let _ = event_tx.send(TurnEvent::Done);
                    return;
                }
                Err(e) => {
                    let _ = event_tx.send(TurnEvent::Error(format!("{e}")));
                    let _ = event_tx.send(TurnEvent::Done);
                    return;
                }
            }
        }

        let elapsed = turn_start.elapsed().as_secs_f64();

        if !got_events {
            let _ = event_tx.send(TurnEvent::Error("No response from API".into()));
            let _ = event_tx.send(TurnEvent::Done);
            return;
        }

        // Stats
        let ctx_pct = if session.total_tokens() > 0 && session.max_tokens() > 0 {
            let used = session.total_tokens();
            let max = session.max_tokens();
            Some(100 - ((used as f64 / max as f64) * 100.0) as u32)
        } else {
            None
        };
        let tokens = usage_data.map(|(_, _, t)| t);
        let _ = event_tx.send(TurnEvent::Stats {
            elapsed,
            tokens,
            ctx_pct,
        });

        // No tool calls = final answer
        if partial_tool_calls.is_empty() {
            let content = content_chunks.join("");
            let reasoning = if reasoning_chunks.is_empty() { None } else { Some(reasoning_chunks.join("")) };
            session.add_assistant_message(
                Some(&content),
                reasoning.as_deref(),
                None,
            );
            let _ = event_tx.send(TurnEvent::Done);
            return;
        }

        // We have tool calls — execute them
        let content = if content_chunks.is_empty() { None } else { Some(content_chunks.join("")) };
        let reasoning = if reasoning_chunks.is_empty() { None } else { Some(reasoning_chunks.join("")) };
        session.add_assistant_message(
            content.as_deref(),
            reasoning.as_deref(),
            Some(partial_tool_calls.clone()),
        );

        for tc in &partial_tool_calls {
            let name = &tc.function.name;
            let args = &tc.function.arguments;

            // Plan mode: block write/shell
            if mode.is_tool_blocked(name) {
                let _ = event_tx.send(TurnEvent::Warning(
                    format!("{name} is not allowed in Plan mode"),
                ));
                continue;
            }

            // Confirm mode: ask user
            if mode.needs_confirmation() {
                let display = format!("{}({})", name, args.chars().take(120).collect::<String>());
                let _ = event_tx.send(TurnEvent::ToolCallProposed {
                    name: name.clone(),
                    args: args.clone(),
                    display: display.clone(),
                });

                // Wait for user confirmation
                // If no response in 100ms, continue (timeout for demo purposes)
                use tokio::time::{timeout, Duration};
                let confirmed = timeout(Duration::from_millis(100), confirm_rx.recv()).await;
                match confirmed {
                    Ok(Some(true)) => {}
                    Ok(Some(false)) | Ok(None) => {
                        let _ = event_tx.send(TurnEvent::Warning(format!("User rejected: {name}")));
                        session.inject_system_message(&format!(
                            "The user rejected the call to {name}(...). Do NOT retry the same call. Explain why or try a different approach."
                        ));
                        continue;
                    }
                    Err(_) => {
                        // Timeout — fall through to execute
                    }
                }
            }

            let _ = event_tx.send(TurnEvent::ToolCallExecuting {
                name: name.clone(),
                args: args.chars().take(120).collect(),
            });

            // Get skill registry for use_skill tool
            // Pass None since we don't have it in this context — use_skill won't work
            let result = tools::execute_tool(name, args, cfg, None);

            match result {
                Ok(output) => {
                    let truncated: String = output.chars().take(500).collect();
                    let display = if truncated.len() < output.len() {
                        format!("{}...", truncated)
                    } else {
                        truncated
                    };
                    let _ = event_tx.send(TurnEvent::ToolCallResult {
                        display,
                        success: true,
                    });
                    session.add_tool_result(&tc.id, &output);
                    reflect.record_attempt(name, args, true);
                }
                Err(e) => {
                    let display = format!("Error: {e}");
                    let _ = event_tx.send(TurnEvent::ToolCallResult {
                        display,
                        success: false,
                    });
                    session.add_tool_result(&tc.id, &format!("error: {e}"));
                    reflect.record_attempt(name, args, false);
                    supervisor.on_tool_failure(name, &e, &session.messages);
                }
            }
        }

        // Compress tool history
        if cfg.compress_tool_history {
            session.compress_old_tool_results();
        }
    }

    // Exceeded max tool turns
    let msg = format!("Reached max {} tool turns.", cfg.max_tool_turns);
    let _ = event_tx.send(TurnEvent::Warning(msg.clone()));
    session.pop_last_user_message();
    let _ = event_tx.send(TurnEvent::Done);
}

/// Owned variant — takes ownership of Session and ModeState, returns them.
/// Useful for terminal mode where we need to pass state across threads.
pub async fn process_turn_owned(
    cfg: Config,
    mut session: Session,
    mut mode: ModeState,
    client: ApiClient,
    user_goal: String,
    event_tx: mpsc::UnboundedSender<TurnEvent>,
    confirm_rx: &mut mpsc::UnboundedReceiver<bool>,
) -> (Session, ModeState) {
    let supervisor = crate::supervisor::NoopSupervisor;
    process_turn(
        &cfg, &mut session, &client, &supervisor,
        &mut mode, &user_goal, event_tx, confirm_rx,
    ).await;
    (session, mode)
}
