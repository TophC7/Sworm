-- App-shell key/value persistence.
-- Stores hot-restore state (open project ids, active project id, ...)
-- kept separate from `app_settings` because preferences and hot-restore
-- state are different classes of data.

CREATE TABLE app_state (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
