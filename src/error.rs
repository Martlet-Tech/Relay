use std::fmt;

#[derive(Debug)]
pub enum RelayError {
    Config(String),
    Api { status: u16, message: String, body: Option<String> },
    RateLimit(String),
    Auth(String),
    Network(String),
    Tool(String),
    Memory(String),
    Skill(String),
}

impl fmt::Display for RelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelayError::Config(msg) => write!(f, "config: {msg}"),
            RelayError::Api { status, message, .. } => write!(f, "api error ({status}): {message}"),
            RelayError::RateLimit(msg) => write!(f, "rate limited: {msg}"),
            RelayError::Auth(msg) => write!(f, "auth failed: {msg}"),
            RelayError::Network(msg) => write!(f, "network: {msg}"),
            RelayError::Tool(msg) => write!(f, "tool: {msg}"),
            RelayError::Memory(msg) => write!(f, "memory: {msg}"),
            RelayError::Skill(msg) => write!(f, "skill: {msg}"),
        }
    }
}

impl std::error::Error for RelayError {}
