use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub source_path: PathBuf,
}

pub struct SkillRegistry {
    pub skills: Vec<Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    pub fn discover(config: &crate::config::Config) -> Result<Self, crate::error::RelayError> {
        let mut registry = Self::new();

        for dir_str in &config.skill_dirs {
            let dir = expand_path(dir_str);
            if !dir.exists() {
                continue;
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let skill_dir = entry.path();
                if !skill_dir.is_dir() {
                    continue;
                }
                let skill_file = skill_dir.join("SKILL.md");
                if !skill_file.exists() {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&skill_file) {
                    if let Some(skill) = parse_skill(&content, &skill_file) {
                        registry.skills.push(skill);
                    }
                }
            }
        }

        Ok(registry)
    }

    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn load_content(&self, name: &str) -> Option<String> {
        self.skills.iter().find(|s| s.name == name).map(|s| s.content.clone())
    }

    pub fn available_list(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        self.skills.iter()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

fn expand_path(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return PathBuf::from(format!("{}{}", home.display(), rest));
        }
    }
    PathBuf::from(p)
}

fn parse_skill(content: &str, source_path: &PathBuf) -> Option<Skill> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let end = content[3..].find("---")?;
    let frontmatter = &content[3..3 + end];
    let body = content[3 + end + 3..].trim();

    let mut name = String::new();
    let mut description = String::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().to_string();
        }
    }

    if name.is_empty() {
        return None;
    }

    Some(Skill {
        name,
        description,
        content: body.to_string(),
        source_path: source_path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill() {
        let content = r#"---
name: test-skill
description: A test skill
---
This is the skill body"#;
        let skill = parse_skill(content, &PathBuf::from("SKILL.md"));
        assert!(skill.is_some());
        let s = skill.unwrap();
        assert_eq!(s.name, "test-skill");
        assert_eq!(s.description, "A test skill");
        assert_eq!(s.content, "This is the skill body");
    }

    #[test]
    fn test_parse_skill_no_frontmatter() {
        let content = "Just content";
        let skill = parse_skill(content, &PathBuf::from("SKILL.md"));
        assert!(skill.is_none());
    }

    #[test]
    fn test_find_skill() {
        let mut registry = SkillRegistry::new();
        registry.skills.push(Skill {
            name: "alpha".into(),
            description: "Alpha skill".into(),
            content: "content".into(),
            source_path: PathBuf::from("."),
        });
        assert!(registry.find("alpha").is_some());
        assert!(registry.find("beta").is_none());
    }
}
