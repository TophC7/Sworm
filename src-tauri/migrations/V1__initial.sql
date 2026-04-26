-- Sworm canonical schema. No migration chain; the previous V1..V9
-- sequence has been folded into this single file because the app
-- doesn't have any users yet, so backwards-compat is unnecessary
-- weight. Wipe the DB file on next launch and refinery will recreate
-- everything from this one statement.

-- TABLES --
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  default_branch TEXT,
  base_ref TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  provider_id TEXT NOT NULL,
  title TEXT NOT NULL,
  cwd TEXT NOT NULL,
  branch TEXT,
  status TEXT NOT NULL,
  shared_workspace INTEGER NOT NULL DEFAULT 1,
  auto_approve INTEGER NOT NULL DEFAULT 0,
  provider_resume_token TEXT,
  archived INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_started_at TEXT,
  last_stopped_at TEXT
);

CREATE TABLE session_entries (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE provider_configs (
  provider_id TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 1,
  binary_path_override TEXT,
  extra_args_json TEXT,
  env_overrides_json TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE mcp_servers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',
  providers_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE skills (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source TEXT NOT NULL,
  local_path TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  installed INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

CREATE TABLE credentials (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  account TEXT NOT NULL,
  service TEXT NOT NULL,
  keyring_ref TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Per-project Nix environment configuration and cached evaluation results.
CREATE TABLE project_nix_envs (
  project_id    TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  nix_file      TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'pending',
  env_json      TEXT,
  error_message TEXT,
  evaluated_at  TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

-- Per-project workspace layout persistence (tab/pane blob, active tab,
-- splits) so workbench layout survives reloads.
CREATE TABLE workspace_state (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  state_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- App-shell key/value (hot-restore: open project ids, active project,
-- pending-open path, ...). Kept separate from `app_settings` because
-- preferences and hot-restore state are different classes of data.
CREATE TABLE app_state (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE lsp_server_configs (
  server_definition_id TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 1,
  binary_path_override TEXT,
  runtime_path_override TEXT,
  runtime_args_json TEXT NOT NULL DEFAULT '[]',
  extra_args_json TEXT NOT NULL DEFAULT '[]',
  trace TEXT NOT NULL DEFAULT 'off',
  settings_json TEXT,
  updated_at TEXT NOT NULL
);

-- INDEXES --
-- Transcript reads; bounds session_transcript_get to one session's
-- output log instead of full-scanning session_entries.
CREATE INDEX idx_session_entries_session_kind
  ON session_entries(session_id, kind, created_at, id);

-- Per-project session lists; covers the WHERE filter and ORDER BY for
-- session_list_for_project / session_list_archived in a single seek.
CREATE INDEX idx_sessions_project_archived_updated
  ON sessions(project_id, archived, updated_at DESC);
