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

export interface ProjectSessionGroup {
  project_id: string
  sessions: Session[]
  archived_sessions: Session[]
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

export type FormatterSelection = 'lsp' | 'biome' | 'nixfmt' | 'disabled'

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

export interface ProviderSettings {
  enabled: boolean
  binary_path_override: string | null
  extra_args: string[]
}

export interface EffectiveLspServerSettings {
  enabled: boolean
  binary_path_override: string | null
  runtime_path_override: string | null
  runtime_args: string[]
  extra_args: string[]
  trace: LspTraceLevel
  settings: unknown | null
}

export interface EffectiveSettings {
  general: GeneralSettings
  formatting: FormattingSettings
  providers: Record<string, ProviderSettings>
  lsp: { servers: Record<string, EffectiveLspServerSettings> }
}

export type SettingsLayerKind = 'global' | 'project'
export type SettingsDiagnosticCode =
  | 'parse_error'
  | 'type_error'
  | 'invalid_enum'
  | 'invalid_null'
  | 'unknown_key'
  | 'unknown_provider'
  | 'unknown_lsp_server'
export type SettingsDiagnosticSeverity = 'warning' | 'error'

export interface SettingsDiagnostic {
  layer: SettingsLayerKind
  path: string
  pointer: string
  code: SettingsDiagnosticCode
  severity: SettingsDiagnosticSeverity
  message: string
}

export interface EffectiveSettingsPayload {
  settings: EffectiveSettings
  diagnostics: SettingsDiagnostic[]
}

export interface SettingsLayerPayload {
  path: string
  loaded: boolean
  value: unknown
  diagnostics: SettingsDiagnostic[]
}

export interface SettingsFileResult {
  path: string
}

export interface ShortcutsFilePayload {
  path: string
  loaded: boolean
  value: unknown
}

export interface ShortcutsFileResult {
  path: string
}

export interface SettingsChangedEvent {
  layer: SettingsLayerKind
  project_id?: string | null
  generation: number
  diagnostics: SettingsDiagnostic[]
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
export type LspServerConnectionStatus = 'connected' | 'missing' | 'error' | 'disabled'
export type LspTransportTraceDirection = 'incoming' | 'outgoing' | 'stderr'
export type BuiltinFormatterGroupId = 'javascript_typescript' | 'json' | 'nix'
export type BuiltinSettingsPageKind = 'language' | 'nix'

export interface BuiltinLanguageContribution {
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

// Optional per-server settings descriptor sourced from the builtin
// manifest. `schema` is an inlined JSON Schema object usable by Monaco
// for autocomplete/validation; `section` hints which LSP workspace
// configuration section the JSON is expected to slot into; `defaults`
// seeds the JSON editor when the user has no settings yet.
export interface LspServerSettingsDescriptor {
  section: string | null
  defaults: unknown
  schema: unknown
}

export interface BuiltinFormatterPolicy {
  group: BuiltinFormatterGroupId
  options: FormatterSelection[]
  default: FormatterSelection
}

export interface BuiltinSettingsPage {
  id: string
  kind: BuiltinSettingsPageKind
  label: string
  icon_filename: string
  language_ids: string[]
  server_definition_ids: string[]
  formatter: BuiltinFormatterPolicy | null
}

export interface BuiltinRuntimeCatalog {
  languages: BuiltinLanguageContribution[]
}

export interface BuiltinSettingsCatalog {
  pages: BuiltinSettingsPage[]
}

export interface BuiltinCatalog {
  runtime: BuiltinRuntimeCatalog
  settings: BuiltinSettingsCatalog
}

export interface LspServerConfig {
  server_definition_id: string
  enabled: boolean
  binary_path_override: string | null
  runtime_path_override: string | null
  runtime_args: string[]
  extra_args: string[]
  trace: LspTraceLevel
  settings: unknown | null
}

export interface LspServerStatus {
  server_definition_id: string
  builtin_id: string
  builtin_label: string
  label: string
  enabled: boolean
  status: LspServerConnectionStatus
  resolved_path: string | null
  runtime_resolved_path: string | null
  message: string | null
  install_hint: string
  document_selectors: LspDocumentSelector[]
  initialization_options: unknown
  settings: LspServerSettingsDescriptor | null
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

export interface GitQuickDiffData {
  indexContent: string | null
  headContent: string | null
  hasIndexChanges: boolean
}

// DIFF PAYLOAD //
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

// Per-file content payload for the lazy working-tree diff path.
// Mirrors `DiffFileContent` in `src-tauri/src/commands/git.rs`.
export interface DiffFileContent {
  oldContent: string | null
  newContent: string | null
  binary: boolean
}

export interface FilePasteCollision {
  source: string
  destination: string
}

// JSON Schema for a project-scoped config file (e.g. .sworm/tasks.json).
// Schemas are derived from Rust types via schemars so they can't drift
// from the deserialization contract.
export interface ConfigSchemaEntry {
  id: string
  fileMatch: string[]
  schema: unknown
}

// Mirror of Rust `TaskDefinition` (src-tauri/src/models/task.rs).
// camelCase here matches the `#[serde(rename_all = "camelCase")]` on
// the source type.
export interface TaskDefinition {
  id: string
  label: string
  command: string
  cwd?: string
  env?: Record<string, string>
  icon?: string
  group?: string
  singleton?: boolean
  clearOnRerun?: boolean
  confirm?: boolean
}

export type IssueStatus = 'todo' | 'in_progress' | 'blocked' | 'completed' | 'wont_fix' | 'archived'
export type IssueEpicStatus = 'todo' | 'in_progress' | 'completed' | 'archived'
export type IssueAssigneeKind = 'human' | 'agent' | 'session' | 'unassigned'

export interface IssueEpic {
  id: string
  title: string
  description: string | null
  status: IssueEpicStatus
  priority: number
  createdBy: string
  updatedBy: string
  createdAt: string
  updatedAt: string
}

export interface Issue {
  id: string
  epicId: string | null
  parentIssueId: string | null
  title: string
  description: string | null
  status: IssueStatus
  priority: number
  assigneeKind: IssueAssigneeKind
  assigneeId: string | null
  createdBy: string
  updatedBy: string
  tags: string[]
  contextJson: string | null
  createdAt: string
  updatedAt: string
}

export interface IssueComment {
  id: string
  issueId: string
  author: string
  body: string
  createdBy: string
  updatedBy: string
  createdAt: string
  updatedAt: string
}

export interface IssueDependency {
  id: string
  issueId: string
  dependsOnIssueId: string
  createdBy: string
  createdAt: string
}

export interface IssueEvent {
  id: number
  actor: string
  action: string
  entityType: string
  entityId: string
  snapshotJson: string | null
  changesJson: string | null
  createdAt: string
}

export interface IssueDetail {
  issue: Issue
  comments: IssueComment[]
  dependsOn: IssueDependency[]
  blockedBy: IssueDependency[]
  subIssues: Issue[]
  events: IssueEvent[]
}

export interface IssueListFilters {
  status?: IssueStatus
  epicId?: string
  includeArchived?: boolean
  limit?: number
}

export interface IssueReadyFilters {
  epicId?: string
  limit?: number
}

export interface IssueSearchFilters {
  status?: IssueStatus
  epicId?: string
  includeArchived?: boolean
  limit?: number
}

export interface IssueCreateInput {
  title: string
  description?: string | null
  status?: IssueStatus
  priority?: number
  epicId?: string | null
  parentIssueId?: string | null
  assigneeKind?: IssueAssigneeKind
  assigneeId?: string | null
  tags?: string[]
  contextJson?: string | null
  actor?: string
}

export interface IssueUpdateInput {
  title?: string
  description?: string | null
  status?: IssueStatus
  priority?: number
  epicId?: string | null
  assigneeKind?: IssueAssigneeKind
  assigneeId?: string | null
  tags?: string[]
  contextJson?: string | null
  actor?: string
}

export interface IssueEpicCreateInput {
  title: string
  description?: string | null
  status?: IssueEpicStatus
  priority?: number
  actor?: string
}

export interface IssueEpicUpdateInput {
  title?: string
  description?: string | null
  status?: IssueEpicStatus
  priority?: number
  actor?: string
}

export interface IssueCommentCreateInput {
  issueId: string
  author: string
  body: string
  actor?: string
}

export interface IssueCommentUpdateInput {
  body: string
  actor?: string
}

export interface IssueDependencyInput {
  issueId: string
  dependsOnIssueId: string
  actor?: string
}

export interface IssueConfigEntry {
  key: string
  value: string
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

// BRANCHES //
//
// Mirror of `src-tauri/src/models/branch.rs`. The Rust types use
// `#[serde(rename_all = "camelCase")]` so JSON keys match the TS
// shape directly.

export type BranchKind = 'local' | 'remote'

export type BranchOpState = 'idle' | 'rebasing' | 'merging'

export interface BranchCommitRef {
  hash: string
  shortHash: string
  subject: string
  author: string
  /** ISO-8601 author date. */
  date: string
}

export interface BranchSummary {
  name: string
  kind: BranchKind
  /** True for the currently checked-out HEAD (one local row at most). */
  isCurrent: boolean
  upstream: string | null
  ahead: number
  behind: number
  tip: BranchCommitRef
}

// ACTIVITY MAP //

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

// NIX ENVIRONMENT //

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
