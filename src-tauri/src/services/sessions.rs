use crate::models::session::Session;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Session service: CRUD for persisted sessions.
pub struct SessionService;

impl SessionService {
    pub fn new() -> Self {
        Self
    }

    /// Create a new session for a project+provider.
    pub fn create(
        &self,
        conn: &Connection,
        project_id: &str,
        provider_id: &str,
        title: &str,
        cwd: &str,
        branch: Option<&str>,
    ) -> Result<Session, String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let provider_resume_token = if provider_id == "claude_code" {
            Some(Self::deterministic_claude_session_id(&id))
        } else {
            None
        };

        conn.execute(
            "INSERT INTO sessions (
                id, project_id, provider_id, title, cwd, branch, status,
                shared_workspace, auto_approve, provider_resume_token, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'idle', 1, 0, ?7, ?8, ?9)",
            rusqlite::params![
                id,
                project_id,
                provider_id,
                title,
                cwd,
                branch,
                provider_resume_token,
                now,
                now
            ],
        )
        .map_err(|e| format!("Failed to create session: {}", e))?;

        Ok(Session {
            id,
            project_id: project_id.to_string(),
            provider_id: provider_id.to_string(),
            title: title.to_string(),
            cwd: cwd.to_string(),
            branch: branch.map(|value| value.to_string()),
            status: "idle".to_string(),
            shared_workspace: true,
            auto_approve: false,
            provider_resume_token,
            created_at: now.clone(),
            updated_at: now,
            last_started_at: None,
            last_stopped_at: None,
        })
    }

    /// Derive a deterministic UUID from the Sworm session ID.
    /// Claude Code CLI requires `--session-id` to be a valid UUID,
    /// so we hash the Sworm ID and format the first 16 bytes as UUID v4.
    pub fn deterministic_claude_session_id(app_session_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("claude:{}", app_session_id));
        let hash = hasher.finalize();

        // Take first 16 bytes and format as UUID v4 (set version + variant bits)
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
        bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1

        Uuid::from_bytes(bytes).to_string()
    }

    pub fn list_for_project(
        &self,
        conn: &Connection,
        project_id: &str,
    ) -> Result<Vec<Session>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, provider_id, title, cwd, branch, status,
                 shared_workspace, auto_approve, provider_resume_token,
                 created_at, updated_at, last_started_at, last_stopped_at
                 FROM sessions WHERE project_id = ?1 ORDER BY updated_at DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map(rusqlite::params![project_id], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    provider_id: row.get(2)?,
                    title: row.get(3)?,
                    cwd: row.get(4)?,
                    branch: row.get(5)?,
                    status: row.get(6)?,
                    shared_workspace: row.get(7)?,
                    auto_approve: row.get(8)?,
                    provider_resume_token: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    last_started_at: row.get(12)?,
                    last_stopped_at: row.get(13)?,
                })
            })
            .map_err(|e| format!("Failed to query sessions: {}", e))?;

        let mut sessions = Vec::new();
        for row in rows {
            match row {
                Ok(session) => sessions.push(session),
                Err(error) => {
                    tracing::warn!("Dropping unreadable session row: {}", error);
                }
            }
        }

        Ok(sessions)
    }

    pub fn get(&self, conn: &Connection, id: &str) -> Result<Option<Session>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, provider_id, title, cwd, branch, status,
                 shared_workspace, auto_approve, provider_resume_token,
                 created_at, updated_at, last_started_at, last_stopped_at
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        stmt.query_row(rusqlite::params![id], |row| {
            Ok(Session {
                id: row.get(0)?,
                project_id: row.get(1)?,
                provider_id: row.get(2)?,
                title: row.get(3)?,
                cwd: row.get(4)?,
                branch: row.get(5)?,
                status: row.get(6)?,
                shared_workspace: row.get(7)?,
                auto_approve: row.get(8)?,
                provider_resume_token: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                last_started_at: row.get(12)?,
                last_stopped_at: row.get(13)?,
            })
        })
        .optional()
        .map_err(|e| format!("Failed to get session: {}", e))
    }

    pub fn update_status(&self, conn: &Connection, id: &str, status: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        match status {
            "running" => conn
                .execute(
                    "UPDATE sessions
                     SET status = ?1, updated_at = ?2, last_started_at = ?2
                     WHERE id = ?3",
                    rusqlite::params![status, now, id],
                )
                .map_err(|e| format!("Failed to update session status: {}", e))?,
            "stopped" | "exited" => conn
                .execute(
                    "UPDATE sessions
                     SET status = ?1, updated_at = ?2, last_stopped_at = ?2
                     WHERE id = ?3",
                    rusqlite::params![status, now, id],
                )
                .map_err(|e| format!("Failed to update session status: {}", e))?,
            _ => conn
                .execute(
                    "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![status, now, id],
                )
                .map_err(|e| format!("Failed to update session status: {}", e))?,
        };

        Ok(())
    }

    pub fn set_resume_token(
        &self,
        conn: &Connection,
        id: &str,
        token: &str,
    ) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE sessions SET provider_resume_token = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![token, now, id],
        )
        .map_err(|e| format!("Failed to persist resume token: {}", e))?;

        Ok(())
    }

    pub fn clear_resume_token(&self, conn: &Connection, id: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE sessions SET provider_resume_token = NULL, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )
        .map_err(|e| format!("Failed to clear resume token: {}", e))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn count_live_for_project(
        &self,
        conn: &Connection,
        project_id: &str,
    ) -> Result<i64, String> {
        conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE project_id = ?1 AND status = 'running'",
            rusqlite::params![project_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count live sessions: {}", e))
    }

    pub fn count_unbound_codex_for_project(
        &self,
        conn: &Connection,
        project_id: &str,
    ) -> Result<i64, String> {
        conn.query_row(
            "SELECT COUNT(*) FROM sessions
             WHERE project_id = ?1
               AND provider_id = 'codex'
               AND status = 'running'
               AND provider_resume_token IS NULL",
            rusqlite::params![project_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count unbound Codex sessions: {}", e))
    }

    /// Reset a session to fresh state: clear timestamps, set status to idle,
    /// and optionally set a new resume token.
    pub fn reset_session(
        &self,
        conn: &Connection,
        id: &str,
        new_token: Option<&str>,
    ) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE sessions
             SET status = 'idle',
                 provider_resume_token = ?1,
                 last_started_at = NULL,
                 last_stopped_at = NULL,
                 updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![new_token, now, id],
        )
        .map_err(|e| format!("Failed to reset session: {}", e))?;

        tracing::info!("Session {} reset to fresh state", id);
        Ok(())
    }

    pub fn remove(&self, conn: &Connection, id: &str) -> Result<(), String> {
        conn.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("Failed to remove session: {}", e))?;
        Ok(())
    }

    /// Reset any sessions left in "running" status from a previous process.
    ///
    /// On startup there are no live PTYs, so any "running" session is stale
    /// (e.g. from a crash, SIGKILL, or dev-mode hot-reload). Mark them
    /// "exited" so they don't block new session creation.
    pub fn reset_stale_running(&self, conn: &Connection) -> Result<usize, String> {
        let now = Utc::now().to_rfc3339();
        let count = conn
            .execute(
                "UPDATE sessions
                 SET status = 'exited', updated_at = ?1, last_stopped_at = ?1
                 WHERE status = 'running'",
                rusqlite::params![now],
            )
            .map_err(|e| format!("Failed to reset stale sessions: {}", e))?;

        if count > 0 {
            tracing::info!("Reset {} stale running session(s) from previous process", count);
        }
        Ok(count)
    }
}
