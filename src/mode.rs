use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Auto,
    Confirm,
    Plan,
}

pub const ALLOWED_IN_PLAN: &[&str] = &["read", "glob", "grep", "use_skill"];
pub const BLOCKED_IN_PLAN: &[&str] = &["shell", "write"];

pub struct ModeState {
    pub current: AgentMode,
    pub plan_content: Option<String>,
    pub plan_approved: bool,
    pub last_proposed_tool: Option<(String, String)>,
}

impl ModeState {
    pub fn new(initial: AgentMode) -> Self {
        Self {
            current: initial,
            plan_content: None,
            plan_approved: false,
            last_proposed_tool: None,
        }
    }

    pub fn is_tool_blocked(&self, tool: &str) -> bool {
        match self.current {
            AgentMode::Plan => BLOCKED_IN_PLAN.contains(&tool),
            _ => false,
        }
    }

    pub fn is_currently_planning(&self) -> bool {
        self.current == AgentMode::Plan && self.plan_content.is_none()
    }

    pub fn needs_confirmation(&self) -> bool {
        match self.current {
            AgentMode::Confirm => true,
            AgentMode::Plan if self.plan_content.is_some() => true,
            _ => false,
        }
    }

    pub fn switch_to(&mut self, mode: AgentMode) {
        if mode != self.current {
            self.current = mode;
            if mode != AgentMode::Plan {
                self.plan_content = None;
                self.plan_approved = false;
            }
        }
    }

    pub fn mode_description(&self) -> &'static str {
        match self.current {
            AgentMode::Auto => "You are in Auto mode. Execute tools freely to complete the task. Do not ask for confirmation.",
            AgentMode::Confirm => "You are in Confirm mode. Before each write or shell tool call, propose your intent briefly. Wait for the user to approve before executing. Plan/read/glob/grep tools can be used freely.",
            AgentMode::Plan => "You are in Plan mode. You may ONLY use read, glob, grep, and use_skill tools. Do NOT call shell or write. Your goal is to investigate the problem and produce a clear plan of action. State your plan explicitly and wait for the user to approve it before executing.",
        }
    }

    pub fn system_prompt_suffix(&self) -> String {
        self.mode_description().to_string()
    }
}
