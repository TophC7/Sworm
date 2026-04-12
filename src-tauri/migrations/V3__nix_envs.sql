-- Per-project Nix environment configuration and cached evaluation results.
CREATE TABLE IF NOT EXISTS project_nix_envs (
  project_id    TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  nix_file      TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'pending',
  env_json      TEXT,
  error_message TEXT,
  evaluated_at  TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
