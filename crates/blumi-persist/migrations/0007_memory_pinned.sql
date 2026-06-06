-- White-box memory: let a user PIN critical entries so SEDM governance
-- (eviction + consolidation) never quietly removes or merges them away.
-- DEFAULT 0 keeps every existing row backward-compatible.
ALTER TABLE memories ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_memories_pinned ON memories(namespace, status, pinned);
