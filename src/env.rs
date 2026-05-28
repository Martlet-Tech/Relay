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

fn shell_hint(shell: &str) -> &'static str {
    let lower = shell.to_lowercase();
    if lower.contains("cmd") || lower.ends_with("cmd.exe") {
        "Use cmd.exe syntax: 'dir', 'type file.txt', 'echo %VAR%', 'findstr pattern file'."
    } else if lower.contains("powershell") {
        "Use PowerShell syntax: 'Get-ChildItem', 'Get-Content', 'Select-String', '$env:VAR'."
    } else {
        "Use POSIX shell syntax: ls, cat, grep, $VAR."
    }
}

static PROMPT_TEMPLATE: &str = include_str!("../prompts/system.md");

pub fn build_system_prompt(
    env: &EnvInfo,
    mode: &crate::mode::ModeState,
    memory: &crate::memory::MemoryStore,
    skills: &crate::skill::SkillRegistry,
) -> String {
    let avail: Vec<&str> = env.tools.iter()
        .filter(|(_, &v)| v)
        .map(|(k, _)| k.as_str())
        .collect();

    let hints = shell_hint(&env.default_shell);

    // Memory section
    let memory_text = if memory.entries.is_empty() {
        "(none)".into()
    } else {
        memory.entries.iter()
            .map(|e| format!("- [{}] {}: {}", e.memory_type_str(), e.name, e.description))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Skills section
    let skills_text = if skills.is_empty() {
        "(none)".into()
    } else {
        skills.available_list()
    };

    let mut prompt = PROMPT_TEMPLATE.to_string();
    prompt = prompt.replace("{{OS}}", &env.os);
    prompt = prompt.replace("{{OS_VERSION}}", &env.os_version);
    prompt = prompt.replace("{{SHELL}}", &env.default_shell);
    prompt = prompt.replace("{{CWD}}", &env.cwd);
    prompt = prompt.replace("{{TOOLS}}", &avail.join(", "));
    prompt = prompt.replace("{{PYTHON_VERSION}}", env.python_version.trim());
    prompt = prompt.replace("{{SHELL_HINT}}", hints);
    prompt = prompt.replace("{{MODE_BEHAVIOR}}", &mode.system_prompt_suffix());
    prompt = prompt.replace("{{MEMORY}}", &memory_text);
    prompt = prompt.replace("{{SKILLS}}", &skills_text);

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
        assert!(prompt.contains("Relay"));
        assert!(prompt.contains("Thinking First"));
    }
}
