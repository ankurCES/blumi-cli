-- Native-lite code knowledge base. Lives in its own DB (knowledge.db) so it can
-- be wiped independently of chat history. Symbols are the unit of retrieval:
-- each has an FTS5 row (name + snippet) and, when embeddings are on, a vector.

CREATE TABLE IF NOT EXISTS code_files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source      TEXT NOT NULL,            -- ingest root label (for list/remove)
    path        TEXT NOT NULL UNIQUE,     -- absolute file path
    lang        TEXT NOT NULL DEFAULT '',
    sha         TEXT NOT NULL,            -- content hash for diff-aware re-index
    symbols     INTEGER NOT NULL DEFAULT 0,
    indexed_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_code_files_source ON code_files(source);

CREATE TABLE IF NOT EXISTS code_symbols (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id     INTEGER NOT NULL REFERENCES code_files(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    start_line  INTEGER NOT NULL,
    end_line    INTEGER NOT NULL,
    snippet     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_code_symbols_file ON code_symbols(file_id);

-- One normalized vector per symbol (cosine = dot). Cascades with the symbol.
CREATE TABLE IF NOT EXISTS code_vec (
    symbol_id   INTEGER PRIMARY KEY REFERENCES code_symbols(id) ON DELETE CASCADE,
    model       TEXT NOT NULL,
    dim         INTEGER NOT NULL,
    vec         BLOB NOT NULL
);

-- External-content FTS5 over symbol name + snippet (porter), kept in sync by
-- triggers. Symbols are immutable once inserted (re-index deletes + re-inserts),
-- so only insert/delete mirroring is needed.
CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(
    name, snippet,
    content='code_symbols',
    content_rowid='id',
    tokenize='porter'
);

CREATE TRIGGER IF NOT EXISTS code_symbols_ai AFTER INSERT ON code_symbols BEGIN
    INSERT INTO code_fts(rowid, name, snippet) VALUES (new.id, new.name, new.snippet);
END;

CREATE TRIGGER IF NOT EXISTS code_symbols_ad AFTER DELETE ON code_symbols BEGIN
    INSERT INTO code_fts(code_fts, rowid, name, snippet) VALUES('delete', old.id, old.name, old.snippet);
END;
