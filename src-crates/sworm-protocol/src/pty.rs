use serde::{Deserialize, Serialize};

/// Events emitted over the lifecycle channel. `run_id` is the ephemeral
/// PTY identity minted by the frontend for one spawn; the durable tab
/// identity never reaches this layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PtyEvent {
    Started {
        run_id: String,
        pid: Option<u32>,
    },
    Exit {
        run_id: String,
        code: Option<i32>,
    },
    Error {
        run_id: String,
        message: String,
    },
    /// Re-seeds the subscriber's render barrier after replay.
    Synced {
        run_id: String,
        sequence: u64,
    },
    /// A provider-side resume identity was discovered for a run after
    /// spawn (Codex thread id, Antigravity conversation id, OMP session id).
    ResumeTokenBound {
        run_id: String,
        token: String,
    },
}
