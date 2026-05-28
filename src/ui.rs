/// Shared UI helpers — pure functions usable by both term and tui modes.

pub const ANSI_BLUE: &str = "\x1b[34m";
pub const ANSI_GREEN: &str = "\x1b[32m";
pub const ANSI_ORANGE: &str = "\x1b[33m";
pub const ANSI_RED: &str = "\x1b[31m";
pub const ANSI_YELLOW: &str = "\x1b[93m";
pub const ANSI_GRAY: &str = "\x1b[90m";
pub const ANSI_CYAN: &str = "\x1b[36m";
pub const ANSI_RESET: &str = "\x1b[0m";

pub fn stats_line(elapsed: f64, tokens: Option<u32>, ctx_pct: Option<u32>) -> String {
    let mut parts = vec![format!("{elapsed:.1}s")];
    if let Some(t) = tokens {
        parts.push(format!("{t}tok"));
    }
    if let Some(p) = ctx_pct {
        parts.push(format!("ctx:{p}%"));
    }
    format!("{}──  {}", ANSI_GRAY, parts.join("  "))
        + ANSI_RESET
}

pub fn truncate_output(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…(truncated)", &s[..max])
    } else {
        s.to_string()
    }
}
