// Central registry of JSON Schemas for Sworm config files.
//
// Each entry pairs a schema with the glob patterns that identify the
// file. The frontend fetches this list on boot and hands each entry to
// Monaco so that opening a matching file gets autocomplete, validation,
// and hover docs for free.
//
// Adding a new config file is a three-step change:
//   <> define/extend the canonical model under models/ (schema is derived)
//   <> append a `ConfigSchemaEntry` below
//   <> ship it; the frontend picks it up automatically on next boot

use schemars::schema_for;
use serde_json::json;
use sworm_protocol::config_schemas::ConfigSchemaEntry;

use crate::services::builtins::BuiltinCatalogService;
use sworm_protocol::{
    settings::{settings_layer_schema, SettingsLayerKind},
    task::TasksFile,
};

pub fn all_config_schemas() -> Result<Vec<ConfigSchemaEntry>, String> {
    let lsp_server_ids = BuiltinCatalogService::list_server_definitions()?
        .into_iter()
        .map(|server| server.server_definition_id)
        .collect::<Vec<_>>();

    Ok(vec![
        ConfigSchemaEntry {
            id: "sworm.tasks".into(),
            file_match: vec!["**/.sworm/tasks.json".into()],
            schema: serde_json::to_value(schema_for!(TasksFile)).expect("tasks schema serializes"),
        },
        ConfigSchemaEntry {
            id: "sworm.settings.global".into(),
            file_match: vec!["**/sworm/settings.jsonc".into()],
            schema: settings_layer_schema(SettingsLayerKind::Global, &lsp_server_ids),
        },
        ConfigSchemaEntry {
            id: "sworm.settings.folder".into(),
            file_match: vec!["**/.sworm/settings.jsonc".into()],
            schema: settings_layer_schema(SettingsLayerKind::Folder, &lsp_server_ids),
        },
        ConfigSchemaEntry {
            id: "sworm.shortcuts".into(),
            file_match: vec!["**/shortcuts.jsonc".into()],
            schema: shortcuts_file_schema(),
        },
    ])
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

#[cfg(test)]
mod tests {
    use super::*;
    use sworm_protocol::settings::CANONICAL_PROVIDER_IDS;

    #[test]
    fn registers_settings_jsonc_schemas() {
        let schemas = all_config_schemas().expect("schemas build");
        let folder_settings = schemas
            .iter()
            .find(|entry| entry.id == "sworm.settings.folder")
            .expect("folder settings schema registered");

        assert_eq!(folder_settings.file_match, vec!["**/.sworm/settings.jsonc"]);

        let global_settings = schemas
            .iter()
            .find(|entry| entry.id == "sworm.settings.global")
            .expect("global settings schema registered");

        assert_eq!(global_settings.file_match, vec!["**/sworm/settings.jsonc"]);
    }

    #[test]
    fn registers_shortcuts_jsonc_schema() {
        let schemas = all_config_schemas().expect("schemas build");
        let shortcuts = schemas
            .iter()
            .find(|entry| entry.id == "sworm.shortcuts")
            .expect("shortcuts schema registered");

        assert_eq!(shortcuts.file_match, vec!["**/shortcuts.jsonc"]);
        assert!(shortcuts.schema.to_string().contains("unboundCommands"));
    }

    #[test]
    fn settings_schema_contains_canonical_sections_and_ids() {
        let rendered = settings_layer_schema(
            SettingsLayerKind::Global,
            &["dev.sworm.vtsls::vtsls".to_string()],
        )
        .to_string();

        for section in [
            "window",
            "terminal",
            "nix",
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
        let rendered = settings_layer_schema(
            SettingsLayerKind::Global,
            &["dev.sworm.vtsls::vtsls".to_string()],
        )
        .to_string();

        assert!(rendered.contains("binary_path_override"));
        assert!(rendered.contains("runtime_path_override"));
        assert!(rendered.contains("\"string\",\"null\""));
        assert!(rendered.contains("font_size"));
        assert!(!rendered.contains("\"integer\",\"null\""));
    }
}
