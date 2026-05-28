use std::collections::HashMap;

pub struct EnvInfo {
    pub platform: String,
    pub os: String,
    pub os_version: String,
    pub default_shell: String,
    pub available_shells: Vec<String>,
    pub python_version: String,
    pub cwd: String,
    pub tools: HashMap<String, bool>,
}

pub fn detect_environment() -> EnvInfo {
    let platform = std::env::consts::OS.to_string();
    let os = if cfg!(target_os = "windows") {
        "Windows".into()
    } else if cfg!(target_os = "linux") {
        "Linux".into()
    } else if cfg!(target_os = "macos") {
        "macOS".into()
    } else {
        platform.clone()
    };

    let os_version = std::env::var("OS_VERSION").ok().unwrap_or_default();
    let default_shell = std::env::var("COMSPEC")
        .or_else(|_| std::env::var("SHELL"))
        .unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                "cmd.exe".into()
            } else {
                "sh".into()
            }
        });

    let available_shells = {
        let mut shells = Vec::new();
        for name in &["cmd.exe", "powershell.exe", "bash", "zsh", "fish", "sh"] {
            if which::which(name).is_ok() {
                shells.push(name.to_string());
            }
        }
        shells
    };

    let python_version = std::process::Command::new("python")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_default();

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "?".into());

    let mut tools = HashMap::new();
    for tool in &["git", "node", "npm", "cargo", "go", "make", "gcc", "clang"] {
        tools.insert(tool.to_string(), which::which(tool).is_ok());
    }

    EnvInfo {
        platform,
        os,
        os_version,
        default_shell,
        available_shells,
        python_version,
        cwd,
        tools,
    }
}

fn sh_hints(shell: &str) -> &'static str {
    let lower = shell.to_lowercase();
    if lower.contains("cmd") || lower.ends_with("cmd.exe") {
        "Use cmd.exe syntax: 'dir', 'type file.txt', 'echo %VAR%', 'findstr pattern file'."
    } else if lower.contains("powershell") {
        "Use PowerShell syntax: 'Get-ChildItem', 'Get-Content', 'Select-String', '$env:VAR'."
    } else {
        "Use POSIX shell syntax: ls, cat, grep, $VAR."
    }
}

pub fn build_system_prompt(
    env: &EnvInfo,
    mode: &crate::mode::ModeState,
    memory: &crate::memory::MemoryStore,
    skills: &crate::skill::SkillRegistry,
) -> String {
    let shell = env.default_shell.clone();
    let hints = sh_hints(&shell);

    let avail: Vec<&str> = env.tools.iter()
        .filter(|(_, &v)| v)
        .map(|(k, _)| k.as_str())
        .collect();

    let mut prompt = format!(
        r#"You are Relay, an AI agent running in a terminal environment.

## Environment
- OS: {} ({})
- Shell: {}
- CWD: {}
- Available: {}
- Python: {}

## Shell Hint
{}

## Capabilities
You have access to shell commands and file reading/writing tools. You can use glob and grep for file discovery.
You can make multiple tool calls in sequence. If a tool fails, diagnose the issue and try a different approach.
Use the default shell for commands unless you have a specific reason to use another.

## Behavior
{}"#,
        env.os, env.os_version, shell, env.cwd,
        avail.join(", "),
        env.python_version.trim(),
        hints,
        mode.system_prompt_suffix(),
    );

    // Memory
    if !memory.entries.is_empty() {
        prompt.push_str("\n\n## Auto Memory\n");
        for e in &memory.entries {
            prompt.push_str(&format!("- [{}] {}: {}\n", e.memory_type_str(), e.name, e.description));
        }
    }

    // Skills
    if !skills.is_empty() {
        prompt.push_str("\n## Available Skills\n");
        prompt.push_str(&skills.available_list());
        prompt.push_str("\nCall use_skill(\"skill-name\") to load a skill's full instructions.");
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_something() {
        let env = detect_environment();
        assert!(!env.platform.is_empty());
        assert!(!env.cwd.is_empty());
    }

    #[test]
    fn test_build_prompt_contains_sections() {
        let env = detect_environment();
        let mode = crate::mode::ModeState::new(crate::mode::AgentMode::Auto);
        let memory = crate::memory::MemoryStore { entries: Vec::new() };
        let skills = crate::skill::SkillRegistry { skills: Vec::new() };
        let prompt = build_system_prompt(&env, &mode, &memory, &skills);
        assert!(prompt.contains("Environment"));
        assert!(prompt.contains("Relay"));
    }
}
