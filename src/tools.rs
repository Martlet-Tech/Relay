use serde::{Deserialize, Serialize};
use serde_json::Map;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolFn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFn {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

fn tool_def(name: &str, desc: &str, params: serde_json::Value) -> ToolDef {
    ToolDef {
        type_: "function".into(),
        function: ToolFn {
            name: name.into(),
            description: desc.into(),
            parameters: params,
        },
    }
}

fn string_param() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": { "type": "string", "description": "The shell command to execute" },
            "timeout_ms": { "type": "integer", "description": "Timeout in ms", "default": 15000 }
        },
        "required": ["command"]
    })
}

fn read_param() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path to read" }
        },
        "required": ["path"]
    })
}

fn write_param() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path to write" },
            "content": { "type": "string", "description": "Content to write" }
        },
        "required": ["path", "content"]
    })
}

fn glob_param() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "Glob pattern to match" }
        },
        "required": ["pattern"]
    })
}

fn grep_param() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "Regex pattern to search" },
            "path": { "type": "string", "description": "File or directory to search" },
            "glob": { "type": "string", "description": "Optional glob filter", "default": "" }
        },
        "required": ["pattern", "path"]
    })
}

fn use_skill_param() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Skill name to load" }
        },
        "required": ["name"]
    })
}

pub static FULL_TOOL_DEFS: once_cell::sync::Lazy<Vec<ToolDef>> = once_cell::sync::Lazy::new(|| {
    vec![
        tool_def("shell", "Execute a shell command and return output. Use for running programs, scripts, and CLI tools. Blocks until completion or timeout (default 15s, max 300s). Output truncated at 50KB.", string_param()),
        tool_def("read", "Read a text file from the local filesystem. Binary files (null-byte detected) will be rejected. Max 100K characters.", read_param()),
        tool_def("write", "Write content to a file. Creates parent directories if missing. Overwrites existing files.", write_param()),
        tool_def("glob", "List files matching a glob pattern (e.g. '**/*.rs'). Max 200 results.", glob_param()),
        tool_def("grep", "Search file contents using a regex pattern. Returns matching lines. Max 200 matches. Binary files skipped.", grep_param()),
        tool_def("use_skill", "Load a skill's full instructions. Call this when you need specialized domain knowledge for a task.", use_skill_param()),
    ]
});

pub static PLAN_TOOL_DEFS: once_cell::sync::Lazy<Vec<ToolDef>> = once_cell::sync::Lazy::new(|| {
    vec![
        tool_def("read", "Read a text file from the local filesystem.", read_param()),
        tool_def("glob", "List files matching a glob pattern (e.g. '**/*.rs'). Max 200 results.", glob_param()),
        tool_def("grep", "Search file contents using a regex pattern.", grep_param()),
        tool_def("use_skill", "Load a skill's full instructions.", use_skill_param()),
    ]
});

fn _is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

fn res(content: String) -> Result<String, String> {
    Ok(content)
}

fn err(msg: impl Into<String>) -> Result<String, String> {
    Err(msg.into())
}

pub fn active_tool_defs(mode: crate::mode::AgentMode) -> &'static [ToolDef] {
    match mode {
        crate::mode::AgentMode::Plan => &PLAN_TOOL_DEFS,
        _ => &FULL_TOOL_DEFS,
    }
}

pub fn execute_tool(
    name: &str,
    args_str: &str,
    config: &crate::config::Config,
    skill_registry: Option<&crate::skill::SkillRegistry>,
) -> Result<String, String> {
    let args: Map<String, serde_json::Value> =
        serde_json::from_str(args_str).map_err(|e| format!("invalid args: {e}"))?;

    match name {
        "shell" => tool_shell(&args, config),
        "read" => tool_read(&args),
        "write" => tool_write(&args),
        "glob" => tool_glob(&args),
        "grep" => tool_grep(&args),
        "use_skill" => tool_use_skill(&args, skill_registry),
        _ => err(format!("unknown tool: {name}")),
    }
}

fn get_str(map: &Map<String, serde_json::Value>, key: &str) -> Result<String, String> {
    map.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing required param: {key}"))
}

fn get_u64(map: &Map<String, serde_json::Value>, key: &str, default: u64) -> u64 {
    map.get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
}

fn tool_shell(args: &Map<String, serde_json::Value>, config: &crate::config::Config) -> Result<String, String> {
    let command = get_str(args, "command")?;
    let timeout_ms = get_u64(args, "timeout_ms", (config.default_shell_timeout * 1000.0) as u64);
    let max_timeout_ms = (300.0 * 1000.0) as u64;
    let timeout_ms = timeout_ms.min(max_timeout_ms);

    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let shell = if cfg!(target_os = "windows") {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        "sh".into()
    };
    let arg_flag = if shell.contains("cmd") { "/C" } else { "-c" };

    let mut child = Command::new(&shell)
        .arg(arg_flag)
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let max_out = config.max_tool_output;
    let max_err = config.max_stderr_output;
    let (tx, rx) = std::sync::mpsc::channel();
    let tx2 = tx.clone();

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut out = String::new();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if out.len() < max_out {
                        out.push_str(&l);
                        out.push('\n');
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!("(read error: {e})"));
                    return;
                }
            }
        }
        let _ = tx.send(out);
    });

    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut out = String::new();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if out.len() < max_err {
                        out.push_str(&l);
                        out.push('\n');
                    }
                }
                Err(_) => break,
            }
        }
        if !out.is_empty() {
            let _ = tx2.send(format!("(stderr)\n{out}"));
        }
    });

    // Wait with timeout
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    let mut stdout_result = String::new();
    let mut stderr_result = String::new();

    while start.elapsed() < timeout {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(s) => {
                if s.starts_with("(stderr)") {
                    stderr_result = s;
                } else {
                    stdout_result = s;
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if stdout_result.is_empty() && stderr_result.is_empty() {
                    continue;
                }
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait().ok();

    let exit_info = match status {
        Some(s) if s.success() => String::new(),
        Some(s) => format!("\n[exit code: {}]", s.code().unwrap_or(-1)),
        None => "\n[timed out]".into(),
    };

    let output = match (stdout_result.as_str(), stderr_result.as_str()) {
        (o, s) if s.starts_with("(stderr)") => {
            format!("{}{}", o.trim_end(), s.replacen("(stderr)\n", "\n(stderr)\n", 1))
        }
        (o, _) => o.trim_end().to_string(),
    };

    let combined = format!("{}{}", output, exit_info);
    Ok(combined)
}

fn tool_read(args: &Map<String, serde_json::Value>) -> Result<String, String> {
    let path = get_str(args, "path")?;
    let path = expand_path(&path);

    // Binary detection
    let data = std::fs::read(&path).map_err(|e| format!("read failed: {e}"))?;
    if data.len() > 8192 && data[..8192].contains(&0) {
        return err("binary file detected, not reading");
    }
    let content = String::from_utf8_lossy(&data);
    let max_chars: usize = 100_000;
    if content.len() > max_chars {
        Ok(format!("{}...(truncated at {max_chars} chars)", &content[..max_chars]))
    } else {
        Ok(content.to_string())
    }
}

fn tool_write(args: &Map<String, serde_json::Value>) -> Result<String, String> {
    let path = get_str(args, "path")?;
    let content = get_str(args, "content")?;
    let path = expand_path(&path);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dirs: {e}"))?;
    }
    std::fs::write(&path, &content).map_err(|e| format!("write failed: {e}"))?;
    Ok(format!("wrote {} bytes to {}", content.len(), path.display()))
}

fn tool_glob(args: &Map<String, serde_json::Value>) -> Result<String, String> {
    let pattern = get_str(args, "pattern")?;
    let max_results = 200;
    let mut results = Vec::new();

    if let Some(parent) = std::path::Path::new(&pattern).parent() {
        let file_part = std::path::Path::new(&pattern)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "*".into());
        let dir: &std::path::Path = if parent.as_os_str().is_empty() { std::path::Path::new(".") } else { parent };

        let glob_pattern = glob::Pattern::new(&file_part).ok();
        for entry in walkdir::WalkDir::new(dir).max_depth(20).into_iter().filter_map(|e| e.ok()) {
            if results.len() >= max_results {
                break;
            }
            if let Some(ref gp) = glob_pattern {
                if let Some(name) = entry.file_name().to_str() {
                    if gp.matches(name) {
                        results.push(entry.path().to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    if results.is_empty() {
        Ok("(no matches)".into())
    } else {
        Ok(results.join("\n"))
    }
}

fn tool_grep(args: &Map<String, serde_json::Value>) -> Result<String, String> {
    let pattern = get_str(args, "pattern")?;
    let path = get_str(args, "path")?;
    let glob_filter = args.get("glob").and_then(|v| v.as_str()).unwrap_or("");
    let max_matches = 200;
    let mut matches = Vec::new();

    let re = regex::Regex::new(&pattern).map_err(|e| format!("invalid regex: {e}"))?;
    let path = expand_path(&path);

    let entries: Vec<std::path::PathBuf> = if path.is_dir() {
        walkdir::WalkDir::new(&path)
            .max_depth(20)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect()
    } else if path.exists() {
        vec![path.clone()]
    } else {
        return Ok("(path not found)".into());
    };

    for file_path in entries {
        if matches.len() >= max_matches {
            break;
        }

        if !glob_filter.is_empty() {
            let gp = glob::Pattern::new(glob_filter).map_err(|_| "invalid glob".to_string())?;
            if !gp.matches(file_path.to_string_lossy().as_ref()) {
                continue;
            }
        }

        let data = match std::fs::read(&file_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if data.len() > 8192 && data[..8192].contains(&0) {
            continue;
        }

        let content = String::from_utf8_lossy(&data);
        for (line_no, line) in content.lines().enumerate() {
            if matches.len() >= max_matches {
                break;
            }
            if re.is_match(line) {
                matches.push(format!("{}:{}:{}", file_path.display(), line_no + 1, line));
            }
        }
    }

    if matches.is_empty() {
        Ok("(no matches)".into())
    } else {
        Ok(matches.join("\n"))
    }
}

fn tool_use_skill(
    args: &Map<String, serde_json::Value>,
    registry: Option<&crate::skill::SkillRegistry>,
) -> Result<String, String> {
    let name = get_str(args, "name")?;
    match registry {
        Some(r) => r.load_content(&name).ok_or_else(|| format!("skill '{name}' not found")),
        None => err("skill system not initialized"),
    }
}

fn expand_path(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return std::path::PathBuf::from(format!("{}{}", home.display(), rest));
        }
    }
    std::path::PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_defs_count() {
        assert_eq!(FULL_TOOL_DEFS.len(), 6);
        assert_eq!(PLAN_TOOL_DEFS.len(), 4);
    }

    #[test]
    fn test_active_tool_defs() {
        assert_eq!(active_tool_defs(crate::mode::AgentMode::Auto).len(), 6);
        assert_eq!(active_tool_defs(crate::mode::AgentMode::Plan).len(), 4);
    }

    #[test]
    fn test_expand_tilde() {
        let p = expand_path("~/test.txt");
        assert!(p.to_string_lossy().contains("test.txt"));
    }

    #[test]
    fn test_binary_detection() {
        assert!(_is_binary(&[0, 1, 2]));
        assert!(!_is_binary(&[b'h', b'e', b'l', b'l', b'o']));
    }

    #[test]
    fn test_shell_echo() {
        let cfg = crate::config::Config::default();
        let args = serde_json::json!({"command": "echo hello relay"}).as_object().unwrap().clone();
        let result = tool_shell(&args, &cfg);
        assert!(result.unwrap_or_default().contains("hello relay"));
    }

    #[test]
    fn test_unknown_tool() {
        let cfg = crate::config::Config::default();
        let result = execute_tool("nonexistent", "{}", &cfg, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_glob_no_matches() {
        let args = serde_json::json!({"pattern": "/nonexistent_dir_xyz/*.rs"})
            .as_object().unwrap().clone();
        let result = tool_glob(&args);
        assert!(result.unwrap_or_default().contains("no matches"));
    }
}
