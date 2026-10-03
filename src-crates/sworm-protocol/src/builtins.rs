use crate::settings::FormatterSelection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
pub struct BuiltinDocumentSelector {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinCatalog {
    pub runtime: BuiltinRuntimeCatalog,
    pub settings: BuiltinSettingsCatalog,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinRuntimeCatalog {
    pub languages: Vec<BuiltinLanguageContribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinSettingsCatalog {
    pub pages: Vec<BuiltinSettingsPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
