use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn ensure_settings() -> Result<HashMap<String, String>, crate::error::RelayError> {
    let sp = config_path();
    if sp.exists() {
        return Ok(HashMap::new()); // already configured
    }

    println!("\n  ╔══════════════════════════════════════╗");
    println!("  ║         Relay — First-time Setup     ║");
    println!("  ╚══════════════════════════════════════╝\n");

    let mut config = HashMap::new();

    // Scan for existing configs
    let existing = detect_existing_configs();
    if !existing.is_empty() {
        println!("  Detected existing configuration:\n");
        for (source, path, label) in &existing {
            println!("    [{source}] {label}");
            println!("           {path:?}");
        }
        print!("\n  Import from existing config? [Y/n]: ");
        io::stdout().flush().ok();
        let mut ans = String::new();
        io::stdin().read_line(&mut ans).ok();
        if !ans.trim().eq_ignore_ascii_case("n") {
            let (_, path, label) = &existing[0];
            println!("  Importing from {label}...");
            let imported = import_config(path);
            for (k, v) in imported {
                println!("    {k}: {v}");
                config.insert(k, v);
            }
        }
    }

    if !config.contains_key("api_key") {
        print!("  API key (from https://platform.deepseek.com): ");
        io::stdout().flush().ok();
        let mut key = String::new();
        io::stdin().read_line(&mut key).ok();
        let key = key.trim().to_string();
        if !key.is_empty() {
            config.insert("api_key".into(), key);
        }
    }

    if !config.contains_key("base_url") {
        print!("  Base URL [https://api.deepseek.com]: ");
        io::stdout().flush().ok();
        let mut url = String::new();
        io::stdin().read_line(&mut url).ok();
        let url = url.trim();
        if !url.is_empty() {
            config.insert("base_url".into(), url.into());
        }
    }

    if !config.contains_key("model") {
        print!("  Model name [deepseek-chat]: ");
        io::stdout().flush().ok();
        let mut model = String::new();
        io::stdin().read_line(&mut model).ok();
        let model = model.trim();
        if !model.is_empty() {
            config.insert("model".into(), model.into());
        }
    }

    print!("  Enter to send mode? [Y/n]: ");
    io::stdout().flush().ok();
    let mut enter_sends = String::new();
    io::stdin().read_line(&mut enter_sends).ok();
    config.insert("enter_sends".into(), if enter_sends.trim().eq_ignore_ascii_case("n") { "false" } else { "true" }.into());

    // Save
    save_config(&config)?;
    println!("\n  ✓ Configuration saved to {:?}\n", config_path());
    Ok(config)
}

fn config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".relay").join("settings.json")
}

fn save_config(data: &HashMap<String, String>) -> Result<(), crate::error::RelayError> {
    let sp = config_path();
    if let Some(parent) = sp.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::error::RelayError::Config(format!("create dir: {e}")))?;
    }
    let value = serde_json::to_value(data).unwrap();
    crate::config::save_settings(&value)
}

pub fn detect_existing_configs() -> Vec<(String, PathBuf, String)> {
    let mut results = Vec::new();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    // ~/.deepseek/config.toml
    let ds_path = home.join(".deepseek").join("config.toml");
    if ds_path.exists() {
        results.push(("DeepSeek".into(), ds_path, "DeepSeek config".into()));
    }

    // ~/.claude/settings.json
    let claude_path = home.join(".claude").join("settings.json");
    if claude_path.exists() {
        results.push(("Claude".into(), claude_path, "Claude config".into()));
    }

    results
}

pub fn import_config(path: &Path) -> HashMap<String, String> {
    let mut config = HashMap::new();
    let content = std::fs::read_to_string(path).ok();
    match content {
        Some(data) if path.to_string_lossy().contains("config.toml") => {
            for line in data.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.starts_with('[') || !line.contains('=') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let val = v.trim().trim_matches('"').to_string();
                    match k.trim() {
                        "api_key" => { config.insert("api_key".into(), val); }
                        "base_url" => { config.insert("base_url".into(), val); }
                        "model" => { config.insert("model".into(), val); }
                        _ => {}
                    }
                }
            }
        }
        Some(data) => {
            // Try parsing as JSON (Claude settings)
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(obj) = parsed.as_object() {
                    for (src_key, target_key) in [("api_key", "api_key"), ("base_url", "base_url"), ("model", "model")] {
                        if let Some(v) = obj.get(src_key).and_then(|v| v.as_str()) {
                            config.insert(target_key.into(), v.to_string());
                        }
                    }
                }
            }
        }
        None => {}
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_toml() {
        let dir = std::env::temp_dir().join("relay_test_setup");
        std::fs::create_dir_all(&dir).ok();
        let fp = dir.join("config.toml");
        std::fs::write(&fp, r#"api_key = "sk-test"
base_url = "https://test.com"
model = "test-model""#).ok();

        let cfg = import_config(&fp);
        assert_eq!(cfg.get("api_key").unwrap(), "sk-test");
        assert_eq!(cfg.get("base_url").unwrap(), "https://test.com");
        assert_eq!(cfg.get("model").unwrap(), "test-model");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
