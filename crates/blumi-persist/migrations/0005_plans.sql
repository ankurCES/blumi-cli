-- Proposed-plan history (the `/plans` browser). Append-only on resolve; the most
-- recent approved plan is "live", earlier approved ones are "approved", and
-- declined ones are "rejected". Shared by the TUI and the gateway (blugo).

CREATE TABLE IF NOT EXISTS plans (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    content     TEXT NOT NULL,
    status      TEXT NOT NULL,            -- 'live' | 'approved' | 'rejected'
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_plans_created ON plans(id);
