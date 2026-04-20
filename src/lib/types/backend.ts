// Typed interfaces for the Rust backend IPC responses.
// Keep in sync with models in src-tauri/src/models/ and commands/.

export interface AppInfo {
  name: string
  version: string
}

export interface EnvProbeResult {
  detected_shell: string
  base_path: string
  shell_path: string | null
  merged_path: string
  probe_succeeded: boolean
  gdk_backend: string | null
  webkit_disable_dmabuf_renderer: string | null
  webkit_disable_compositing_mode: string | null
}

export interface PtyEvent {
  type: 'started' | 'exit' | 'error'
  session_id: string
  pid?: number | null
  code?: number | null
  message?: string
}

export interface Project {
  id: string
  name: string
  path: string
  default_branch: string | null
  base_ref: string | null
  created_at: string
  updated_at: string
}

export interface Session {
  id: string
  project_id: string
  provider_id: string
  title: string
  cwd: string
  branch: string | null
  status: SessionStatus
  shared_workspace: boolean
  auto_approve: boolean
  provider_resume_token: string | null
  archived: boolean
  created_at: string
  updated_at: string
  last_started_at: string | null
  last_stopped_at: string | null
}

export type SessionStatus = 'idle' | 'starting' | 'running' | 'stopped' | 'exited' | 'failed'

export type ProviderConnectionStatus = 'connected' | 'missing' | 'error'

export interface ProviderStatus {
  id: string
  label: string
  status: ProviderConnectionStatus
  version: string | null
  resolved_path: string | null
  message: string | null
  install_hint: string
}

export interface GeneralSettings {
  theme: string
  terminal_font_family: string
  terminal_font_size: number
  nix_eval_timeout_secs: number
}

export type FormatterSelection = 'auto' | 'lsp' | 'biome' | 'nixfmt' | 'disabled'

export interface FormattingLanguageSettings {
  formatter: FormatterSelection
}

export interface FormattingSettings {
  javascript_typescript: FormattingLanguageSettings
  json: FormattingLanguageSettings
  nix: FormattingLanguageSettings
}

export interface ProviderConfig {
  provider_id: string
  enabled: boolean
  binary_path_override: string | null
  extra_args: string[]
}

export interface ProviderSettingsEntry {
  provider: ProviderStatus
  config: ProviderConfig
}

export interface SettingsPayload {
  general: GeneralSettings
  formatting: FormattingSettings
  providers: ProviderSettingsEntry[]
}

export type LspTraceLevel = 'off' | 'messages' | 'verbose'
export type LspRuntimeKind = 'host_binary' | 'bundled_binary' | 'extension_binary' | 'bundled_js' | 'extension_js'
export type LspServerConnectionStatus = 'connected' | 'missing' | 'error' | 'disabled'
export type LspTransportTraceDirection = 'incoming' | 'outgoing' | 'stderr'

export interface LspLanguageContribution {
  id: string
  label: string
  aliases: string[]
  extensions: string[]
  filenames: string[]
}

export interface LspDocumentSelector {
  language: string | null
  extensions: string[]
  filenames: string[]
}

export interface LspServerCatalogEntry {
  server_definition_id: string
  extension_id: string
  extension_label: string
  label: string
  install_hint: string
  runtime_kind: LspRuntimeKind
  command: string
  runtime_command: string | null
  entry_path: string | null
  args: string[]
  document_selectors: LspDocumentSelector[]
  initialization_options: unknown
  built_in: boolean
}

export interface LspExtensionEntry {
  id: string
  label: string
  built_in: boolean
  source_path: string | null
  languages: LspLanguageContribution[]
  servers: LspServerCatalogEntry[]
}

export interface LspServerConfig {
  server_definition_id: string
  enabled: boolean
  binary_path_override: string | null
  runtime_path_override: string | null
  runtime_args: string[]
  extra_args: string[]
  trace: LspTraceLevel
  settings_json: string | null
}

export interface LspServerStatus {
  server_definition_id: string
  extension_id: string
  extension_label: string
  label: string
  enabled: boolean
  status: LspServerConnectionStatus
  resolved_path: string | null
  runtime_resolved_path: string | null
  message: string | null
  install_hint: string
  document_selectors: LspDocumentSelector[]
  initialization_options: unknown
}

export interface LspServerSettingsEntry {
  server: LspServerStatus
  config: LspServerConfig
}

export type LspEvent =
  | {
      type: 'started'
      session_id: string
      pid: number | null
      resolved_path: string | null
      runtime_resolved_path: string | null
    }
  | {
      type: 'message'
      session_id: string
      payload_json: string
    }
  | {
      type: 'trace'
      session_id: string
      direction: LspTransportTraceDirection
      payload: string
    }
  | {
      type: 'exit'
      session_id: string
      code: number | null
    }
  | {
      type: 'error'
      session_id: string
      message: string
    }

export interface GitChange {
  path: string
  status: string
  staged: boolean
  additions: number | null
  deletions: number | null
}

export interface GitSummary {
  is_repo: boolean
  branch: string | null
  base_ref: string | null
  ahead: number | null
  behind: number | null
  changes: GitChange[]
  staged_count: number
  unstaged_count: number
  untracked_count: number
}

export interface GraphCommit {
  hash: string
  short_hash: string
  parents: string[]
  author: string
  date: string
  message: string
  refs: string[]
}

export interface CommitDetail {
  hash: string
  short_hash: string
  parents: string[]
  author: string
  date: string
  message: string
  body: string
  files: CommitFileChange[]
}

export interface CommitFileChange {
  path: string
  status: string
  additions: number
  deletions: number
}

// ── Monaco multi-file diff payload ──────────────
//
// Mirrors `src-tauri/src/models/file_diff.rs`. One `FileDiff` per
// changed file; the frontend pairs `oldContent`/`newContent` into two
// Monaco models and hands them to a `DiffEditor`.

export type GitStatusKind =
  | 'added'
  | 'modified'
  | 'deleted'
  | 'renamed'
  | 'copied'
  | 'untracked'
  | 'unmerged'
  | 'unknown'

export type DiffSource =
  | { kind: 'working'; staged: boolean | null }
  | { kind: 'commit'; hash: string }
  | { kind: 'stash'; index: number }

export interface FileDiff {
  path: string
  oldPath: string | null
  status: GitStatusKind
  lang: string
  oldContent: string | null
  newContent: string | null
  binary: boolean
  additions: number | null
  deletions: number | null
}

export interface FilePasteCollision {
  source: string
  destination: string
}

export interface FileEntryStat {
  isDir: boolean
}

export interface StashEntry {
  index: number
  message: string
  date: string
  files: CommitFileChange[]
}

// ── Activity Map ────────────────────────────

export interface DiscoveredProviderActivity {
  provider_id: string
  last_active: string
  daily_counts: [number, number, number, number, number, number, number]
}

export interface DiscoveredProject {
  path: string
  name: string
  path_exists: boolean
  is_sworm_project: boolean
  sworm_project_id: string | null
  last_active: string
  providers: DiscoveredProviderActivity[]
}

// ── Nix Environment ─────────────────────────

export type NixEnvStatus = 'pending' | 'evaluating' | 'ready' | 'error' | 'timeout'

export interface NixDiagnostic {
  message: string
  line: number
  column: number
}

export interface NixEnvRecord {
  project_id: string
  nix_file: string
  status: NixEnvStatus
  error_message: string | null
  evaluated_at: string | null
  created_at: string
  updated_at: string
}

export interface NixDetection {
  project_id: string
  project_path: string
  detected_files: string[]
  selected: NixEnvRecord | null
}
