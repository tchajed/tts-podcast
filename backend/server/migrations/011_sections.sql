-- Per-episode section timestamps. NULL for episodes without section
-- structure (articles, summarized mode, or papers where cleaning didn't
-- emit `## ` headers). JSON array of { "title": "...", "start_secs": N }.
ALTER TABLE episodes ADD COLUMN sections_json TEXT;
