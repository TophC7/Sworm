use crate::models::lsp::{
    LspExtensionEntry, LspExtensionManifest, LspRuntimeKind, LspServerCatalogEntry,
    LspServerDefinition,
};
use serde_json::from_str;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tracing::warn;

const BUILTIN_SVELTE_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.svelte/extension.json");
const BUILTIN_TAILWIND_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.tailwind/extension.json");
const BUILTIN_VTSLS_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.vtsls/extension.json");
const BUILTIN_HTML_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.html/extension.json");
const BUILTIN_CSS_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.css/extension.json");
const BUILTIN_NIX_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.nix/extension.json");
const BUILTIN_BIOME_EXTENSION: &str =
    include_str!("../../lsp/extensions/dev.sworm.biome/extension.json");

#[derive(Debug, Clone)]
pub struct LoadedLspExtension {
    pub manifest: LspExtensionManifest,
    pub built_in: bool,
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LoadedLspServerDefinition {
    pub extension: LoadedLspExtension,
    pub definition: LspServerDefinition,
}

pub struct LspCatalog;

impl LspCatalog {
    pub fn server_definition_id(extension_id: &str, server_id: &str) -> String {
        format!("{}::{}", extension_id, server_id)
    }

    pub fn load(app_handle: &tauri::AppHandle) -> Vec<LoadedLspExtension> {
        let mut seen_ids = HashSet::new();
        let mut entries = Vec::new();

        for builtin in [
            Self::parse_builtin(BUILTIN_SVELTE_EXTENSION),
            Self::parse_builtin(BUILTIN_TAILWIND_EXTENSION),
            Self::parse_builtin(BUILTIN_VTSLS_EXTENSION),
            Self::parse_builtin(BUILTIN_HTML_EXTENSION),
            Self::parse_builtin(BUILTIN_CSS_EXTENSION),
            Self::parse_builtin(BUILTIN_NIX_EXTENSION),
            Self::parse_builtin(BUILTIN_BIOME_EXTENSION),
        ]
        .into_iter()
        .flatten()
        {
            if seen_ids.insert(builtin.manifest.id.clone()) {
                entries.push(builtin);
            } else {
                warn!("Skipping duplicate built-in LSP extension {}", builtin.manifest.id);
            }
        }

        let user_root = match app_handle.path().app_data_dir() {
            Ok(path) => path.join("lsp").join("extensions"),
            Err(error) => {
                warn!("Failed to resolve app data dir for LSP extensions: {}", error);
                return entries;
            }
        };

        if !user_root.exists() {
            return entries;
        }

        let Ok(dirs) = std::fs::read_dir(&user_root) else {
            warn!("Failed to read LSP extension directory {:?}", user_root);
            return entries;
        };

        for dir in dirs.flatten() {
            let path = dir.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("extension.json");
            let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };

            match Self::parse_manifest(&raw, false, Some(path.clone())) {
                Some(extension) if seen_ids.insert(extension.manifest.id.clone()) => entries.push(extension),
                Some(extension) => warn!(
                    "Skipping duplicate user LSP extension {} from {:?}",
                    extension.manifest.id, manifest_path
                ),
                None => warn!("Skipping invalid LSP extension manifest at {:?}", manifest_path),
            }
        }

        entries
    }

    pub fn list(app_handle: &tauri::AppHandle) -> Vec<LspExtensionEntry> {
        Self::load(app_handle)
            .into_iter()
            .map(|entry| LspExtensionEntry {
                id: entry.manifest.id.clone(),
                label: entry.manifest.label.clone(),
                built_in: entry.built_in,
                source_path: entry
                    .source_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                languages: entry.manifest.languages.clone(),
                servers: entry
                    .manifest
                    .servers
                    .iter()
                    .map(|server| LspServerCatalogEntry {
                        server_definition_id: Self::server_definition_id(
                            &entry.manifest.id,
                            &server.id,
                        ),
                        extension_id: entry.manifest.id.clone(),
                        extension_label: entry.manifest.label.clone(),
                        label: server.label.clone(),
                        install_hint: server.install_hint.clone(),
                        runtime_kind: server.runtime.kind,
                        command: server.runtime.command.clone(),
                        runtime_command: server.runtime.runtime_command.clone(),
                        entry_path: server.runtime.entry_path.clone(),
                        args: server.args.clone(),
                        document_selectors: server.document_selectors.clone(),
                        initialization_options: server.initialization_options.clone(),
                        built_in: entry.built_in,
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn find_server_definition(
        app_handle: &tauri::AppHandle,
        server_definition_id: &str,
    ) -> Option<LoadedLspServerDefinition> {
        Self::load(app_handle).into_iter().find_map(|extension| {
            extension
                .manifest
                .servers
                .iter()
                .find(|server| {
                    Self::server_definition_id(&extension.manifest.id, &server.id) == server_definition_id
                })
                .cloned()
                .map(|definition| LoadedLspServerDefinition {
                    extension,
                    definition,
                })
        })
    }

    fn parse_builtin(raw: &str) -> Option<LoadedLspExtension> {
        Self::parse_manifest(raw, true, None)
    }

    fn parse_manifest(
        raw: &str,
        built_in: bool,
        source_path: Option<PathBuf>,
    ) -> Option<LoadedLspExtension> {
        let manifest = from_str::<LspExtensionManifest>(raw).ok()?;
        if manifest.id.trim().is_empty() || manifest.label.trim().is_empty() {
            return None;
        }

        if manifest
            .servers
            .iter()
            .any(|server| server.id.trim().is_empty() || server.label.trim().is_empty())
        {
            return None;
        }

        if !validate_manifest(&manifest) {
            return None;
        }

        Some(LoadedLspExtension {
            manifest: normalize_manifest(manifest),
            built_in,
            source_path,
        })
    }
}

fn normalize_manifest(mut manifest: LspExtensionManifest) -> LspExtensionManifest {
    for language in &mut manifest.languages {
        language.extensions = normalize_segments(&language.extensions);
        language.filenames = normalize_segments(&language.filenames);
    }

    for server in &mut manifest.servers {
        for selector in &mut server.document_selectors {
            selector.extensions = normalize_segments(&selector.extensions);
            selector.filenames = normalize_segments(&selector.filenames);
        }
    }

    manifest
}

fn validate_manifest(manifest: &LspExtensionManifest) -> bool {
    for server in &manifest.servers {
        if matches!(
            server.runtime.kind,
            LspRuntimeKind::BundledBinary | LspRuntimeKind::BundledJs
        ) {
            warn!(
                "Rejecting LSP extension {} because server {} uses unsupported runtime kind {:?}",
                manifest.id, server.id, server.runtime.kind
            );
            return false;
        }
    }

    true
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

pub fn resolve_extension_relative_path(root: Option<&Path>, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.to_path_buf();
    }

    root.unwrap_or_else(|| Path::new(".")).join(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_manifests_parse() {
        let svelte = LspCatalog::parse_builtin(BUILTIN_SVELTE_EXTENSION);
        let tailwind = LspCatalog::parse_builtin(BUILTIN_TAILWIND_EXTENSION);
        let vtsls = LspCatalog::parse_builtin(BUILTIN_VTSLS_EXTENSION);
        let html = LspCatalog::parse_builtin(BUILTIN_HTML_EXTENSION);
        let css = LspCatalog::parse_builtin(BUILTIN_CSS_EXTENSION);
        let nix = LspCatalog::parse_builtin(BUILTIN_NIX_EXTENSION);
        let biome = LspCatalog::parse_builtin(BUILTIN_BIOME_EXTENSION);

        assert!(svelte.is_some());
        assert!(tailwind.is_some());
        assert!(vtsls.is_some());
        assert!(html.is_some());
        assert!(css.is_some());
        assert!(nix.is_some());
        assert!(biome.is_some());
        assert_eq!(
            svelte.unwrap().manifest.id,
            "dev.sworm.svelte".to_string()
        );
        assert_eq!(
            tailwind.unwrap().manifest.id,
            "dev.sworm.tailwind".to_string()
        );
        assert_eq!(vtsls.unwrap().manifest.id, "dev.sworm.vtsls".to_string());
        assert_eq!(html.unwrap().manifest.id, "dev.sworm.html".to_string());
        assert_eq!(css.unwrap().manifest.id, "dev.sworm.css".to_string());
        assert_eq!(nix.unwrap().manifest.id, "dev.sworm.nix".to_string());
        assert_eq!(biome.unwrap().manifest.id, "dev.sworm.biome".to_string());
    }

    #[test]
    fn resolves_relative_extension_paths_against_root() {
        let root = Path::new("/tmp/example-extension");
        let path = resolve_extension_relative_path(Some(root), "bin/server");
        assert_eq!(path, PathBuf::from("/tmp/example-extension/bin/server"));
    }

    #[test]
    fn rejects_unsupported_bundled_runtime_manifests() {
        let raw = r#"
        {
          "id": "dev.sworm.test-bundled",
          "label": "Bundled Test",
          "servers": [
            {
              "id": "bundled-server",
              "label": "Bundled Server",
              "runtime": {
                "kind": "bundled_js",
                "command": "bun",
                "entry_path": "server.js"
              },
              "install_hint": "n/a"
            }
          ]
        }
        "#;

        assert!(LspCatalog::parse_manifest(raw, false, None).is_none());
    }
}
