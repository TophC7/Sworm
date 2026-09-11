-- Transcripts of window runs whose process exited. A daemon outlives the
-- desktop, so a run can finish while nobody is attached; its output and exit
-- code wait here instead of in memory.
CREATE TABLE completed_runs (
  run_id       TEXT PRIMARY KEY,
  exit_code    INTEGER,
  output_start INTEGER NOT NULL,
  output       BLOB NOT NULL,
  events_json  TEXT NOT NULL,
  finished_at  TEXT NOT NULL
);

CREATE INDEX completed_runs_finished_at ON completed_runs (finished_at);
