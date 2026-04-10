use chrono::DateTime;
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CodexThread {
    pub id: String,
    pub cwd: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
}

pub struct CodexStateReader;

impl CodexStateReader {
    fn db_path() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let path = PathBuf::from(home).join(".codex/state_5.sqlite");
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

    pub fn find_recent_threads_for_cwd(
        cwd: &str,
        since_rfc3339: &str,
    ) -> Result<Vec<CodexThread>, String> {
        let since = DateTime::parse_from_rfc3339(since_rfc3339)
            .map_err(|error| format!("Invalid Codex since timestamp: {}", error))?
            .timestamp();
        let conn = Self::open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, cwd, created_at, updated_at, archived
                 FROM threads
                 WHERE cwd = ?1
                   AND archived = 0
                   AND (updated_at >= ?2 OR created_at >= ?2)
                 ORDER BY updated_at DESC, created_at DESC",
            )
            .map_err(|error| format!("Failed to query Codex threads: {}", error))?;

        let rows = stmt
            .query_map(rusqlite::params![cwd, since], |row| {
                Ok(CodexThread {
                    id: row.get(0)?,
                    cwd: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    archived: row.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(|error| format!("Failed to map Codex threads: {}", error))?;

        let mut threads = Vec::new();
        for row in rows {
            threads.push(row.map_err(|error| format!("Failed to read Codex thread: {}", error))?);
        }

        Ok(threads)
    }

    pub fn find_latest_thread_for_cwd(cwd: &str) -> Result<Option<CodexThread>, String> {
        let conn = Self::open()?;
        conn.query_row(
            "SELECT id, cwd, created_at, updated_at, archived
             FROM threads
             WHERE cwd = ?1 AND archived = 0
             ORDER BY updated_at DESC, created_at DESC
             LIMIT 1",
            rusqlite::params![cwd],
            |row| {
                Ok(CodexThread {
                    id: row.get(0)?,
                    cwd: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    archived: row.get::<_, i64>(4)? != 0,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed to query Codex latest thread: {}", error))
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

use rusqlite::OptionalExtension;
