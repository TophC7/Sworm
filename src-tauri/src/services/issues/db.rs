//! Per-project SQLite handle and connection plumbing for the issue store.

use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) const ISSUE_DB_REL: &str = ".sworm/issues.db";
pub(super) const READ_POOL_SIZE: usize = 4;

pub(super) const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS issue_config (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_id_counters (
  entity_type TEXT PRIMARY KEY,
  counter INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS issue_epics (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL,
  priority INTEGER NOT NULL,
  created_by TEXT NOT NULL,
  updated_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_items (
  id TEXT PRIMARY KEY,
  epic_id TEXT NOT NULL REFERENCES issue_epics(id) ON DELETE RESTRICT,
  parent_issue_id TEXT REFERENCES issue_items(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL,
  priority INTEGER NOT NULL,
  assignee_kind TEXT NOT NULL,
  assignee_id TEXT,
  created_by TEXT NOT NULL,
  updated_by TEXT NOT NULL,
  tags_json TEXT NOT NULL DEFAULT '[]',
  context_json TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_comments (
  id TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL REFERENCES issue_items(id) ON DELETE CASCADE,
  author TEXT NOT NULL,
  body TEXT NOT NULL,
  created_by TEXT NOT NULL,
  updated_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_dependencies (
  id TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL REFERENCES issue_items(id) ON DELETE CASCADE,
  depends_on_issue_id TEXT NOT NULL REFERENCES issue_items(id) ON DELETE CASCADE,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(issue_id, depends_on_issue_id)
);

CREATE TABLE IF NOT EXISTS issue_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  actor TEXT NOT NULL,
  action TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  snapshot_json TEXT,
  changes_json TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_issue_items_status_updated ON issue_items(status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_issue_items_ready ON issue_items(status, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_issue_items_epic ON issue_items(epic_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_issue_items_parent ON issue_items(parent_issue_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_issue_dependencies_issue ON issue_dependencies(issue_id);
CREATE INDEX IF NOT EXISTS idx_issue_dependencies_depends ON issue_dependencies(depends_on_issue_id);
CREATE INDEX IF NOT EXISTS idx_issue_comments_issue ON issue_comments(issue_id, created_at);
CREATE INDEX IF NOT EXISTS idx_issue_events_entity ON issue_events(entity_type, entity_id, created_at DESC);

INSERT OR IGNORE INTO issue_config(key, value) VALUES ('issue_prefix', 'ISSUE');
INSERT OR IGNORE INTO issue_config(key, value) VALUES ('epic_prefix', 'EPIC');
INSERT OR IGNORE INTO issue_config(key, value) VALUES ('comment_prefix', 'NOTE');
INSERT OR IGNORE INTO issue_id_counters(entity_type, counter) VALUES ('issue', 0);
INSERT OR IGNORE INTO issue_id_counters(entity_type, counter) VALUES ('epic', 0);
INSERT OR IGNORE INTO issue_id_counters(entity_type, counter) VALUES ('comment', 0);
"#;

/// Per-project SQLite handle. One serialized writer plus a small reader
/// pool against the same WAL file, so concurrent reads don't queue
/// behind an in-flight transaction.
pub(super) struct IssueProjectDb {
    pub(super) writer: Mutex<Connection>,
    pub(super) readers: Vec<Mutex<Connection>>,
    pub(super) read_cursor: AtomicUsize,
}

impl IssueProjectDb {
    pub(super) fn write(&self) -> MutexGuard<'_, Connection> {
        self.writer.lock()
    }

    pub(super) fn read(&self) -> MutexGuard<'_, Connection> {
        let len = self.readers.len();
        let start = self.read_cursor.fetch_add(1, Ordering::Relaxed) % len;
        for offset in 0..len {
            let idx = (start + offset) % len;
            if let Some(guard) = self.readers[idx].try_lock() {
                return guard;
            }
        }
        self.readers[start].lock()
    }
}

pub(super) fn open_issue_connection(db_path: &Path, read_only: bool) -> Result<Connection, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open issue db {}: {}", db_path.display(), e))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )
    .map_err(|e| format!("Failed to configure issue db {}: {}", db_path.display(), e))?;
    if read_only {
        conn.execute_batch("PRAGMA query_only=ON;").map_err(|e| {
            format!(
                "Failed to set issue db reader pragma {}: {}",
                db_path.display(),
                e
            )
        })?;
    }
    Ok(conn)
}
