-- Learned per-symbol fitness for code search (mirrors `memories.value`): rewarded
-- when surfaced symbols contribute to a productive turn, decayed on failure.
-- Folded into the recall ranking (value-weighted cosine) so genuinely-useful
-- symbols float up over time. 1.0 = neutral.
ALTER TABLE code_symbols ADD COLUMN value REAL NOT NULL DEFAULT 1.0;
