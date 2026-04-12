CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TABLE feeds (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug         TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    feed_token   UUID NOT NULL UNIQUE DEFAULT gen_random_uuid(),
    tts_default  TEXT NOT NULL DEFAULT 'openai'
                     CHECK (tts_default IN ('openai', 'elevenlabs')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE episodes (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feed_id        UUID NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    title          TEXT NOT NULL,
    source_url     TEXT NOT NULL,
    source_type    TEXT NOT NULL CHECK (source_type IN ('article', 'arxiv')),
    raw_text       TEXT,
    cleaned_text   TEXT,
    audio_url      TEXT,
    duration_secs  INTEGER,
    tts_provider   TEXT CHECK (tts_provider IN ('openai', 'elevenlabs')),
    status         TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending','scraping','cleaning',
                                         'tts','done','error')),
    error_msg      TEXT,
    pub_date       TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_episodes_feed_status ON episodes(feed_id, status);
CREATE INDEX idx_episodes_pub_date    ON episodes(feed_id, pub_date DESC);
