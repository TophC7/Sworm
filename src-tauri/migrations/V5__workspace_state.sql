-- Per-project workspace layout persistence.
-- Holds the serialized tab/pane blob for each open project, so
-- sessions, editor tabs, splits and active-tab selections survive
-- reloads and restarts.

CREATE TABLE workspace_state (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  state_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
