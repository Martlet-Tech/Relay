#[cfg(feature = "tui")]
pub async fn run_tui(
    _cfg: &crate::config::Config,
    _session: &mut crate::message::Session,
    _client: &crate::client::ApiClient,
) {
    // TUI mode placeholder — will be implemented with ratatui
    println!("TUI mode not yet implemented. Use --no-tui for terminal mode.");
}
