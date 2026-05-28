/// Anti-stuck mechanism: failure counting, forced reflection, and loop detection.
/// Does not make extra LLM calls — injects system messages into the session.

pub struct ReflectState {
    consecutive_same_tool_failures: u32,
    total_failures: u32,
    last_tool: Option<String>,
    last_args: Option<String>,
    same_tool_same_args_count: u32,
    reset_after_turn: bool,
}

impl ReflectState {
    pub fn new() -> Self {
        Self {
            consecutive_same_tool_failures: 0,
            total_failures: 0,
            last_tool: None,
            last_args: None,
            same_tool_same_args_count: 0,
            reset_after_turn: false,
        }
    }

    /// Record a tool call attempt. Call this after execute_tool().
    pub fn record_attempt(&mut self, tool: &str, args: &str, success: bool) {
        if success {
            // Reset on success
            self.consecutive_same_tool_failures = 0;
            self.total_failures = 0;
            self.same_tool_same_args_count = 0;
            self.last_tool = None;
            self.last_args = None;
            return;
        }

        self.total_failures += 1;

        let is_same_tool = self.last_tool.as_deref() == Some(tool);
        let is_same_args = self.last_args.as_deref() == Some(args);

        if is_same_tool {
            self.consecutive_same_tool_failures += 1;
            if is_same_args {
                self.same_tool_same_args_count += 1;
            } else {
                self.same_tool_same_args_count = 0;
            }
        } else {
            self.consecutive_same_tool_failures = 1;
            self.same_tool_same_args_count = 0;
        }

        self.last_tool = Some(tool.to_string());
        self.last_args = Some(args.to_string());
    }

    /// Check if a tool call should be blocked (same tool + same args repeated).
    /// Returns true if the call should be blocked.
    pub fn should_block(&self, cfgs: &super::config::Config) -> Option<String> {
        if !cfgs.anti_stuck_enabled {
            return None;
        }
        if self.same_tool_same_args_count >= 2 {
            Some("Do NOT call the same tool with the same arguments again. Try a different approach entirely.".into())
        } else {
            None
        }
    }

    /// Check if the model should be forced to reflect before next action.
    pub fn should_reflect(&self, cfgs: &super::config::Config) -> Option<String> {
        if !cfgs.anti_stuck_enabled {
            return None;
        }
        if self.consecutive_same_tool_failures >= cfgs.reflect_after_failures {
            let last = self.last_tool.as_deref().unwrap_or("tool");
            Some(format!(
                "You've called `{last}` {} times and it failed each time. Before your next action, answer:\n\
                 1. What assumption might be wrong?\n\
                 2. What is one completely different approach?\n\
                 3. Is there a prerequisite step you missed?",
                self.consecutive_same_tool_failures
            ))
        } else if self.total_failures >= cfgs.max_failures_before_hard_stop {
            Some(
                "Stop. You're stuck. Go back to the original goal.\n\
                 Are you solving the right problem? List 3 alternative interpretations of what the user needs."
                    .into(),
            )
        } else {
            None
        }
    }

    /// Reset after a turn completes (called when response is done, no more tool calls).
    pub fn reset(&mut self) {
        self.consecutive_same_tool_failures = 0;
        self.total_failures = 0;
        self.last_tool = None;
        self.last_args = None;
        self.same_tool_same_args_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_new_state_starts_clean() {
        let s = ReflectState::new();
        assert_eq!(s.total_failures, 0);
    }

    #[test]
    fn test_success_resets() {
        let mut s = ReflectState::new();
        s.record_attempt("shell", "rm -rf /", false);
        assert_eq!(s.total_failures, 1);
        s.record_attempt("shell", "ls", true);
        assert_eq!(s.total_failures, 0);
        assert_eq!(s.consecutive_same_tool_failures, 0);
    }

    #[test]
    fn test_reflect_after_two_failures() {
        let cfg = Config {
            anti_stuck_enabled: true,
            reflect_after_failures: 2,
            max_failures_before_hard_stop: 4,
            ..Config::default()
        };
        let mut s = ReflectState::new();
        assert!(s.should_reflect(&cfg).is_none());

        s.record_attempt("shell", "cmd1", false);
        assert!(s.should_reflect(&cfg).is_none());

        s.record_attempt("shell", "cmd2", false);
        let msg = s.should_reflect(&cfg);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("shell"));
    }

    #[test]
    fn test_block_after_three_same() {
        let cfg = Config {
            anti_stuck_enabled: true,
            ..Config::default()
        };
        let mut s = ReflectState::new();
        s.record_attempt("shell", "rm -rf", false);
        assert!(s.should_block(&cfg).is_none());
        s.record_attempt("shell", "rm -rf", false);
        assert!(s.should_block(&cfg).is_none());
        s.record_attempt("shell", "rm -rf", false);
        let block = s.should_block(&cfg);
        assert!(block.is_some());
        assert!(block.unwrap().contains("Do NOT call"));
    }

    #[test]
    fn test_hard_stop() {
        let cfg = Config {
            anti_stuck_enabled: true,
            reflect_after_failures: 5,
            max_failures_before_hard_stop: 4,
            ..Config::default()
        };
        let mut s = ReflectState::new();
        for i in 0..4 {
            s.record_attempt(&format!("t{i}"), "x", false);
        }
        let msg = s.should_reflect(&cfg);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("Stop"));
    }

    #[test]
    fn test_disabled() {
        let cfg = Config {
            anti_stuck_enabled: false,
            reflect_after_failures: 1,
            ..Config::default()
        };
        let mut s = ReflectState::new();
        s.record_attempt("shell", "x", false);
        assert!(s.should_reflect(&cfg).is_none());
        assert!(s.should_block(&cfg).is_none());
    }

    #[test]
    fn test_reset() {
        let mut s = ReflectState::new();
        s.record_attempt("shell", "x", false);
        s.record_attempt("shell", "x", false);
        s.reset();
        assert_eq!(s.total_failures, 0);
        assert_eq!(s.consecutive_same_tool_failures, 0);
    }
}
