CREATE TABLE jobs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    episode_id  UUID NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    job_type    TEXT NOT NULL CHECK (job_type IN ('scrape','clean','tts')),
    status      TEXT NOT NULL DEFAULT 'queued'
                    CHECK (status IN ('queued','running','done','error')),
    attempts    INTEGER NOT NULL DEFAULT 0,
    run_after   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_jobs_queued ON jobs(status, run_after)
    WHERE status = 'queued';
