//! Durable transcripts of window runs that already exited.
//!
//! A daemon outlives its desktop, so a run can finish while nobody is
//! attached. Keeping the retained window in memory for that case leaks a run's
//! worth of output per finished run and loses everything on daemon restart, so
//! the transcript is written here at exit and the memory is freed.

use crate::services::db::DatabaseService;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension};
use std::sync::Arc;
use sworm_protocol::pty::PtyEvent;
use tracing::warn;

/// Everything a reconnecting desktop needs to replay a finished run.
pub struct CompletedRun {
    pub run_id: String,
    pub exit_code: Option<i32>,
    /// Offset of the first retained output byte. Earlier bytes were trimmed
    /// out of the rolling window before the run exited.
    pub output_start: u64,
    pub output: Vec<u8>,
    /// Lifecycle events with the sequence numbers the run assigned them.
    pub events: Vec<(u64, PtyEvent)>,
}

impl CompletedRun {
    /// Offset one past the last retained output byte.
    pub fn output_end(&self) -> u64 {
        self.output_start.saturating_add(self.output.len() as u64)
    }

    /// Highest event sequence in the transcript.
    pub fn event_sequence(&self) -> u64 {
        self.events.last().map_or(0, |(sequence, _)| *sequence)
    }
}

/// Transcript store, pruned by age and count on every insert.
pub struct CompletedRunStore {
    db: Arc<DatabaseService>,
    keep: usize,
    max_age: Duration,
}

impl CompletedRunStore {
    pub fn new(db: Arc<DatabaseService>, keep: usize, max_age: Duration) -> Self {
        Self { db, keep, max_age }
    }

    pub fn put(&self, run: &CompletedRun) -> Result<(), String> {
        let events = serde_json::to_string(&run.events)
            .map_err(|error| format!("encode completed run events: {error}"))?;
        let db = self.db.write();
        db.conn()
            .execute(
                "INSERT INTO completed_runs
                   (run_id, exit_code, output_start, output, events_json, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(run_id) DO UPDATE SET
                   exit_code = excluded.exit_code,
                   output_start = excluded.output_start,
                   output = excluded.output,
                   events_json = excluded.events_json,
                   finished_at = excluded.finished_at",
                rusqlite::params![
                    run.run_id,
                    run.exit_code,
                    run.output_start as i64,
                    run.output,
                    events,
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|error| format!("store completed run: {error}"))?;
        self.prune(db.conn())
    }

    pub fn get(&self, run_id: &str) -> Result<Option<CompletedRun>, String> {
        let db = self.db.read();
        let row = db
            .conn()
            .query_row(
                "SELECT exit_code, output_start, output, events_json
                   FROM completed_runs WHERE run_id = ?1",
                rusqlite::params![run_id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("read completed run: {error}"))?;
        let Some((exit_code, output_start, output, events_json)) = row else {
            return Ok(None);
        };
        let events = serde_json::from_str(&events_json)
            .map_err(|error| format!("decode completed run events: {error}"))?;
        Ok(Some(CompletedRun {
            run_id: run_id.to_owned(),
            exit_code: exit_code.map(|code| code as i32),
            output_start: output_start as u64,
            output,
            events,
        }))
    }

    /// Drop a transcript whose run was explicitly stopped.
    pub fn delete(&self, run_id: &str) {
        let db = self.db.write();
        if let Err(error) = db.conn().execute(
            "DELETE FROM completed_runs WHERE run_id = ?1",
            rusqlite::params![run_id],
        ) {
            warn!("Failed to delete completed run {run_id}: {error}");
        }
    }

    fn prune(&self, conn: &Connection) -> Result<(), String> {
        let cutoff: DateTime<Utc> = Utc::now() - self.max_age;
        conn.execute(
            "DELETE FROM completed_runs WHERE finished_at < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
        )
        .map_err(|error| format!("prune completed runs by age: {error}"))?;
        conn.execute(
            "DELETE FROM completed_runs WHERE run_id NOT IN (
               SELECT run_id FROM completed_runs ORDER BY finished_at DESC LIMIT ?1
             )",
            rusqlite::params![self.keep as i64],
        )
        .map_err(|error| format!("prune completed runs by count: {error}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Drops the database file the store was built on.
    struct TempDb(PathBuf);

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store(keep: usize, max_age: Duration) -> (TempDb, CompletedRunStore) {
        let dir = std::env::temp_dir().join(format!("sworm-completed-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let db = Arc::new(DatabaseService::new(dir.join("test.db")).expect("database"));
        (TempDb(dir), CompletedRunStore::new(db, keep, max_age))
    }

    fn run(run_id: &str) -> CompletedRun {
        CompletedRun {
            run_id: run_id.to_owned(),
            exit_code: Some(7),
            output_start: 4,
            output: b"tail".to_vec(),
            events: vec![(
                3,
                PtyEvent::Exit {
                    run_id: run_id.to_owned(),
                    code: Some(7),
                },
            )],
        }
    }

    #[test]
    fn stores_and_reads_a_transcript() {
        let (_dir, store) = store(8, Duration::days(7));
        store.put(&run("one")).expect("put");

        let stored = store.get("one").expect("get").expect("stored run");
        assert_eq!(stored.exit_code, Some(7));
        assert_eq!(stored.output_start, 4);
        assert_eq!(stored.output_end(), 8);
        assert_eq!(stored.output, b"tail");
        assert_eq!(stored.event_sequence(), 3);

        store.delete("one");
        assert!(store.get("one").expect("get").is_none());
    }

    #[test]
    fn keeps_only_the_newest_transcripts() {
        let (_dir, store) = store(2, Duration::days(7));
        for run_id in ["one", "two", "three"] {
            store.put(&run(run_id)).expect("put");
            // finished_at has second resolution at RFC3339 formatting edges.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        assert!(store.get("one").expect("get").is_none());
        assert!(store.get("two").expect("get").is_some());
        assert!(store.get("three").expect("get").is_some());
    }

    #[test]
    fn drops_transcripts_past_their_age() {
        let (_dir, store) = store(8, Duration::zero());
        store.put(&run("stale")).expect("put");
        assert!(store.get("stale").expect("get").is_none());
    }
}
