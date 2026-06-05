-- Durable-execution checkpoints (LangGraph-checkpointer analog).
-- One in-progress turn per session, overwritten after each completed tool step,
-- so a crash/gateway-restart resumes the turn from the last step instead of
-- replaying it. Cleared on clean turn completion (the full session snapshot is
-- the durable record once a turn finishes).
CREATE TABLE IF NOT EXISTS checkpoints (
    session_id    TEXT NOT NULL,
    turn_seq      INTEGER NOT NULL,
    step          INTEGER NOT NULL,
    messages_json TEXT NOT NULL,
    todos_json    TEXT NOT NULL,
    model         TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'in_progress',
    created_at    TEXT NOT NULL,
    PRIMARY KEY (session_id, turn_seq)
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_incomplete
    ON checkpoints(session_id, status);
