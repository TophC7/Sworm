use crate::models::builtins::{
    BuiltinCatalog, BuiltinDocumentSelector, BuiltinFormatterGroupId, BuiltinFormatterPolicy,
    BuiltinLanguageContribution, BuiltinLspServerDefinition, BuiltinManifest,
    BuiltinRuntimeCatalog, BuiltinSettingsCatalog, BuiltinSettingsPage, BuiltinSettingsPageKind,
};
use crate::services::settings::FormatterSelection;
use serde_json::from_str;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

struct BuiltinAsset {
    id: &'static str,
    manifest: &'static str,
    resources: &'static [(&'static str, &'static str)],
}

const BUILTIN_ASSETS: &[BuiltinAsset] = &[
    BuiltinAsset {
        id: "dev.sworm.biome",
        manifest: include_str!("../../builtins/dev.sworm.biome/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.biome/settings.schema.json"),
        )],
    },
    BuiltinAsset {
        id: "dev.sworm.css",
        manifest: include_str!("../../builtins/dev.sworm.css/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.css/settings.schema.json"),
        )],
    },
    BuiltinAsset {
        id: "dev.sworm.html",
        manifest: include_str!("../../builtins/dev.sworm.html/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.html/settings.schema.json"),
        )],
    },
    BuiltinAsset {
        id: "dev.sworm.nix",
        manifest: include_str!("../../builtins/dev.sworm.nix/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.nix/settings.schema.json"),
        )],
    },
    BuiltinAsset {
        id: "dev.sworm.svelte",
        manifest: include_str!("../../builtins/dev.sworm.svelte/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.svelte/settings.schema.json"),
        )],
    },
    BuiltinAsset {
        id: "dev.sworm.tailwind",
        manifest: include_str!("../../builtins/dev.sworm.tailwind/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.tailwind/settings.schema.json"),
        )],
    },
    BuiltinAsset {
        id: "dev.sworm.vtsls",
        manifest: include_str!("../../builtins/dev.sworm.vtsls/builtin.json"),
        resources: &[(
            "settings.schema.json",
            include_str!("../../builtins/dev.sworm.vtsls/settings.schema.json"),
        )],
    },
];

#[derive(Debug, Clone)]
struct LoadedBuiltinManifest {
    manifest: BuiltinManifest,
}

#[derive(Debug, Clone)]
pub struct LoadedBuiltinLspServerDefinition {
    pub server_definition_id: String,
    pub builtin_id: String,
    pub builtin_label: String,
    pub definition: BuiltinLspServerDefinition,
}

#[derive(Debug, Clone)]
struct LoadedBuiltinCatalog {
    catalog: BuiltinCatalog,
    server_definitions: Vec<LoadedBuiltinLspServerDefinition>,
    server_definitions_by_id: HashMap<String, LoadedBuiltinLspServerDefinition>,
}

#[derive(Clone, Copy)]
struct PageOverlay {
    id: &'static str,
    kind: BuiltinSettingsPageKind,
    label: &'static str,
    icon_filename: &'static str,
    language_ids: &'static [&'static str],
    formatter_group: Option<BuiltinFormatterGroupId>,
}

const PAGE_OVERLAYS: &[PageOverlay] = &[
    PageOverlay {
        id: "language:javascript-typescript",
        kind: BuiltinSettingsPageKind::Language,
        label: "JS / TypeScript",
        icon_filename: "index.ts",
        language_ids: &["javascript", "typescript"],
        formatter_group: Some(BuiltinFormatterGroupId::JavascriptTypescript),
    },
    PageOverlay {
        id: "language:json",
        kind: BuiltinSettingsPageKind::Language,
        label: "JSON",
        icon_filename: "data.json",
        language_ids: &["json"],
        formatter_group: Some(BuiltinFormatterGroupId::Json),
    },
    PageOverlay {
        id: "language:css",
        kind: BuiltinSettingsPageKind::Language,
        label: "CSS",
        icon_filename: "index.css",
        language_ids: &["css", "scss", "less"],
        formatter_group: None,
    },
    PageOverlay {
        id: "language:nix",
        kind: BuiltinSettingsPageKind::Nix,
        label: "Nix",
        icon_filename: "flake.nix",
        language_ids: &["nix"],
        formatter_group: Some(BuiltinFormatterGroupId::Nix),
    },
];

static LOADED_BUILTINS: OnceLock<Result<LoadedBuiltinCatalog, String>> = OnceLock::new();

pub struct BuiltinCatalogService;

impl BuiltinCatalogService {
    pub fn catalog() -> Result<BuiltinCatalog, String> {
        Ok(loaded_catalog()?.catalog.clone())
    }

    pub fn server_definition_id(builtin_id: &str, server_id: &str) -> String {
        format!("{}::{}", builtin_id, server_id)
    }

    pub fn list_server_definitions() -> Result<Vec<LoadedBuiltinLspServerDefinition>, String> {
        Ok(loaded_catalog()?.server_definitions.clone())
    }

    pub fn find_server_definition(
        server_definition_id: &str,
    ) -> Result<Option<LoadedBuiltinLspServerDefinition>, String> {
        Ok(loaded_catalog()?
            .server_definitions_by_id
            .get(server_definition_id)
            .cloned())
    }
}

fn loaded_catalog() -> Result<&'static LoadedBuiltinCatalog, String> {
    match LOADED_BUILTINS.get_or_init(load_catalog) {
        Ok(catalog) => Ok(catalog),
        Err(error) => Err(error.clone()),
    }
}

fn load_catalog() -> Result<LoadedBuiltinCatalog, String> {
    let mut seen_builtin_ids = HashSet::new();
    let mut builtins = Vec::new();

    for asset in BUILTIN_ASSETS {
        let parsed = parse_builtin(asset)?;
        if !seen_builtin_ids.insert(parsed.manifest.id.clone()) {
            return Err(format!("Duplicate builtin id {}", parsed.manifest.id));
        }
        builtins.push(parsed);
    }

    let runtime_languages = merge_languages(&builtins)?;
    validate_page_overlays(&runtime_languages)?;
    let server_definitions = build_server_definitions(&builtins, &runtime_languages)?;
    let settings_pages = build_settings_pages(&runtime_languages, &server_definitions);
    let server_definitions_by_id = server_definitions
        .iter()
        .cloned()
        .map(|definition| (definition.server_definition_id.clone(), definition))
        .collect();

    Ok(LoadedBuiltinCatalog {
        catalog: BuiltinCatalog {
            runtime: BuiltinRuntimeCatalog {
                languages: runtime_languages,
            },
            settings: BuiltinSettingsCatalog {
                pages: settings_pages,
            },
        },
        server_definitions,
        server_definitions_by_id,
    })
}

fn parse_builtin(asset: &BuiltinAsset) -> Result<LoadedBuiltinManifest, String> {
    let mut manifest = from_str::<BuiltinManifest>(asset.manifest)
        .map_err(|error| format!("Failed to parse {} builtin manifest: {}", asset.id, error))?;

    if manifest.id.trim().is_empty() || manifest.label.trim().is_empty() {
        return Err(format!(
            "Builtin {} must have a non-empty id and label",
            asset.id
        ));
    }

    if manifest.id != asset.id {
        return Err(format!(
            "Builtin manifest id {} does not match embedded asset id {}",
            manifest.id, asset.id
        ));
    }

    normalize_manifest(&mut manifest);
    validate_manifest(&manifest)?;
    inline_settings_schemas(&mut manifest, asset.resources)?;

    Ok(LoadedBuiltinManifest { manifest })
}

fn normalize_manifest(manifest: &mut BuiltinManifest) {
    for language in &mut manifest.contributes.languages {
        language.id = normalize_language_id(&language.id);
        language.aliases = normalize_segments(&language.aliases);
        language.extensions = normalize_segments(&language.extensions);
        language.filenames = normalize_segments(&language.filenames);
    }

    for server in &mut manifest.contributes.lsp_servers {
        server.applies_to_languages = server
            .applies_to_languages
            .iter()
            .map(|value| normalize_language_id(value))
            .filter(|value| !value.is_empty())
            .collect();
        for selector in &mut server.document_selectors {
            if let Some(language) = selector.language.as_ref() {
                let normalized = normalize_language_id(language);
                selector.language = (!normalized.is_empty()).then_some(normalized);
            }
            selector.extensions = normalize_segments(&selector.extensions);
            selector.filenames = normalize_segments(&selector.filenames);
        }
    }
}

fn validate_manifest(manifest: &BuiltinManifest) -> Result<(), String> {
    let mut seen_language_ids = HashSet::new();
    for language in &manifest.contributes.languages {
        if language.id.trim().is_empty() || language.label.trim().is_empty() {
            return Err(format!(
                "Builtin {} has a language with an empty id or label",
                manifest.id
            ));
        }
        if !seen_language_ids.insert(language.id.clone()) {
            return Err(format!(
                "Builtin {} declares language {} more than once",
                manifest.id, language.id
            ));
        }
    }

    let mut seen_server_ids = HashSet::new();
    for server in &manifest.contributes.lsp_servers {
        if server.id.trim().is_empty() || server.label.trim().is_empty() {
            return Err(format!(
                "Builtin {} has an LSP server with an empty id or label",
                manifest.id
            ));
        }
        if !seen_server_ids.insert(server.id.clone()) {
            return Err(format!(
                "Builtin {} declares LSP server {} more than once",
                manifest.id, server.id
            ));
        }
        if server.runtime.command.trim().is_empty() {
            return Err(format!(
                "Builtin {} server {} must define a host command",
                manifest.id, server.id
            ));
        }
        if server.applies_to_languages.is_empty() {
            return Err(format!(
                "Builtin {} server {} must declare applies_to_languages",
                manifest.id, server.id
            ));
        }
        if server.document_selectors.is_empty() {
            return Err(format!(
                "Builtin {} server {} must define document selectors",
                manifest.id, server.id
            ));
        }

        if let Some(settings) = server.settings.as_ref() {
            if settings.schema.is_none() && settings.schema_path.is_none() {
                return Err(format!(
                    "Builtin {} server {} settings must define schema or schema_path",
                    manifest.id, server.id
                ));
            }
        }
    }

    Ok(())
}

fn inline_settings_schemas(
    manifest: &mut BuiltinManifest,
    resources: &[(&'static str, &'static str)],
) -> Result<(), String> {
    let manifest_id = manifest.id.clone();
    for server in &mut manifest.contributes.lsp_servers {
        let Some(settings) = server.settings.as_mut() else {
            continue;
        };

        if settings.schema.is_some() {
            settings.schema_path = None;
            continue;
        }

        let Some(relative) = settings.schema_path.take() else {
            continue;
        };

        let raw = load_builtin_resource(resources, &relative).ok_or_else(|| {
            format!(
                "Builtin {} server {} references missing schema {}",
                manifest_id, server.id, relative
            )
        })?;

        let schema = serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| {
            format!(
                "Builtin {} server {} has invalid schema {}: {}",
                manifest_id, server.id, relative, error
            )
        })?;
        settings.schema = Some(schema);
    }

    Ok(())
}

fn load_builtin_resource(
    resources: &[(&'static str, &'static str)],
    relative: &str,
) -> Option<String> {
    let normalized = normalize_resource_key(relative);
    resources
        .iter()
        .find(|(name, _)| normalize_resource_key(name) == normalized)
        .map(|(_, contents)| (*contents).to_string())
}

fn normalize_resource_key(value: &str) -> &str {
    value.strip_prefix("./").unwrap_or(value)
}

fn merge_languages(
    builtins: &[LoadedBuiltinManifest],
) -> Result<Vec<BuiltinLanguageContribution>, String> {
    let mut merged = BTreeMap::<String, BuiltinLanguageContribution>::new();

    for builtin in builtins {
        for language in &builtin.manifest.contributes.languages {
            if let Some(existing) = merged.get_mut(&language.id) {
                if existing.label != language.label {
                    return Err(format!(
                        "Language {} is declared with conflicting labels: {} vs {}",
                        language.id, existing.label, language.label
                    ));
                }

                append_unique(&mut existing.aliases, &language.aliases);
                append_unique(&mut existing.extensions, &language.extensions);
                append_unique(&mut existing.filenames, &language.filenames);
            } else {
                merged.insert(language.id.clone(), language.clone());
            }
        }
    }

    let mut languages = merged.into_values().collect::<Vec<_>>();
    languages.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
    Ok(languages)
}

fn validate_page_overlays(languages: &[BuiltinLanguageContribution]) -> Result<(), String> {
    let language_ids = languages
        .iter()
        .map(|language| language.id.as_str())
        .collect::<HashSet<_>>();

    for overlay in PAGE_OVERLAYS {
        for language_id in overlay.language_ids {
            if !language_ids.contains(language_id) {
                return Err(format!(
                    "Settings page {} references unknown language {}",
                    overlay.id, language_id
                ));
            }
        }
    }

    Ok(())
}

fn build_server_definitions(
    builtins: &[LoadedBuiltinManifest],
    languages: &[BuiltinLanguageContribution],
) -> Result<Vec<LoadedBuiltinLspServerDefinition>, String> {
    let known_language_ids = languages
        .iter()
        .map(|language| language.id.as_str())
        .collect::<HashSet<_>>();
    let mut seen_server_definition_ids = HashSet::new();
    let mut definitions = Vec::new();

    for builtin in builtins {
        for definition in &builtin.manifest.contributes.lsp_servers {
            for language_id in &definition.applies_to_languages {
                if !known_language_ids.contains(language_id.as_str()) {
                    return Err(format!(
                        "Builtin {} server {} targets unknown language {}",
                        builtin.manifest.id, definition.id, language_id
                    ));
                }
            }

            validate_document_selectors(
                &builtin.manifest.id,
                &definition.id,
                &definition.document_selectors,
                &known_language_ids,
            )?;

            let server_definition_id =
                BuiltinCatalogService::server_definition_id(&builtin.manifest.id, &definition.id);
            if !seen_server_definition_ids.insert(server_definition_id.clone()) {
                return Err(format!(
                    "Duplicate server definition id {}",
                    server_definition_id
                ));
            }

            definitions.push(LoadedBuiltinLspServerDefinition {
                server_definition_id,
                builtin_id: builtin.manifest.id.clone(),
                builtin_label: builtin.manifest.label.clone(),
                definition: definition.clone(),
            });
        }
    }

    Ok(definitions)
}

fn validate_document_selectors(
    builtin_id: &str,
    server_id: &str,
    selectors: &[BuiltinDocumentSelector],
    known_language_ids: &HashSet<&str>,
) -> Result<(), String> {
    for selector in selectors {
        if let Some(language_id) = selector.language.as_deref() {
            if !known_language_ids.contains(language_id) {
                return Err(format!(
                    "Builtin {} server {} selector references unknown language {}",
                    builtin_id, server_id, language_id
                ));
            }
        }
    }

    Ok(())
}

fn build_settings_pages(
    languages: &[BuiltinLanguageContribution],
    server_definitions: &[LoadedBuiltinLspServerDefinition],
) -> Vec<BuiltinSettingsPage> {
    let mut pages = Vec::new();
    let mut claimed_language_ids = HashSet::new();

    for overlay in PAGE_OVERLAYS {
        let language_ids = overlay
            .language_ids
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        claimed_language_ids.extend(language_ids.iter().cloned());

        pages.push(BuiltinSettingsPage {
            id: overlay.id.to_string(),
            kind: overlay.kind,
            label: overlay.label.to_string(),
            icon_filename: overlay.icon_filename.to_string(),
            server_definition_ids: server_definition_ids_for_languages(
                server_definitions,
                &language_ids,
            ),
            language_ids,
            formatter: overlay.formatter_group.map(formatter_policy),
        });
    }

    let mut auto_languages = languages
        .iter()
        .filter(|language| !claimed_language_ids.contains(language.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    auto_languages.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));

    for language in auto_languages {
        let language_ids = vec![language.id.clone()];
        pages.push(BuiltinSettingsPage {
            id: format!("language:{}", language.id),
            kind: BuiltinSettingsPageKind::Language,
            label: language.label.clone(),
            icon_filename: icon_filename_for_language(&language),
            server_definition_ids: server_definition_ids_for_languages(
                server_definitions,
                &language_ids,
            ),
            language_ids,
            formatter: None,
        });
    }

    pages
}

fn server_definition_ids_for_languages(
    server_definitions: &[LoadedBuiltinLspServerDefinition],
    language_ids: &[String],
) -> Vec<String> {
    let page_languages = language_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();

    server_definitions
        .iter()
        .filter(|server| {
            server
                .definition
                .applies_to_languages
                .iter()
                .any(|language_id| page_languages.contains(language_id.as_str()))
        })
        .map(|server| server.server_definition_id.clone())
        .collect()
}

fn formatter_policy(group: BuiltinFormatterGroupId) -> BuiltinFormatterPolicy {
    match group {
        BuiltinFormatterGroupId::JavascriptTypescript | BuiltinFormatterGroupId::Json => {
            BuiltinFormatterPolicy {
                group,
                options: vec![
                    FormatterSelection::Biome,
                    FormatterSelection::Lsp,
                    FormatterSelection::Disabled,
                ],
                default: FormatterSelection::Biome,
            }
        }
        BuiltinFormatterGroupId::Nix => BuiltinFormatterPolicy {
            group,
            options: vec![
                FormatterSelection::Nixfmt,
                FormatterSelection::Lsp,
                FormatterSelection::Disabled,
            ],
            default: FormatterSelection::Nixfmt,
        },
    }
}

fn append_unique(target: &mut Vec<String>, incoming: &[String]) {
    let mut seen = target.iter().cloned().collect::<HashSet<_>>();
    for value in incoming {
        if seen.insert(value.clone()) {
            target.push(value.clone());
        }
    }
}

fn icon_filename_for_language(language: &BuiltinLanguageContribution) -> String {
    if let Some(filename) = language.filenames.first() {
        return filename.clone();
    }

    if let Some(extension) = language.extensions.first() {
        return format!("index{}", extension);
    }

    format!("{}.txt", language.id)
}

fn normalize_segments(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| normalize_segment(value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_segment(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('.') {
        return trimmed.to_ascii_lowercase();
    }
    if trimmed.contains('/') {
        return trimmed.to_string();
    }
    trimmed.to_ascii_lowercase()
}

fn normalize_language_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_manifests_parse() {
        for builtin in BUILTIN_ASSETS {
            let parsed = parse_builtin(builtin)
                .unwrap_or_else(|error| panic!("failed to parse {}: {}", builtin.id, error));
            assert_eq!(parsed.manifest.id, builtin.id);
        }
    }

    #[test]
    fn builtin_schemas_inline_on_parse() {
        for builtin in BUILTIN_ASSETS {
            let parsed = parse_builtin(builtin)
                .unwrap_or_else(|error| panic!("failed to parse {}: {}", builtin.id, error));
            for server in &parsed.manifest.contributes.lsp_servers {
                let Some(settings) = server.settings.as_ref() else {
                    continue;
                };
                assert!(
                    settings.schema_path.is_none(),
                    "schema_path should be consumed for {}::{}",
                    builtin.id,
                    server.id
                );
                assert!(
                    settings.schema.is_some(),
                    "schema should be inlined for {}::{}",
                    builtin.id,
                    server.id
                );
            }
        }
    }

    #[test]
    fn rejects_missing_applies_to_languages() {
        let raw = r#"
        {
          "id": "dev.sworm.test",
          "label": "Test",
          "contributes": {
            "languages": [
              {
                "id": "test",
                "label": "Test",
                "extensions": [".test"]
              }
            ],
            "lsp_servers": [
              {
                "id": "server",
                "label": "Server",
                "runtime": {
                  "kind": "host_binary",
                  "command": "test-server"
                },
                "install_hint": "n/a",
                "document_selectors": [
                  {
                    "language": "test",
                    "extensions": [".test"]
                  }
                ]
              }
            ]
          }
        }
        "#;

        let mut manifest = serde_json::from_str::<BuiltinManifest>(raw).unwrap();
        normalize_manifest(&mut manifest);
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn builds_expected_settings_pages() {
        let catalog = load_catalog().expect("catalog should load");
        let page_ids = catalog
            .catalog
            .settings
            .pages
            .iter()
            .map(|page| page.id.as_str())
            .collect::<Vec<_>>();

        assert!(page_ids.contains(&"language:javascript-typescript"));
        assert!(page_ids.contains(&"language:json"));
        assert!(page_ids.contains(&"language:css"));
        assert!(page_ids.contains(&"language:svelte"));
        assert!(page_ids.contains(&"language:html"));
        assert!(page_ids.contains(&"language:nix"));
    }
}
