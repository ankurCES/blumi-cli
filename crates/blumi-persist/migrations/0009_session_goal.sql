-- Standing session objective (set via /goal or the gateway). Persisted so a
-- long autonomous task keeps its goal across a resume, not just an in-actor
-- rollover. Empty string = no goal.
ALTER TABLE sessions ADD COLUMN goal TEXT NOT NULL DEFAULT '';
