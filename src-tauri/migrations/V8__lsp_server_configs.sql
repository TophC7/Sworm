CREATE TABLE IF NOT EXISTS lsp_server_configs (
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
