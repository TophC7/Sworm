use refinery::embed_migrations;
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::info;

embed_migrations!("migrations");

/// SQLite database service.
///
/// Owns the connection and runs embedded refinery migrations
/// automatically on initialization.
pub struct DatabaseService {
    conn: Connection,
    db_path: PathBuf,
}

impl DatabaseService {
    /// Open (or create) the database at the given path and run all
    /// pending migrations.
    pub fn new(db_path: PathBuf) -> Result<Self, anyhow::Error> {
        // Ensure the parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conn = Connection::open(&db_path)?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        // Run embedded migrations
        info!("Running database migrations from {:?}", db_path);
        migrations::runner().run(&mut conn)?;
        info!("Database migrations complete");

        Ok(Self { conn, db_path })
    }

    /// Return a reference to the raw connection for queries.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Return the database file path.
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// Simple smoke test: count rows in the projects table.
    pub fn smoke_test(&self) -> Result<String, anyhow::Error> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;

        Ok(format!(
            "DB OK at {:?}, projects count: {}",
            self.db_path, count
        ))
    }
}

/// Resolve the default database path inside the Tauri app data directory.
pub fn resolve_db_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, anyhow::Error> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to resolve app data dir: {}", e))?;
    Ok(app_data.join("ade.db"))
}

// Import the path resolver trait
use tauri::Manager;
