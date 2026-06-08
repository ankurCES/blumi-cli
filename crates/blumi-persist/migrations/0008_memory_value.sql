-- Value: a learned *fitness* signal, separate from engagement (`utility`, which
-- only ever grows on retrieval). Updated by turn outcome (rewarded on productive
-- steps, decayed on failures) and RPL regret; eviction ranks by this so
-- genuinely-useful memories survive — not merely frequently-retrieved ones.
ALTER TABLE memories ADD COLUMN value REAL NOT NULL DEFAULT 1.0;
