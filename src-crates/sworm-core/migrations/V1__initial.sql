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

-- Transcripts of window runs whose process exited. A daemon outlives the
-- desktop, so a run can finish while nobody is attached; its output and exit
-- code wait here instead of in memory.
CREATE TABLE completed_runs (
  run_id       TEXT PRIMARY KEY,
  exit_code    INTEGER,
  output_start INTEGER NOT NULL,
  output       BLOB NOT NULL,
  events_json  TEXT NOT NULL,
  finished_at  TEXT NOT NULL
);

CREATE INDEX completed_runs_finished_at ON completed_runs (finished_at);
