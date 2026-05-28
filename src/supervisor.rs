use crate::message::Message;

/// Supervisor trait — observes worker behavior and can intervene.
/// Reserved for future dual-agent mode. Noop in initial release.
pub trait Supervisor: Send + Sync {
    /// Called before each turn. Return Some(msg) to inject into session.
    fn review_before_turn(
        &self,
        _history: &[Message],
        _goal: &str,
    ) -> Option<String> {
        None
    }

    /// Called after a tool call fails. Return Some(msg) to inject into session.
    fn on_tool_failure(
        &self,
        _tool: &str,
        _error: &str,
        _history: &[Message],
    ) -> Option<String> {
        None
    }
}

/// Default no-op supervisor. Never intervenes.
pub struct NoopSupervisor;

impl Supervisor for NoopSupervisor {}
