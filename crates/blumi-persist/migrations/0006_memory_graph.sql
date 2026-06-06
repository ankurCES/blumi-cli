-- SEDM memory graph: weighted similarity edges between memories. Pure
-- enrichment over the existing semantic memory (memories / memory_vectors) —
-- nothing here changes admission, utility, consolidation, or diffusion. Powers
-- graph-augmented recall (pull connected memories) + the memory-graph view.

CREATE TABLE IF NOT EXISTS memory_edges (
    src    INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    dst    INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    weight REAL NOT NULL,
    PRIMARY KEY (src, dst)
);
CREATE INDEX IF NOT EXISTS idx_memory_edges_src ON memory_edges(src);
CREATE INDEX IF NOT EXISTS idx_memory_edges_dst ON memory_edges(dst);
