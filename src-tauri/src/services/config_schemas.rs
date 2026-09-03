// Central registry of JSON Schemas for Sworm config files.
//
// Each entry pairs a schema with the glob patterns that identify the
// file. The frontend fetches this list on boot and hands each entry to
// Monaco so that opening a matching file gets autocomplete, validation,
// and hover docs for free.
//
// Adding a new config file is a three-step change:
//   <> define the canonical model under models/
//   <> append a `ConfigSchemaEntry` below
//   <> ship it; the frontend picks it up automatically on next boot

use schemars::schema_for;
use serde::Serialize;
use serde_json::json;

use crate::models::{settings::CANONICAL_PROVIDER_IDS, task::TasksFile};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaEntry {
    /// Stable identifier used by the frontend registry (e.g. `sworm.tasks`).
    pub id: String,

    /// Glob patterns matched against the opened file's URI.
    pub file_match: Vec<String>,

    /// JSON Schema object.
    pub schema: serde_json::Value,
}

pub fn all_config_schemas() -> Vec<ConfigSchemaEntry> {
    vec![
        ConfigSchemaEntry {
            id: "sworm.tasks".into(),
            file_match: vec!["**/.sworm/tasks.json".into()],
            schema: serde_json::to_value(schema_for!(TasksFile)).expect("tasks schema serializes"),
        },
        ConfigSchemaEntry {
            id: "sworm.settings".into(),
            file_match: vec!["**/.sworm/settings.jsonc".into()],
            schema: settings_file_schema(),
        },
        ConfigSchemaEntry {
            id: "sworm.shortcuts".into(),
            file_match: vec!["**/shortcuts.jsonc".into()],
            schema: shortcuts_file_schema(),
        },
    ]
}

fn settings_file_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Sworm settings",
        "description": "Project settings can override executable paths and args for providers and LSP servers. Only trust settings from repositories you trust.",
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "general": general_settings_schema(),
            "explorer": explorer_settings_schema(),
            "formatting": formatting_settings_schema(),
            "providers": providers_settings_schema(),
            "lsp": lsp_settings_schema()
        }
    })
}

fn shortcuts_file_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Sworm shortcuts",
        "description": "Global keyboard shortcut overrides. Defaults live in Sworm's command registry; this file stores only user changes.",
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "version": {
                "type": "integer",
                "const": 1,
                "default": 1
            },
            "bindings": {
                "type": "array",
                "default": [],
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "required": ["command", "key"],
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Stable command ID, such as toggle-command-palette or editor:editor.action.formatDocument."
                        },
                        "key": {
                            "type": "string",
                            "description": "Shortcut chord, such as Ctrl+P or Ctrl+Shift+P."
                        }
                    }
                }
            },
            "unboundCommands": {
                "type": "array",
                "default": [],
                "items": { "type": "string" },
                "description": "Command IDs whose default shortcuts are explicitly disabled."
            }
        }
    })
}

fn general_settings_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "theme": {
                "type": "string",
                "default": "system",
                "description": "Theme preference. Current built-in value is system."
            },
            "terminal_font_family": {
                "type": "string",
                "default": "JetBrains Mono"
            },
            "terminal_font_size": {
                "type": "integer",
                "minimum": 1,
                "default": 13
            },
            "nix_eval_timeout_secs": {
                "type": "integer",
                "minimum": 1,
                "default": 600
            }
        }
    })
}

fn explorer_settings_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "exclude": {
                "type": "object",
                "additionalProperties": { "type": "boolean" },
                "default": { "**/.git": true, "**/.svn": true, "**/.hg": true, "**/.DS_Store": true, "**/Thumbs.db": true },
                "description": "Globs hidden from the file explorer, matched against project-relative paths. Setting this replaces the defaults; map a glob to false to keep it listed."
            },
            "exclude_gitignore": {
                "type": "boolean",
                "default": false,
                "description": "Hide files ignored by git instead of dimming them."
            },
            "compact_folders": {
                "type": "boolean",
                "default": true,
                "description": "Collapse single-child directory chains into one row, such as src/lib/utils."
            }
        }
    })
}

fn formatting_settings_schema() -> serde_json::Value {
    let group_schema = json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "formatter": {
                "type": "string",
                "enum": ["lsp", "biome", "nixfmt", "disabled"]
            }
        }
    });

    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "javascript_typescript": group_schema.clone(),
            "json": group_schema.clone(),
            "nix": group_schema
        }
    })
}

fn providers_settings_schema() -> serde_json::Value {
    let provider_schema = provider_settings_schema();
    let provider_properties = CANONICAL_PROVIDER_IDS
        .iter()
        .map(|provider_id| (provider_id.to_string(), provider_schema.clone()))
        .collect::<serde_json::Map<_, _>>();

    json!({
        "type": "object",
        "description": "Provider entries are keyed by internal provider ID. Unknown provider IDs are ignored at runtime with diagnostics.",
        "additionalProperties": provider_schema,
        "properties": provider_properties
    })
}

fn provider_settings_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "enabled": {
                "type": "boolean",
                "default": true
            },
            "binary_path_override": {
                "type": ["string", "null"],
                "default": null,
                "description": "Optional provider executable override. Project settings can change what Sworm executes."
            },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" },
                "default": [],
                "description": "Additional provider CLI args. Project settings can change what Sworm executes."
            }
        }
    })
}

fn lsp_settings_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "servers": {
                "type": "object",
                "description": "LSP server entries are keyed by BuiltinCatalogService server_definition_id, formatted as ${builtin_id}::${server_id}. Unknown server IDs are ignored at runtime with diagnostics.",
                "additionalProperties": lsp_server_settings_schema(),
                "properties": {
                    "dev.sworm.vtsls::vtsls": lsp_server_settings_schema()
                }
            }
        }
    })
}

fn lsp_server_settings_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "enabled": {
                "type": "boolean",
                "default": true
            },
            "binary_path_override": {
                "type": ["string", "null"],
                "default": null,
                "description": "Optional language server executable override. Project settings can change what Sworm executes."
            },
            "runtime_path_override": {
                "type": ["string", "null"],
                "default": null,
                "description": "Optional runtime executable override. Project settings can change what Sworm executes."
            },
            "runtime_args": {
                "type": "array",
                "items": { "type": "string" },
                "default": [],
                "description": "Additional runtime args. Project settings can change what Sworm executes."
            },
            "extra_args": {
                "type": "array",
                "items": { "type": "string" },
                "default": [],
                "description": "Additional language server args. Project settings can change what Sworm executes."
            },
            "trace": {
                "type": "string",
                "enum": ["off", "messages", "verbose"],
                "default": "off"
            },
            "settings": {
                "type": ["object", "array", "string", "number", "boolean", "null"],
                "default": null,
                "description": "Native LSP settings object/value sent to the language server."
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_project_settings_jsonc_schema() {
        let schemas = all_config_schemas();
        let settings = schemas
            .iter()
            .find(|entry| entry.id == "sworm.settings")
            .expect("settings schema registered");

        assert_eq!(settings.file_match, vec!["**/.sworm/settings.jsonc"]);
        assert!(settings
            .schema
            .to_string()
            .contains("Project settings can override executable paths"));
    }

    #[test]
    fn registers_shortcuts_jsonc_schema() {
        let schemas = all_config_schemas();
        let shortcuts = schemas
            .iter()
            .find(|entry| entry.id == "sworm.shortcuts")
            .expect("shortcuts schema registered");

        assert_eq!(shortcuts.file_match, vec!["**/shortcuts.jsonc"]);
        assert!(shortcuts.schema.to_string().contains("unboundCommands"));
    }

    #[test]
    fn settings_schema_contains_canonical_sections_and_ids() {
        let schema = settings_file_schema();
        let rendered = schema.to_string();

        for section in [
            "general",
            "explorer",
            "exclude_gitignore",
            "compact_folders",
            "formatting",
            "providers",
            "lsp",
        ] {
            assert!(rendered.contains(section), "schema includes {section}");
        }
        for provider_id in CANONICAL_PROVIDER_IDS {
            assert!(
                rendered.contains(&provider_id.to_string()),
                "schema includes provider ID {provider_id}"
            );
        }
        assert!(rendered.contains("dev.sworm.vtsls::vtsls"));
    }

    #[test]
    fn settings_schema_allows_null_only_on_nullable_fields() {
        let schema = settings_file_schema();
        let rendered = schema.to_string();

        assert!(rendered.contains("binary_path_override"));
        assert!(rendered.contains("runtime_path_override"));
        assert!(rendered.contains("\"string\",\"null\""));
        assert!(rendered.contains("terminal_font_size"));
        assert!(!rendered.contains("\"integer\",\"null\""));
    }
}
