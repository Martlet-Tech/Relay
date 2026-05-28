use crate::error::RelayError;

#[derive(Debug, Clone)]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "user" => MemoryType::User,
            "feedback" => MemoryType::Feedback,
            "project" => MemoryType::Project,
            "reference" => MemoryType::Reference,
            _ => MemoryType::Reference,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::User => "user",
            MemoryType::Feedback => "feedback",
            MemoryType::Project => "project",
            MemoryType::Reference => "reference",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub content: String,
    pub memory_type: MemoryType,
}

impl MemoryEntry {
    pub fn memory_type_str(&self) -> &'static str {
        self.memory_type.as_str()
    }
}

pub struct MemoryStore {
    pub entries: Vec<MemoryEntry>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn load(config: &crate::config::Config) -> Result<Self, RelayError> {
        let mut store = Self::new();

        let root = if config.memory_root == "auto" {
            find_workspace_memory_dir()
        } else {
            Some(crate::config::expand_path(&config.memory_root))
        };

        let memory_dir = match root {
            Some(d) => d,
            None => return Ok(store),
        };

        if !memory_dir.exists() {
            return Ok(store);
        }

        // Read MEMORY.md index
        let index_path = memory_dir.join("MEMORY.md");
        let index_content = match std::fs::read_to_string(&index_path) {
            Ok(c) => c,
            Err(_) => return Ok(store),
        };

        // Parse index entries: "- [Title](file.md) — description"
        for line in index_content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("- [") {
                if let Some(end_bracket) = rest.find("](") {
                    let name = &rest[..end_bracket];
                    let after_open = &rest[end_bracket + 2..];
                    if let Some(end_paren) = after_open.find(')') {
                        let file_name = &after_open[..end_paren];
                        let desc = after_open[end_paren + 1..].trim()
                            .trim_start_matches("—")
                            .trim_start_matches('-')
                            .trim();

                        let file_path = memory_dir.join(file_name);
                        if let Ok(content) = std::fs::read_to_string(&file_path) {
                            let (memory_type, entry_content) = parse_memory_file(&content);
                            store.entries.push(MemoryEntry {
                                name: name.to_string(),
                                description: desc.to_string(),
                                content: entry_content,
                                memory_type,
                            });
                        }
                    }
                }
            }
        }

        Ok(store)
    }

    pub fn index_summary(&self) -> String {
        if self.entries.is_empty() {
            return "(no memory loaded)".into();
        }
        let mut lines: Vec<String> = Vec::new();
        for e in &self.entries {
            lines.push(format!("- [{}] {}", e.memory_type_str(), e.description));
        }
        lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn find_workspace_memory_dir() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for anc in cwd.ancestors() {
        let candidate = anc.join("memory");
        if candidate.join("MEMORY.md").exists() {
            return Some(candidate);
        }
    }
    None
}

fn parse_memory_file(content: &str) -> (MemoryType, String) {
    let mut memory_type = MemoryType::Reference;
    let mut body_start = 0;

    if content.trim_start().starts_with("---") {
        let end = content[3..].find("---").map(|p| p + 6);
        if let Some(end_pos) = end {
            let frontmatter = &content[3..end_pos - 3];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("type:") {
                    memory_type = MemoryType::from_str(val.trim());
                }
                if let Some(val) = line.strip_prefix("type: ") {
                    memory_type = MemoryType::from_str(val.trim());
                }
            }
            body_start = end_pos;
        }
    }

    let body = content[body_start..].trim().to_string();
    (memory_type, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_file_with_frontmatter() {
        let content = "---\ntype: user\n---\nThis is memory content";
        let (mtype, body) = parse_memory_file(content);
        assert!(matches!(mtype, MemoryType::User));
        assert_eq!(body, "This is memory content");
    }

    #[test]
    fn test_parse_memory_file_no_frontmatter() {
        let content = "Just content";
        let (mtype, body) = parse_memory_file(content);
        assert!(matches!(mtype, MemoryType::Reference));
        assert_eq!(body, "Just content");
    }

    #[test]
    fn test_memory_type_from_str() {
        assert!(matches!(MemoryType::from_str("user"), MemoryType::User));
        assert!(matches!(MemoryType::from_str("feedback"), MemoryType::Feedback));
        assert!(matches!(MemoryType::from_str("project"), MemoryType::Project));
        assert!(matches!(MemoryType::from_str("reference"), MemoryType::Reference));
        assert!(matches!(MemoryType::from_str("unknown"), MemoryType::Reference));
    }

    #[test]
    fn test_new_store_empty() {
        let store = MemoryStore::new();
        assert!(store.entries.is_empty());
        assert!(store.is_empty());
    }
}
