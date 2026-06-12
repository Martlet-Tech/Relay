use crate::app::{process_turn_owned, TurnEvent};
use crate::client::ApiClient;
use crate::config::Config;
use crate::env;
use crate::message::Session;
use crate::mode::{AgentMode, ModeState};
use crate::ui;
use std::io::{self, Write};
use tokio::sync::mpsc;

pub async fn run_terminal(cfg: &Config, session: &mut Session, client: &ApiClient) {
    let env = env::detect_environment();
    print_banner(cfg, &env);

    let mut mode = ModeState::new(
        match cfg.default_mode.as_str() {
            "confirm" => AgentMode::Confirm,
            "plan" => AgentMode::Plan,
            _ => AgentMode::Auto,
        }
    );

    loop {
        print!("\n  │ > ");
        io::stdout().flush().ok();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            break;
        }
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('/') {
            if !handle_command(&line, session, &mut mode).await {
                break;
            }
            continue;
        }

        println!("  ──  {}", line);
        session.add_user_message(&line);

        // Swap session and mode into owned values for the processing task
        let local_cfg = cfg.clone();
        let local_client = crate::client::ApiClient::new(&local_cfg);
        let local_session = std::mem::take(session);
        let local_mode = std::mem::replace(&mut mode, ModeState::new(AgentMode::Auto));
        let goal = line.clone();

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (confirm_tx, mut confirm_rx) = mpsc::unbounded_channel();

        // Spawn processing — takes ownership of session and mode
        let proc_handle = tokio::spawn(async move {
            process_turn_owned(
                local_cfg,
                local_session,
                local_mode,
                local_client,
                goal,
                event_tx,
                &mut confirm_rx,
            ).await
        });

        // Process events in the current task
        while let Some(event) = event_rx.recv().await {
            match event {
                TurnEvent::Content(text) => { print!("{}", text); io::stdout().flush().ok(); }
                TurnEvent::Thinking(_text) => { /* reasoning_content — not displayed per-token */ }
                TurnEvent::ToolCallExecuting { name, args } => {
                    println!("\n{}◆ {}({}){}", ui::ANSI_ORANGE, name, args, ui::ANSI_RESET);
                }
                TurnEvent::ToolCallResult { display, success } => {
                    for line in display.split('\n') {
                        if success {
                            println!("{}{}{}", ui::ANSI_GRAY, line, ui::ANSI_RESET);
                        } else {
                            println!("{}✗ {}{}", ui::ANSI_RED, line, ui::ANSI_RESET);
                        }
                    }
                }
                TurnEvent::ToolCallProposed { name, args, display: _ } => {
                    print!("{}({}) — execute? [y/n]: ", name, args);
                    io::stdout().flush().ok();
                    let mut ans = String::new();
                    io::stdin().read_line(&mut ans).ok();
                    let _ = confirm_tx.send(!ans.trim().eq_ignore_ascii_case("n"));
                }
                TurnEvent::Stats { elapsed, tokens, ctx_pct } => {
                    println!("\n{}", ui::stats_line(elapsed, tokens, ctx_pct));
                }
                TurnEvent::Warning(msg) => println!("{}⚠ {}{}", ui::ANSI_YELLOW, msg, ui::ANSI_RESET),
                TurnEvent::Error(msg) => println!("{}✗ {}{}", ui::ANSI_RED, msg, ui::ANSI_RESET),
                TurnEvent::NeedClarification { question } => {
                    println!("{}? {}{}", ui::ANSI_YELLOW, question, ui::ANSI_RESET);
                }
                TurnEvent::PlanReady { plan } => {
                    println!("\n{}◇ Plan ──{}{}", ui::ANSI_CYAN, ui::ANSI_RESET, plan);
                }
                TurnEvent::ModeChanged { from, to } => {
                    println!("  mode: {:?} → {:?}", from, to);
                }
                TurnEvent::Done => break,
            }
        }
        io::stdout().flush().ok();

        // Retrieve owned values back
        if let Ok(result) = proc_handle.await {
            *session = result.0;
            mode = result.1;
        }
    }
}

async fn handle_command(line: &str, session: &mut Session, mode: &mut ModeState) -> bool {
    let parts: Vec<&str> = line[1..].split_whitespace().collect();
    if parts.is_empty() { return true; }

    match parts[0].to_lowercase().as_str() {
        "exit" | "quit" => return false,
        "clear" => { session.clear(); println!("  --- cleared"); }
        "model" if parts.len() > 1 => { println!("  --- switched to {}", parts[1]); }
        "tools" => {
            for t in crate::tools::FULL_TOOL_DEFS.iter() {
                println!("  {}: {}", t.function.name, t.function.description);
            }
        }
        "tokens" => println!("  ~{} tokens", session.total_tokens()),
        "memory" => println!("  (memory not available in terminal mode yet)"),
        "skill" => println!("  (skills not available in terminal mode yet)"),
        "mode" if parts.len() > 1 => {
            let new_mode = match parts[1] {
                "confirm" => AgentMode::Confirm, "plan" => AgentMode::Plan, _ => AgentMode::Auto
            };
            let old = mode.current;
            mode.switch_to(new_mode);
            session.inject_system_message(&mode.system_prompt_suffix());
            println!("  mode: {:?} → {:?}", old, new_mode);
        }
        "auto" | "confirm" | "plan" => {
            let new_mode = match parts[0] { "confirm" => AgentMode::Confirm, "plan" => AgentMode::Plan, _ => AgentMode::Auto };
            let old = mode.current;
            mode.switch_to(new_mode);
            session.inject_system_message(&mode.system_prompt_suffix());
            println!("  mode: {:?} → {}", old, parts[0]);
        }
        _ => println!("  unknown command: /{}", parts[0]),
    }
    true
}

fn print_banner(cfg: &Config, env: &crate::env::EnvInfo) {
    let avail: Vec<&str> = env.tools.iter()
        .filter(|(_, &v)| v).map(|(k, _)| k.as_str()).collect();
    println!();
    println!("  ┌──────────────────────────────────┐");
    println!("  │             relay                │");
    println!("  ├──────────────────────────────────┤");
    println!("  │  model: {}", cfg.model);
    println!("  │  os:    {} ({})", env.os, env.os_version);
    println!("  │  shell: {}", env.default_shell);
    println!("  │  cwd:   {}", env.cwd);
    if !avail.is_empty() { println!("  │  tools: {}", avail.join(", ")); }
    println!("  ├──────────────────────────────────┤");
    println!("  │  /exit /clear /model /tools      │");
    println!("  │  /tokens /auto /confirm /plan     │");
    println!("  └──────────────────────────────────┘");
    println!();
}
