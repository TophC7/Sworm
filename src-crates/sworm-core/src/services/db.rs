use parking_lot::{Mutex, MutexGuard};
use refinery::embed_migrations;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, warn};

embed_migrations!("migrations");

/// Number of read-only connections in the pool. Four covers concurrent
/// UI fetches (app state + nix env + issue queries + git summary
/// writeback) without exhausting per-process file handles.
const READ_POOL_SIZE: usize = 4;

/// SQLite database service.
///
/// Owns one writer connection and a small pool of reader connections.
/// All connections target the same WAL-mode database file, so multiple
/// reads can run concurrently while the single writer remains
/// serialized. Refinery migrations run on the writer at startup.
///
/// API:
/// - [`Self::lock`] / [`Self::write`]; exclusive writer (use for any
///   `INSERT` / `UPDATE` / `DELETE` and any `SELECT` that must see
///   in-flight writes immediately).
/// - [`Self::read`]; least-busy reader from the pool. Use for
///   read-only queries on hot paths (app state, nix env records) so a
///   long-running write doesn't block them.
pub struct DatabaseService {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    /// Round-robin starting index for the read pool; keeps read
    /// load spread across connections instead of always hammering
    /// the first one.
    read_cursor: AtomicUsize,
}

/// Open the writer connection, apply its PRAGMAs and run every pending
/// migration. Fails when the on-disk schema is incompatible with the
/// embedded migrations (refinery checksum mismatch, missing tables).
fn open_migrated(db_path: &Path) -> Result<Connection, anyhow::Error> {
    let mut writer = Connection::open(db_path)?;
    // WAL is per-database (the journal_mode pragma persists), but
    // we still issue it on every connection so a fresh file gets
    // upgraded on its first open.
    writer.execute_batch("PRAGMA journal_mode=WAL;")?;
    writer.execute_batch("PRAGMA foreign_keys=ON;")?;
    // 5s busy timeout so a concurrent writer (e.g. a second Sworm
    // build sharing the file) waits briefly instead of SQLITE_BUSY.
    writer.execute_batch("PRAGMA busy_timeout=5000;")?;

    info!("Running database migrations from {:?}", db_path);
    migrations::runner().run(&mut writer)?;
    info!("Database migrations complete");
    Ok(writer)
}

impl DatabaseService {
    /// Open (or create) the database at the given path and run all
    /// pending migrations on the writer connection. An incompatible
    /// existing file is deleted and recreated: everything persisted
    /// here (workbench layout, recent folders, Nix env cache) is
    /// rebuildable. Builds the read pool after migrations succeed so
    /// readers don't observe a half-migrated schema.
    pub fn new(db_path: PathBuf) -> Result<Self, anyhow::Error> {
        // Ensure the parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let writer = match open_migrated(&db_path) {
            Ok(conn) => conn,
            Err(error) => {
                warn!(
                    "Database at {} is incompatible ({error}); recreating it",
                    db_path.display()
                );
                for suffix in ["", "-wal", "-shm"] {
                    let _ = std::fs::remove_file(format!("{}{suffix}", db_path.display()));
                }
                open_migrated(&db_path)?
            }
        };

        let mut readers = Vec::with_capacity(READ_POOL_SIZE);
        for _ in 0..READ_POOL_SIZE {
            let r = Connection::open(&db_path)?;
            r.execute_batch("PRAGMA foreign_keys=ON;")?;
            r.execute_batch("PRAGMA busy_timeout=5000;")?;
            // Reader-only hint: prevents accidental writes from a
            // misrouted query and lets SQLite skip a few WAL mode
            // checks on this connection.
            r.execute_batch("PRAGMA query_only=ON;")?;
            readers.push(Mutex::new(r));
        }

        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            read_cursor: AtomicUsize::new(0),
        })
    }

    /// Acquire the writer guard. Holds the writer mutex for the
    /// guard's lifetime; keep the critical section short.
    pub fn write(&self) -> WriteGuard<'_> {
        WriteGuard {
            inner: self.writer.lock(),
        }
    }

    /// Acquire a reader guard. Round-robins through the pool, falling
    /// back to a blocking lock on the chosen connection if all are
    /// busy. Use only for read-only queries.
    pub fn read(&self) -> ReadGuard<'_> {
        let len = self.readers.len();
        let start = self.read_cursor.fetch_add(1, Ordering::Relaxed) % len;
        // Try-lock starting from the round-robin cursor; first
        // available reader wins.
        for offset in 0..len {
            let idx = (start + offset) % len;
            if let Some(g) = self.readers[idx].try_lock() {
                return ReadGuard { inner: g };
            }
        }
        // All contended; block on the cursor's pick.
        ReadGuard {
            inner: self.readers[start].lock(),
        }
    }
}

/// Writer guard. Wraps the writer mutex guard with a `.conn()`
/// accessor; pair every read-only path with [`DatabaseService::read`]
/// instead so a long writer doesn't block UI fetches.
pub struct WriteGuard<'a> {
    inner: MutexGuard<'a, Connection>,
}

impl<'a> WriteGuard<'a> {
    pub fn conn(&self) -> &Connection {
        &self.inner
    }
}

/// Reader guard; same shape as [`WriteGuard`], but the underlying
/// connection has `PRAGMA query_only=ON`. Attempting a write through
/// this guard will fail at the SQLite layer.
pub struct ReadGuard<'a> {
    inner: MutexGuard<'a, Connection>,
}

impl<'a> ReadGuard<'a> {
    pub fn conn(&self) -> &Connection {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::migrations;

    #[test]
    fn v3_timestamps_legacy_recent_folders_in_mru_order() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations::runner()
            .set_target(refinery::Target::Version(2))
            .run(&mut conn)
            .unwrap();
        conn.execute(
            "INSERT INTO app_state (key, value_json, updated_at) VALUES
             ('recent_folders', '[\"sworm://host/a\",\"/b\",\"/c\"]', 'x'),
             ('other', '[\"/d\"]', 'x')",
            [],
        )
        .unwrap();
        migrations::runner().run(&mut conn).unwrap();

        let get = |key: &str| -> String {
            conn.query_row(
                "SELECT value_json FROM app_state WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .unwrap()
        };
        let folders: Vec<serde_json::Value> = serde_json::from_str(&get("recent_folders")).unwrap();
        let paths: Vec<_> = folders
            .iter()
            .map(|folder| folder["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["sworm://host/a", "/b", "/c"]);
        let opened: Vec<_> = folders
            .iter()
            .map(|folder| {
                chrono::DateTime::parse_from_rfc3339(folder["opened_at"].as_str().unwrap()).unwrap()
            })
            .collect();
        assert!(
            opened.windows(2).all(|pair| pair[0] > pair[1]),
            "{opened:?}"
        );
        assert_eq!(get("other"), "[\"/d\"]");
    }

    #[test]
    fn v3_leaves_non_legacy_recent_folders_untouched() {
        for value in [
            "not json [",
            "{\"a\":1}",
            "[{\"path\":\"/x\",\"opened_at\":\"t\"}]",
            "[\"/a\",1]",
        ] {
            let mut conn = rusqlite::Connection::open_in_memory().unwrap();
            migrations::runner()
                .set_target(refinery::Target::Version(2))
                .run(&mut conn)
                .unwrap();
            conn.execute(
                "INSERT INTO app_state (key, value_json, updated_at) VALUES ('recent_folders', ?1, 'x')",
                [value],
            )
            .unwrap();
            migrations::runner().run(&mut conn).unwrap();
            let stored: String = conn
                .query_row(
                    "SELECT value_json FROM app_state WHERE key = 'recent_folders'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(stored, value);
        }
    }
}
