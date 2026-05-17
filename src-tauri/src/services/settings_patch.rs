use jsonc_parser::{
    cst::{CstInputValue, CstRootNode},
    ParseOptions,
};
use serde_json::Value;

pub fn patch_top_level_section(
    original: &str,
    section: &str,
    value: &Value,
) -> Result<String, String> {
    let root = CstRootNode::parse(original, &ParseOptions::default())
        .map_err(|error| format!("Failed to parse settings JSONC: {error}"))?;
    let object = root
        .object_value_or_create()
        .ok_or_else(|| "Settings JSONC root must be an object".to_string())?;
    let input = serde_value_to_cst_input(value)?;

    match object.get(section) {
        Some(property) => property.set_value(input),
        None => {
            object.append(section, input);
        }
    }

    Ok(root.to_string())
}

fn serde_value_to_cst_input(value: &Value) -> Result<CstInputValue, String> {
    Ok(match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(
            values
                .iter()
                .map(serde_value_to_cst_input)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), serde_value_to_cst_input(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patch_preserves_unknown_keys_and_untouched_comments() {
        let original = r#"{
  // Provider comment must stay.
  "providers": {
    "claude_code": { "enabled": true }
  },
  "future_top": { "keep": true },
  "general": {
    // Touched-section comments may be lost.
    "terminal_font_size": 13
  }
}
"#;

        let patched = patch_top_level_section(
            original,
            "general",
            &json!({
                "terminal_font_family": "Iosevka",
                "terminal_font_size": 15
            }),
        )
        .expect("patch succeeds");

        assert!(patched.contains("Provider comment must stay"));
        assert!(patched.contains("future_top"));
        assert!(patched.contains("Iosevka"));
        assert!(patched.contains("terminal_font_size"));

        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched JSONC parses");
        assert_eq!(parsed["future_top"]["keep"], json!(true));
        assert_eq!(parsed["general"]["terminal_font_size"], json!(15));
    }

    #[test]
    fn patch_adds_missing_top_level_section() {
        let patched = patch_top_level_section(
            r#"{
  "future_top": true
}
"#,
            "formatting",
            &json!({ "json": { "formatter": "biome" } }),
        )
        .expect("patch succeeds");

        assert!(patched.contains("future_top"));
        assert!(patched.contains("formatting"));

        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched JSONC parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(parsed["formatting"]["json"]["formatter"], json!("biome"));
    }

    #[test]
    fn patch_creates_object_for_empty_file() {
        let patched = patch_top_level_section("", "general", &json!({ "theme": "system" }))
            .expect("patch succeeds");

        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched JSONC parses");
        assert_eq!(parsed["general"]["theme"], json!("system"));
    }

    #[test]
    fn patch_rejects_non_object_root() {
        let error = patch_top_level_section("[]", "general", &json!({ "theme": "system" }))
            .expect_err("non-object root rejected");

        assert!(error.contains("root must be an object"));
    }
}
