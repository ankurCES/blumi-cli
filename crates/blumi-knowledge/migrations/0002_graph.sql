-- Symbol reference graph (graphify-style, native): an edge src→dst means the
-- body of symbol `src` mentions the name of symbol `dst`. Rebuilt after ingest.
-- Powers cheap structural retrieval (neighbors / shortest path / hubs) so the
-- agent can answer "what connects/relies on X" with a small subgraph instead of
-- re-reading whole files.

CREATE TABLE IF NOT EXISTS code_edges (
    src   INTEGER NOT NULL REFERENCES code_symbols(id) ON DELETE CASCADE,
    dst   INTEGER NOT NULL REFERENCES code_symbols(id) ON DELETE CASCADE,
    PRIMARY KEY (src, dst)
);
CREATE INDEX IF NOT EXISTS idx_code_edges_src ON code_edges(src);
CREATE INDEX IF NOT EXISTS idx_code_edges_dst ON code_edges(dst);
