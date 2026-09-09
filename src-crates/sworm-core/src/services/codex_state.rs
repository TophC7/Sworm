use crate::services::folders::home_dir;
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

pub struct CodexStateReader;

impl CodexStateReader {
    fn db_path() -> Option<PathBuf> {
        let path = home_dir()?.join(".codex/state_5.sqlite");
        path.exists().then_some(path)
    }

    fn open() -> Result<Connection, String> {
        let path = Self::db_path().ok_or_else(|| "Codex state database not found".to_string())?;
        Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| format!("Failed to open Codex state: {}", error))
    }

    /// Visit unarchived threads in `cwd` created at or after `since_unix`
    /// (seconds; `threads.created_at` is second-granular), oldest first.
    /// Returning `false` from `visit` stops the SQLite row stream.
    pub fn visit_threads_created_since(
        cwd: &str,
        since_unix: i64,
        mut visit: impl FnMut(String, i64) -> bool,
    ) -> Result<(), String> {
        let conn = Self::open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at
                 FROM threads
                 WHERE cwd = ?1
                   AND archived = 0
                   AND created_at >= ?2
                 ORDER BY created_at ASC",
            )
            .map_err(|error| format!("Failed to query Codex threads: {}", error))?;

        let rows = stmt
            .query_map(rusqlite::params![cwd, since_unix], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| format!("Failed to map Codex threads: {}", error))?;

        for row in rows {
            let (id, created_at) =
                row.map_err(|error| format!("Failed to read Codex thread: {}", error))?;
            if !visit(id, created_at) {
                break;
            }
        }
        Ok(())
    }

    pub fn thread_exists(thread_id: &str, cwd: &str) -> Result<bool, String> {
        let conn = Self::open()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads
                 WHERE id = ?1 AND cwd = ?2 AND archived = 0",
                rusqlite::params![thread_id, cwd],
                |row| row.get(0),
            )
            .map_err(|error| format!("Failed to verify Codex thread: {}", error))?;
        Ok(count > 0)
    }
}
