//! Pure input validation and default-actor resolution.

pub(super) const DEFAULT_ACTOR: &str = "sworm";
pub(super) const DEFAULT_ISSUE_PREFIX: &str = "ISSUE";
pub(super) const DEFAULT_EPIC_PREFIX: &str = "EPIC";
pub(super) const DEFAULT_COMMENT_PREFIX: &str = "NOTE";

pub(super) fn actor(value: Option<&str>) -> &str {
    value
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(DEFAULT_ACTOR)
}

pub(super) fn validate_title(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err("Value must not be empty".to_string())
    } else {
        Ok(())
    }
}

pub(super) fn validate_priority(value: i64) -> Result<(), String> {
    if (0..=5).contains(&value) {
        Ok(())
    } else {
        Err("Priority must be between 0 and 5".to_string())
    }
}

pub(super) fn validate_issue_status(value: &str) -> Result<(), String> {
    match value {
        "todo" | "in_progress" | "blocked" | "completed" | "wont_fix" | "archived" => Ok(()),
        _ => Err(format!("Invalid issue status: {}", value)),
    }
}

pub(super) fn validate_epic_status(value: &str) -> Result<(), String> {
    match value {
        "todo" | "in_progress" | "completed" | "archived" => Ok(()),
        _ => Err(format!("Invalid epic status: {}", value)),
    }
}

pub(super) fn validate_assignee(kind: &str, id: Option<&str>) -> Result<(), String> {
    match kind {
        "human" | "agent" | "session" => {
            if id.is_some_and(|v| !v.trim().is_empty()) {
                Ok(())
            } else {
                Err(format!("assigneeId required for assigneeKind {}", kind))
            }
        }
        "unassigned" => Ok(()),
        _ => Err(format!("Invalid assignee kind: {}", kind)),
    }
}

pub(super) fn validate_tags(tags: &[String]) -> Result<(), String> {
    if tags.iter().any(|tag| tag.trim().is_empty()) {
        Err("Tags must not be empty".to_string())
    } else {
        Ok(())
    }
}

pub(super) fn validate_config_key(key: &str) -> Result<(), String> {
    match key {
        "issue_prefix" | "epic_prefix" | "comment_prefix" => Ok(()),
        _ => Err(format!("Invalid issue config key: {}", key)),
    }
}

pub(super) fn validate_prefix(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_uppercase();
    let mut chars = normalized.chars();
    let Some(first) = chars.next() else {
        return Err("Prefix must not be empty".to_string());
    };
    if !first.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric()) {
        return Err(
            "Prefix must start with a letter and contain only letters or numbers".to_string(),
        );
    }
    Ok(normalized)
}
