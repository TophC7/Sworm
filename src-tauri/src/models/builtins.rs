use crate::services::settings::FormatterSelection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinManifest {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub contributes: BuiltinContributions,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuiltinContributions {
    #[serde(default)]
    pub languages: Vec<BuiltinLanguageContribution>,
    #[serde(default, rename = "lsp_servers")]
    pub lsp_servers: Vec<BuiltinLspServerDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinLanguageContribution {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinLspServerDefinition {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub applies_to_languages: Vec<String>,
    pub runtime: BuiltinLspRuntimeDefinition,
    #[serde(default)]
    pub args: Vec<String>,
    pub install_hint: String,
    #[serde(default)]
    pub document_selectors: Vec<BuiltinDocumentSelector>,
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default)]
    pub settings: Option<BuiltinLspServerSettingsDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuiltinLspServerSettingsDescriptor {
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub defaults: Option<Value>,
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinLspRuntimeDefinition {
    pub kind: BuiltinRuntimeKind,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinRuntimeKind {
    HostBinary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinDocumentSelector {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinCatalog {
    pub runtime: BuiltinRuntimeCatalog,
    pub settings: BuiltinSettingsCatalog,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinRuntimeCatalog {
    pub languages: Vec<BuiltinLanguageContribution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinSettingsCatalog {
    pub pages: Vec<BuiltinSettingsPage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinSettingsPage {
    pub id: String,
    pub kind: BuiltinSettingsPageKind,
    pub label: String,
    pub icon_filename: String,
    pub language_ids: Vec<String>,
    pub server_definition_ids: Vec<String>,
    pub formatter: Option<BuiltinFormatterPolicy>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinSettingsPageKind {
    Language,
    Nix,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuiltinFormatterPolicy {
    pub group: BuiltinFormatterGroupId,
    pub options: Vec<FormatterSelection>,
    pub default: FormatterSelection,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinFormatterGroupId {
    JavascriptTypescript,
    Json,
    Nix,
}
