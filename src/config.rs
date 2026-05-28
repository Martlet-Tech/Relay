use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // Connection
    pub api_key: String,
    #[serde(rename = "base_url")]
    pub base_url: String,
    pub model: String,

    // Context limits
    pub max_tokens: u32,
    pub max_context_tokens: u32,
    pub context_safety_margin: u32,
    pub max_tool_turns: u32,

    // Retry
    pub retry_max_attempts: u32,
    pub retry_base_delay: f64,
    pub retry_max_delay: f64,
    pub request_timeout: f64,

    // Tool safety
    pub default_shell_timeout: f64,
    pub max_tool_output: usize,
    pub max_stderr_output: usize,

    // Run mode
    pub default_mode: String,

    // UI
    pub avatar_size: u32,

    // Anti-stuck
    pub anti_stuck_enabled: bool,
    pub reflect_after_failures: u32,
    pub max_failures_before_hard_stop: u32,
    pub compress_tool_history: bool,

    // Memory
    pub memory_enabled: bool,
    pub memory_root: String,

    // Skill
    pub skill_enabled: bool,
    pub skill_dirs: Vec<String>,

    // Supervisor (reserved)
    pub dual_agent: bool,
    pub supervisor_model: String,
    pub supervisor_max_turns: u32,

    // Other
    pub enter_sends: bool,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            max_tokens: 16384,
            max_context_tokens: 128_000,
            context_safety_margin: 4000,
            max_tool_turns: 20,
            retry_max_attempts: 3,
            retry_base_delay: 2.0,
            retry_max_delay: 60.0,
            request_timeout: 180.0,
            default_shell_timeout: 15.0,
            max_tool_output: 50_000,
            max_stderr_output: 10_000,
            default_mode: "auto".into(),
            avatar_size: 3,
            anti_stuck_enabled: true,
            reflect_after_failures: 2,
            max_failures_before_hard_stop: 4,
            compress_tool_history: true,
            memory_enabled: true,
            memory_root: "auto".into(),
            skill_enabled: true,
            skill_dirs: vec!["~/.relay/skills".into()],
            dual_agent: false,
            supervisor_model: String::new(),
            supervisor_max_turns: 3,
            enter_sends: true,
            log_level: "WARNING".into(),
        }
    }
}

pub(crate) fn expand_path(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return PathBuf::from(format!("{}{}", home.display(), rest));
        }
    }
    PathBuf::from(p)
}

fn settings_path() -> PathBuf {
    expand_path("~/.relay/settings.json")
}

fn toml_fallback_path() -> PathBuf {
    expand_path("~/.deepseek/config.toml")
}

pub fn load_config() -> Result<Config, crate::error::RelayError> {
    use crate::error::RelayError;
    use std::fs;

    let sp = settings_path();
    let mut cfg: Config = if sp.exists() {
        let data = fs::read_to_string(&sp)
            .map_err(|e| RelayError::Config(format!("read {sp:?}: {e}")))?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        let fp = toml_fallback_path();
        if fp.exists() {
            let data = fs::read_to_string(&fp)
                .map_err(|e| RelayError::Config(format!("read {fp:?}: {e}")))?;
            toml_config_fallback(&data)
        } else {
            Config::default()
        }
    };

    if let Ok(v) = std::env::var("DEEPSEEK_API_KEY") {
        cfg.api_key = v;
    }
    if let Ok(v) = std::env::var("DEEPSEEK_BASE_URL") {
        cfg.base_url = v;
    }
    if let Ok(v) = std::env::var("DEEPSEEK_MODEL") {
        cfg.model = v;
    }

    if cfg.api_key.is_empty() {
        return Err(RelayError::Config(
            "API key not configured. Set DEEPSEEK_API_KEY or run first-time setup.".into(),
        ));
    }

    Ok(cfg)
}

fn toml_config_fallback(data: &str) -> Config {
    let mut cfg = Config::default();
    for line in data.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('[') || !line.contains('=') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let k = key.trim();
            let v = val.trim().trim_matches('"');
            match k {
                "api_key" => cfg.api_key = v.into(),
                "base_url" => cfg.base_url = v.into(),
                "model" => cfg.model = v.into(),
                _ => {}
            }
        }
    }
    cfg
}

pub fn save_settings(data: &serde_json::Value) -> Result<(), crate::error::RelayError> {
    use crate::error::RelayError;
    use std::fs;
    use std::io::Write;

    let sp = settings_path();
    if let Some(parent) = sp.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| RelayError::Config(format!("create {parent:?}: {e}")))?;
    }

    let tmp = sp.with_extension("json.tmp");
    let mut f = fs::File::create(&tmp)
        .map_err(|e| RelayError::Config(format!("create tmp: {e}")))?;
    f.write_all(serde_json::to_string_pretty(data).unwrap().as_bytes())
        .map_err(|e| RelayError::Config(format!("write: {e}")))?;
    f.sync_all().map_err(|e| RelayError::Config(format!("sync: {e}")))?;
    drop(f);
    fs::rename(&tmp, &sp)
        .map_err(|e| RelayError::Config(format!("rename: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_api_key_empty() {
        let cfg = Config::default();
        assert!(cfg.api_key.is_empty());
    }

    #[test]
    fn test_toml_fallback() {
        let data = r#"api_key = "sk-test"
base_url = "https://custom.com"
model = "test-model""#;
        let cfg = toml_config_fallback(data);
        assert_eq!(cfg.api_key, "sk-test");
        assert_eq!(cfg.base_url, "https://custom.com");
        assert_eq!(cfg.model, "test-model");
    }

    #[test]
    fn test_toml_fallback_ignores_comments() {
        let data = r#"# comment
[section]
key = "value"
api_key = "sk-real""#;
        let cfg = toml_config_fallback(data);
        assert_eq!(cfg.api_key, "sk-real");
    }

    #[test]
    fn test_expand_tilde() {
        let p = expand_path("~/test.txt");
        assert!(p.to_string_lossy().contains("test.txt"));
    }
}
