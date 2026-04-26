use parking_lot::{Mutex, MutexGuard};
use refinery::embed_migrations;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::info;

embed_migrations!("migrations");

/// Number of read-only connections in the pool. Four covers concurrent
/// UI fetches (sessions list + transcripts + workspace state + git
/// summary writeback) without exhausting per-process file handles.
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
///   read-only queries on hot paths (session lists, transcripts,
///   workspace state) so a long-running write doesn't block them.
pub struct DatabaseService {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    /// Round-robin starting index for the read pool; keeps read
    /// load spread across connections instead of always hammering
    /// the first one.
    read_cursor: AtomicUsize,
    db_path: PathBuf,
}

impl DatabaseService {
    /// Open (or create) the database at the given path and run all
    /// pending migrations on the writer connection. Builds the read
    /// pool after migrations succeed so readers don't observe a
    /// half-migrated schema.
    pub fn new(db_path: PathBuf) -> Result<Self, anyhow::Error> {
        // Ensure the parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut writer = Connection::open(&db_path)?;
        // WAL is per-database (the journal_mode pragma persists), but
        // we still issue it on every connection so a fresh file gets
        // upgraded on its first open.
        writer.execute_batch("PRAGMA journal_mode=WAL;")?;
        writer.execute_batch("PRAGMA foreign_keys=ON;")?;
        // 5s busy timeout so concurrent writers (transcript service +
        // command thread) wait briefly instead of returning SQLITE_BUSY.
        writer.execute_batch("PRAGMA busy_timeout=5000;")?;

        info!("Running database migrations from {:?}", db_path);
        migrations::runner().run(&mut writer)?;
        info!("Database migrations complete");

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
            db_path,
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

    /// Return the database file path.
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// Simple smoke test: count rows in the projects table.
    pub fn smoke_test(&self) -> Result<String, anyhow::Error> {
        let guard = self.read();
        let count: i64 = guard
            .conn()
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;

        Ok(format!(
            "DB OK at {:?}, projects count: {}",
            self.db_path, count
        ))
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

/// Resolve the default database path inside the Tauri app data directory.
pub fn resolve_db_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, anyhow::Error> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to resolve app data dir: {}", e))?;
    Ok(app_data.join("sworm.db"))
}

// Import the path resolver trait
use tauri::Manager;
