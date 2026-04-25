// Terminal transcript persistence.
//
// Stores an append-only byte log per session inside `session_entries`
// so terminal output survives app restarts. The write path runs on a
// dedicated thread with its own DB connection so the PTY reader never
// blocks on sqlite.
//
// Shape of a transcript row:
//   kind          = "output"
//   payload_json  = {"chunk": "<base64>", "seq": N}
//
// Reads concatenate rows in seq order and return the tail window.
//
// Durability model:
//   - seq is allocated on the writer thread, only after the INSERT
//     succeeds. A dropped / failed write does not burn a seq value,
//     so gaps in the seq sequence never arise from the happy path.
//   - `append` blocks up to WRITE_SEND_TIMEOUT when the queue is full,
//     so a momentary fsync stall doesn't silently drop terminal bytes.
//   - `shutdown_and_join` pushes a Shutdown sentinel and joins the
//     writer, guaranteeing all queued work reaches disk before exit.
//   - `clear` is routed through the same queue so deletes can't race
//     with in-flight appends for the same session.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use crossbeam_channel::SendTimeoutError;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use uuid::Uuid;

/// Default tail window returned by `transcript_get` when the caller
/// does not specify a byte limit.
pub const DEFAULT_TRANSCRIPT_TAIL_BYTES: usize = 1024 * 1024; // 1 MiB

/// Bounded write queue depth. A full queue means callers pay a short
/// blocking wait rather than dropping data on the floor.
const WRITE_QUEUE_DEPTH: usize = 256;

/// How long `append` waits for queue capacity before giving up. Long
/// enough to ride out a normal DB contention blip; short enough that
/// a stuck writer can't stall the PTY reader forever.
const WRITE_SEND_TIMEOUT: Duration = Duration::from_millis(500);

pub struct TranscriptService {
    db_path: PathBuf,
    tx: crossbeam_channel::Sender<WriteJob>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

enum WriteJob {
    Append {
        session_id: String,
        bytes: Vec<u8>,
    },
    /// Delete every transcript row for a session. Routed through the
    /// writer so the per-session seq counter is dropped in the same
    /// critical section as the DELETE.
    Clear {
        session_id: String,
    },
    /// Signal from shutdown_and_join: after this is handled the writer
    /// returns, letting `join()` complete.
    Shutdown,
}

impl TranscriptService {
    pub fn start(db_path: PathBuf) -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<WriteJob>(WRITE_QUEUE_DEPTH);
        let db_path_for_thread = db_path.clone();

        let handle = thread::Builder::new()
            .name("transcript-writer".to_string())
            .spawn(move || writer_loop(db_path_for_thread, rx))
            .expect("spawn transcript writer thread");

        Self {
            db_path,
            tx,
            writer: Mutex::new(Some(handle)),
        }
    }

    /// Queue a chunk for durable persistence. Blocks up to
    /// WRITE_SEND_TIMEOUT when the queue is full, then drops (with a
    /// warning) rather than stalling the PTY reader indefinitely.
    pub fn append(&self, session_id: &str, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let job = WriteJob::Append {
            session_id: session_id.to_string(),
            bytes: bytes.to_vec(),
        };
        match self.tx.send_timeout(job, WRITE_SEND_TIMEOUT) {
            Ok(()) => {}
            Err(SendTimeoutError::Timeout(_)) => {
                tracing::warn!(
                    "Transcript write queue saturated for {}, dropping {} bytes after {:?}",
                    session_id,
                    bytes.len(),
                    WRITE_SEND_TIMEOUT
                );
            }
            Err(SendTimeoutError::Disconnected(_)) => {
                tracing::debug!(
                    "Transcript writer disconnected, dropping {} bytes for {}",
                    bytes.len(),
                    session_id
                );
            }
        }
    }

    /// Delete every transcript row for a session.
    pub fn clear(&self, session_id: &str) {
        let job = WriteJob::Clear {
            session_id: session_id.to_string(),
        };
        if let Err(err) = self.tx.send(job) {
            tracing::warn!(
                "Transcript clear enqueue failed for {}: {}",
                session_id,
                err
            );
        }
    }

    /// Flush all queued writes and stop the writer thread. Called at
    /// app exit so the final batch of bytes reaches disk before the
    /// process is torn down.
    pub fn shutdown_and_join(&self) {
        // If we've already shut down, the handle will be gone and
        // further sends are no-ops after the writer exits.
        let handle = self.writer.lock().take();
        let Some(handle) = handle else {
            return;
        };
        // Sentinel is the last thing on the queue, so everything
        // before it is flushed to disk first. If the send fails,
        // the writer is already gone — either way, join cleans up.
        let _ = self.tx.send(WriteJob::Shutdown);
        if let Err(err) = handle.join() {
            tracing::warn!("Transcript writer thread panicked: {:?}", err);
        }
    }

    /// Load the transcript for a session and return the tail window as
    /// raw bytes. Runs on the caller's thread — designed for Tauri
    /// commands which already have their own DB connection.
    pub fn read_tail(
        conn: &Connection,
        session_id: &str,
        limit_bytes: Option<usize>,
    ) -> Result<Vec<u8>, String> {
        let limit = limit_bytes.unwrap_or(DEFAULT_TRANSCRIPT_TAIL_BYTES);
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM session_entries
                 WHERE session_id = ?1 AND kind = 'output'
                 ORDER BY created_at DESC, id DESC",
            )
            .map_err(|e| format!("transcript prepare failed: {}", e))?;

        let rows = stmt
            .query_map(rusqlite::params![session_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("transcript query failed: {}", e))?;

        let mut entries: Vec<(i64, Vec<u8>)> = Vec::new();
        let mut total = 0usize;
        for row in rows {
            let raw = match row {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!("Skipping unreadable transcript row: {}", err);
                    continue;
                }
            };
            match parse_payload(&raw) {
                Ok((seq, bytes)) => {
                    total += bytes.len();
                    entries.push((seq, bytes));
                    if total >= limit {
                        break;
                    }
                }
                Err(err) => {
                    tracing::warn!("Skipping malformed transcript payload: {}", err);
                }
            }
        }

        // Rows are read newest-first so large transcripts don't require
        // parsing the whole history. Sort the retained tail back into
        // playback order before concatenating.
        entries.sort_by_key(|(seq, _)| *seq);

        let mut overflow = total.saturating_sub(limit);
        let mut out = Vec::with_capacity(limit);
        for (_, bytes) in entries {
            if overflow >= bytes.len() {
                overflow -= bytes.len();
                continue;
            }
            if overflow > 0 {
                out.extend_from_slice(&bytes[overflow..]);
                overflow = 0;
            } else {
                out.extend_from_slice(&bytes);
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }
}

impl Drop for TranscriptService {
    fn drop(&mut self) {
        // Belt-and-braces: if something skipped shutdown_and_join, at
        // least join the thread by dropping our sender clone and
        // waiting.
        let handle = self.writer.lock().take();
        if let Some(handle) = handle {
            let _ = self.tx.send(WriteJob::Shutdown);
            let _ = handle.join();
        }
    }
}

fn writer_loop(db_path: PathBuf, rx: crossbeam_channel::Receiver<WriteJob>) {
    let conn = match open_conn(&db_path) {
        Ok(c) => c,
        Err(err) => {
            tracing::error!("Transcript writer failed to open DB: {}", err);
            return;
        }
    };

    let mut seq_counters: HashMap<String, i64> = HashMap::new();

    while let Ok(job) = rx.recv() {
        match job {
            WriteJob::Append { session_id, bytes } => {
                if let Err(err) = append_row(&conn, &mut seq_counters, &session_id, &bytes) {
                    tracing::warn!("Transcript append for {} failed: {}", session_id, err);
                }
            }
            WriteJob::Clear { session_id } => {
                if let Err(err) = clear_rows(&conn, &mut seq_counters, &session_id) {
                    tracing::warn!("Transcript clear for {} failed: {}", session_id, err);
                }
            }
            WriteJob::Shutdown => {
                tracing::info!("Transcript writer draining and exiting");
                return;
            }
        }
    }
}

fn open_conn(path: &PathBuf) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("transcript open failed: {}", e))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )
    .map_err(|e| format!("transcript pragma failed: {}", e))?;
    Ok(conn)
}

/// Look up the live max seq on disk for a session. Uses MAX(seq), not
/// COUNT(*), so gaps (cleared rows, a future multi-writer world) don't
/// cause us to reuse seq values.
fn load_seq_from_db(conn: &Connection, session_id: &str) -> Result<i64, String> {
    let max_seq: Option<i64> = conn
        .query_row(
            "SELECT MAX(CAST(json_extract(payload_json, '$.seq') AS INTEGER))
             FROM session_entries WHERE session_id = ?1 AND kind = 'output'",
            rusqlite::params![session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(|e| format!("transcript seq lookup failed: {}", e))?;
    Ok(max_seq.unwrap_or(0))
}

fn append_row(
    conn: &Connection,
    counters: &mut HashMap<String, i64>,
    session_id: &str,
    bytes: &[u8],
) -> Result<(), String> {
    // Tentative seq: if the INSERT fails we roll it back so the next
    // append for this session tries again with the same value.
    let tentative_seq = match counters.get(session_id) {
        Some(prev) => *prev + 1,
        None => load_seq_from_db(conn, session_id)? + 1,
    };

    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    let payload = serde_json::json!({
        "chunk": BASE64.encode(bytes),
        "seq": tentative_seq,
    })
    .to_string();

    conn.execute(
        "INSERT INTO session_entries (id, session_id, kind, payload_json, created_at)
         VALUES (?1, ?2, 'output', ?3, ?4)",
        rusqlite::params![id, session_id, payload, now],
    )
    .map_err(|e| format!("transcript insert failed: {}", e))?;

    counters.insert(session_id.to_string(), tentative_seq);
    Ok(())
}

fn clear_rows(
    conn: &Connection,
    counters: &mut HashMap<String, i64>,
    session_id: &str,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM session_entries WHERE session_id = ?1 AND kind = 'output'",
        rusqlite::params![session_id],
    )
    .map_err(|e| format!("transcript clear failed: {}", e))?;
    counters.remove(session_id);
    Ok(())
}

fn parse_payload(raw: &str) -> Result<(i64, Vec<u8>), String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("transcript payload parse failed: {}", e))?;
    let seq = value
        .get("seq")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "transcript payload missing seq".to_string())?;
    let chunk = value
        .get("chunk")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "transcript payload missing chunk".to_string())?;
    let bytes = BASE64
        .decode(chunk)
        .map_err(|e| format!("transcript chunk decode failed: {}", e))?;
    Ok((seq, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_entries (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    fn insert_chunk(conn: &Connection, id: &str, seq: i64, bytes: &[u8], created_at: &str) {
        let payload = serde_json::json!({
            "chunk": BASE64.encode(bytes),
            "seq": seq,
        })
        .to_string();
        conn.execute(
            "INSERT INTO session_entries (id, session_id, kind, payload_json, created_at)
             VALUES (?1, 's1', 'output', ?2, ?3)",
            rusqlite::params![id, payload, created_at],
        )
        .unwrap();
    }

    #[test]
    fn read_tail_returns_requested_suffix() {
        let conn = test_conn();
        insert_chunk(&conn, "a", 1, b"abc", "2026-01-01T00:00:00Z");
        insert_chunk(&conn, "b", 2, b"def", "2026-01-01T00:00:01Z");
        insert_chunk(&conn, "c", 3, b"ghi", "2026-01-01T00:00:02Z");

        let bytes = TranscriptService::read_tail(&conn, "s1", Some(5)).unwrap();

        assert_eq!(bytes, b"efghi");
    }

    #[test]
    fn read_tail_preserves_playback_order() {
        let conn = test_conn();
        insert_chunk(&conn, "a", 1, b"one", "2026-01-01T00:00:00Z");
        insert_chunk(&conn, "b", 2, b"two", "2026-01-01T00:00:01Z");
        insert_chunk(&conn, "c", 3, b"three", "2026-01-01T00:00:02Z");

        let bytes = TranscriptService::read_tail(&conn, "s1", Some(20)).unwrap();

        assert_eq!(bytes, b"onetwothree");
    }

    #[test]
    fn read_tail_zero_limit_returns_empty() {
        let conn = test_conn();
        insert_chunk(&conn, "a", 1, b"abc", "2026-01-01T00:00:00Z");

        let bytes = TranscriptService::read_tail(&conn, "s1", Some(0)).unwrap();

        assert!(bytes.is_empty());
    }
}
