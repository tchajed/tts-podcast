CREATE TABLE feeds (
    id           TEXT PRIMARY KEY,
    slug         TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    feed_token   TEXT NOT NULL UNIQUE,
    tts_default  TEXT NOT NULL DEFAULT 'openai'
                     CHECK (tts_default IN ('openai', 'elevenlabs', 'google')),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE episodes (
    id             TEXT PRIMARY KEY,
    feed_id        TEXT NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    title          TEXT NOT NULL DEFAULT '',
    source_url     TEXT,
    source_type    TEXT NOT NULL CHECK (source_type IN ('article', 'arxiv', 'pdf')),
    raw_text       TEXT,
    cleaned_text   TEXT,
    audio_url      TEXT,
    image_url      TEXT,
    duration_secs  INTEGER,
    tts_provider   TEXT CHECK (tts_provider IN ('openai', 'elevenlabs', 'google')),
    status         TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending','scraping','cleaning',
                                         'tts','image','done','error')),
    error_msg      TEXT,
    pub_date       TEXT,
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_episodes_feed_status ON episodes(feed_id, status);
CREATE INDEX idx_episodes_pub_date    ON episodes(feed_id, pub_date DESC);
