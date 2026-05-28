mod error;
mod config;
mod message;
mod client;
mod tools;
mod env;
mod memory;
mod skill;
mod mode;
mod supervisor;
mod reflect;
mod app;
mod setup;
mod ui;
mod term;
mod tui;

use clap::Parser;

#[derive(Parser)]
#[command(name = "relay", version, about = "AI agent with local tool calling")]
struct Args {
    /// Model override
    #[arg(long)]
    model: Option<String>,

    /// Force terminal mode (no TUI)
    #[arg(long)]
    no_tui: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // First-time setup
    let _ = setup::ensure_settings().map_err(|e| {
        eprintln!("Setup error: {e}");
    });

    // Load config
    let cfg = config::load_config().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        eprintln!("Run first-time setup or set DEEPSEEK_API_KEY");
        std::process::exit(1);
    });

    // Environment and session
    let env = env::detect_environment();
    let memory = memory::MemoryStore::new();
    let skills = skill::SkillRegistry::new();

    let default_mode = match cfg.default_mode.as_str() {
        "confirm" => mode::AgentMode::Confirm,
        "plan" => mode::AgentMode::Plan,
        _ => mode::AgentMode::Auto,
    };
    let mode_state = mode::ModeState::new(default_mode);

    let system_prompt = env::build_system_prompt(&env, &mode_state, &memory, &skills);
    let mut session = message::Session::new(&cfg, &system_prompt);
    let client = client::ApiClient::new(&cfg);

    // Run UI
    #[cfg(feature = "tui")]
    if !args.no_tui {
        tui::run_tui(&cfg, &mut session, &client).await;
    } else {
        term::run_terminal(&cfg, &mut session, &client).await;
    }

    #[cfg(not(feature = "tui"))]
    term::run_terminal(&cfg, &mut session, &client).await;

    Ok(())
}
