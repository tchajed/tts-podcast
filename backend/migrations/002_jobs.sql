CREATE TABLE jobs (
    id          TEXT PRIMARY KEY,
    episode_id  TEXT NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    job_type    TEXT NOT NULL CHECK (job_type IN ('scrape','pdf','clean','tts','image')),
    status      TEXT NOT NULL DEFAULT 'queued'
                    CHECK (status IN ('queued','running','done','error')),
    attempts    INTEGER NOT NULL DEFAULT 0,
    run_after   TEXT NOT NULL DEFAULT (datetime('now')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_jobs_queued ON jobs(status, run_after)
    WHERE status = 'queued';
