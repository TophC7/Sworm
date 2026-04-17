-- Composite index for transcript reads.
-- Without this, session_transcript_get full-scans session_entries across
-- every session and kind that has ever existed, holding the DB mutex
-- while it sorts. The index keeps the lookup bounded by the size of a
-- single session's output log.

CREATE INDEX IF NOT EXISTS idx_session_entries_session_kind
  ON session_entries(session_id, kind, created_at, id);
