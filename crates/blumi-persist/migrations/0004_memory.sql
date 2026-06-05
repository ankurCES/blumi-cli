-- Semantic long-term memory (LangGraph "Store" analog + SEDM governance).
-- Vectors live in a sibling table (normalized f32 LE BLOBs; cosine = dot
-- product). FTS5 mirrors the text for the keyword fallback when embeddings are
-- unavailable. Governance columns (hits/utility/status/origin) drive SEDM
-- consolidation, eviction, and cross-peer diffusion.

CREATE TABLE IF NOT EXISTS memories (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace      TEXT NOT NULL,                 -- user | agent | project:<hash>
    kind           TEXT NOT NULL DEFAULT 'note',
    text           TEXT NOT NULL,
    origin         TEXT NOT NULL DEFAULT '',      -- authoring node id ('' = local)
    source_session TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    hits           INTEGER NOT NULL DEFAULT 0,
    last_used_at   TEXT,
    utility        REAL NOT NULL DEFAULT 1.0,
    status         TEXT NOT NULL DEFAULT 'active'  -- active | merged | evicted
);

CREATE INDEX IF NOT EXISTS idx_memories_ns ON memories(namespace, status);
CREATE INDEX IF NOT EXISTS idx_memories_util ON memories(namespace, status, utility);

-- One normalized vector per memory.
CREATE TABLE IF NOT EXISTS memory_vectors (
    memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    model     TEXT NOT NULL,
    dim       INTEGER NOT NULL,
    vec       BLOB NOT NULL
);

-- External-content FTS5 over memory text (porter). `text` is immutable after
-- insert, so only insert/delete need mirroring (no update trigger).
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    text,
    content='memories',
    content_rowid='id',
    tokenize='porter'
);

CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.id, old.text);
END;
