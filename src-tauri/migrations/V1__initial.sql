-- Sworm canonical schema. No migration chain; the previous V1..V9
-- sequence has been folded into this single file because the app
-- doesn't have any users yet, so backwards-compat is unnecessary
-- weight. Wipe the DB file on next launch and refinery will recreate
-- everything from this one statement.

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

-- Per-folder Nix environment configuration and cached evaluation results.
CREATE TABLE folder_nix_envs (
  folder_path   TEXT PRIMARY KEY,
  nix_file      TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'pending',
  env_json      TEXT,
  error_message TEXT,
  evaluated_at  TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

-- App-shell key/value for hot-restore data: workbench tabs, recent
-- folders, and other frontend-owned state.
CREATE TABLE app_state (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
