-- P0 of the structural code graph: typed, resolved edges + symbol enrichment.
-- Backward compatible — the Tier-0 `build_graph()` keeps inserting (src, dst)
-- rows, which now default to kind='ref' / resolved=0 / count=1.

-- Symbol enrichment for resolution + display (all nullable → no backfill).
ALTER TABLE code_symbols ADD COLUMN fqname    TEXT;     -- module-qualified name
ALTER TABLE code_symbols ADD COLUMN parent_id INTEGER;  -- soft ref: enclosing decl's id
ALTER TABLE code_symbols ADD COLUMN signature TEXT;     -- one-line decl signature

-- Rebuild code_edges with `kind` in the primary key (an edge pair can carry
-- several relations), plus a `resolved` flag (1 = scope/import-resolved, 0 =
-- name heuristic) and a `count` of collapsed reference sites. SQLite can't alter
-- a primary key in place, so recreate (the graph is rebuilt from symbols on the
-- next ingest anyway).
CREATE TABLE code_edges_new (
    src      INTEGER NOT NULL REFERENCES code_symbols(id) ON DELETE CASCADE,
    dst      INTEGER NOT NULL REFERENCES code_symbols(id) ON DELETE CASCADE,
    kind     TEXT    NOT NULL DEFAULT 'ref',
    resolved INTEGER NOT NULL DEFAULT 0,
    count    INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (src, dst, kind)
);
INSERT OR IGNORE INTO code_edges_new (src, dst, kind, resolved, count)
    SELECT src, dst, 'ref', 0, 1 FROM code_edges;
DROP TABLE code_edges;
ALTER TABLE code_edges_new RENAME TO code_edges;
CREATE INDEX IF NOT EXISTS idx_code_edges_src ON code_edges(src, kind);
CREATE INDEX IF NOT EXISTS idx_code_edges_dst ON code_edges(dst, kind);  -- "who refs X"
