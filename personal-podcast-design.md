# Personal Podcast App — Design Document

**Audience:** Claude Code + Opus 4.6 implementing from scratch  
**Status:** Ready for implementation  
**Stack:** Rust (Axum) · Postgres · Tigris (S3) · Next.js · Fly.io

---

## 1. Overview

A self-hosted web app that converts web articles and arXiv papers into podcast episodes. Users submit URLs via a web UI; the backend scrapes, cleans the text using Claude, synthesizes audio via TTS, and publishes episodes to a private RSS feed consumable by any podcast client.

There is no user authentication. Access is controlled by secret feed tokens (UUIDs) embedded in RSS URLs and API requests. Multiple feeds are supported for topic organization.

---

## 2. Repository Structure

```
/
├── backend/                  # Rust Axum server
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── config.rs         # Env var loading, AppConfig struct
│   │   ├── db.rs             # sqlx pool setup, migration runner
│   │   ├── error.rs          # Unified AppError type, IntoResponse impl
│   │   ├── routes/
│   │   │   ├── mod.rs        # Router assembly
│   │   │   ├── feeds.rs      # Feed CRUD
│   │   │   ├── episodes.rs   # Episode submission + status
│   │   │   └── rss.rs        # RSS XML generation
│   │   ├── pipeline/
│   │   │   ├── mod.rs
│   │   │   ├── scrape.rs     # HTTP fetch + readability
│   │   │   ├── clean.rs      # Claude API cleanup
│   │   │   ├── tts.rs        # TTS dispatch (OpenAI / ElevenLabs)
│   │   │   └── storage.rs    # Tigris S3 upload
│   │   └── worker.rs         # Postgres-backed job loop
│   └── migrations/
│       ├── 001_initial.sql
│       └── 002_jobs.sql
├── frontend/                 # Next.js app
│   ├── package.json
│   └── src/
│       ├── app/
│       │   ├── page.tsx              # Feed list
│       │   ├── feeds/[token]/page.tsx  # Feed view + submit form
│       │   └── feeds/[token]/episodes/[id]/page.tsx
│       └── lib/
│           └── api.ts        # Typed fetch wrappers
├── fly.toml
└── Dockerfile
```

---

## 3. Data Model

### 3.1 SQL Schema

```sql
-- migrations/001_initial.sql

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TABLE feeds (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug         TEXT NOT NULL UNIQUE,        -- human-readable, URL-safe: "ml-papers"
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    feed_token   UUID NOT NULL UNIQUE DEFAULT gen_random_uuid(),  -- secret RSS key
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
    raw_text       TEXT,                      -- populated after scrape stage
    cleaned_text   TEXT,                      -- populated after clean stage
    audio_url      TEXT,                      -- populated after tts stage
    duration_secs  INTEGER,
    tts_provider   TEXT CHECK (tts_provider IN ('openai', 'elevenlabs')),
    status         TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending','scraping','cleaning',
                                         'tts','done','error')),
    error_msg      TEXT,
    pub_date       TIMESTAMPTZ,               -- set when status → done
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_episodes_feed_status ON episodes(feed_id, status);
CREATE INDEX idx_episodes_pub_date    ON episodes(feed_id, pub_date DESC);
```

```sql
-- migrations/002_jobs.sql

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
```

### 3.2 Key Invariants

- Every episode has exactly one active job at a time. A new job is inserted only when the previous one completes successfully or permanently fails.
- `episode.status` mirrors the latest job stage. Update them together in a transaction.
- `episode.pub_date` is set only when `status = 'done'`. RSS query filters on this.
- Feed tokens are never exposed in logs.

---

## 4. Configuration

All config is read from environment variables at startup. The `AppConfig` struct is populated once and stored in Axum state.

```
# Required
DATABASE_URL=postgres://...
AWS_ACCESS_KEY_ID=tid_...
AWS_SECRET_ACCESS_KEY=tsec_...
AWS_ENDPOINT_URL_S3=https://fly.storage.tigris.dev
AWS_REGION=auto
BUCKET_NAME=your-bucket-name
ANTHROPIC_API_KEY=sk-ant-...
ADMIN_TOKEN=...                 # Bearer token for feed creation

# One or both TTS providers required
OPENAI_API_KEY=sk-...
ELEVENLABS_API_KEY=...

# Optional
ELEVENLABS_VOICE_ID=...         # Default ElevenLabs voice (falls back to "Rachel")
HOST=0.0.0.0
PORT=8080
WORKER_POLL_INTERVAL_SECS=5     # Default: 5
MAX_JOB_ATTEMPTS=3              # Default: 3
```

`AppConfig` should `panic!` at startup if required vars are missing. Never log secrets.

---

## 5. API Routes

Base path: `/api/v1`

All write operations (POST/DELETE on feeds) require `Authorization: Bearer {ADMIN_TOKEN}`.

Feed-scoped reads (episodes, RSS) require the `feed_token` in the URL path — no additional auth.

### 5.1 Feeds

```
POST   /api/v1/feeds
GET    /api/v1/feeds
GET    /api/v1/feeds/:feed_token
DELETE /api/v1/feeds/:feed_token
```

**POST /api/v1/feeds** — Create a feed. Admin-only.

Request:
```json
{
  "slug": "ml-papers",
  "title": "ML Papers",
  "description": "Weekly arXiv reading list",
  "tts_default": "openai"
}
```

Response `201`:
```json
{
  "id": "uuid",
  "slug": "ml-papers",
  "title": "ML Papers",
  "feed_token": "uuid",
  "rss_url": "https://yourapp.fly.dev/feed/uuid/rss.xml"
}
```

**GET /api/v1/feeds** — List all feeds. Admin-only. Returns array of feed objects without `feed_token`.

**GET /api/v1/feeds/:feed_token** — Get feed + recent episodes. Public (token is the auth).

Response `200`:
```json
{
  "id": "uuid",
  "slug": "ml-papers",
  "title": "ML Papers",
  "rss_url": "https://yourapp.fly.dev/feed/uuid/rss.xml",
  "episodes": [ /* see episode shape below */ ]
}
```

### 5.2 Episodes

```
POST   /api/v1/feeds/:feed_token/episodes
GET    /api/v1/feeds/:feed_token/episodes/:episode_id
DELETE /api/v1/feeds/:feed_token/episodes/:episode_id
POST   /api/v1/feeds/:feed_token/episodes/:episode_id/retry
```

**POST /api/v1/feeds/:feed_token/episodes** — Submit a URL for processing.

Request:
```json
{
  "url": "https://arxiv.org/abs/2301.07041",
  "tts_provider": "elevenlabs"   // optional, falls back to feed default
}
```

Response `202`:
```json
{
  "id": "uuid",
  "status": "pending",
  "source_url": "https://arxiv.org/abs/2301.07041",
  "source_type": "arxiv"
}
```

**GET /api/v1/feeds/:feed_token/episodes/:episode_id** — Poll for status.

Episode shape:
```json
{
  "id": "uuid",
  "title": "Scaling Laws for Neural Language Models",
  "source_url": "https://arxiv.org/abs/2001.08361",
  "source_type": "arxiv",
  "status": "done",
  "audio_url": "https://bucket.t3.tigrisfiles.io/episodes/uuid/abc123.mp3",
  "duration_secs": 1842,
  "tts_provider": "elevenlabs",
  "error_msg": null,
  "pub_date": "2024-01-15T10:30:00Z",
  "created_at": "2024-01-15T10:25:00Z"
}
```

**POST /api/v1/feeds/:feed_token/episodes/:episode_id/retry** — Re-queue a failed episode from its last successful stage.

### 5.3 RSS

```
GET /feed/:feed_token/rss.xml
```

No `/api/v1` prefix — this is a plain RSS endpoint consumed directly by podcast clients.

Returns `Content-Type: application/rss+xml; charset=utf-8`.

Queries `episodes WHERE feed_id = $1 AND status = 'done' ORDER BY pub_date DESC LIMIT 50`.

RSS format: standard RSS 2.0 with `<enclosure>` tags. See Section 8 for exact XML structure.

---

## 6. Pipeline

Each episode moves through three sequential stages. Each stage is a separate job in the `jobs` table. The worker completes one stage, inserts the next job, and updates the episode status — all in a single transaction.

```
submitted → [scrape job] → [clean job] → [tts job] → done
```

### 6.1 Source Detection

On episode submission, detect `source_type` from the URL before inserting into DB:

- URL matches `arxiv.org/abs/` or `ar5iv.org` → `arxiv`
- Everything else → `article`

Extract arXiv ID from URL if applicable (e.g. `2301.07041` from `https://arxiv.org/abs/2301.07041`).

### 6.2 Scrape Stage

**Article path:**

1. HTTP GET the URL with a realistic `User-Agent` header and 30s timeout.
2. Extract readable text using `readability-rs` crate. This strips navigation, ads, footers, leaving title + body.
3. Store result in `episode.raw_text`.
4. If fetch fails (non-2xx, timeout, parse error) → fail the job.

**arXiv path:**

Do NOT fetch the PDF. Instead:

1. Fetch metadata from arXiv API: `https://export.arxiv.org/api/query?id_list={arxiv_id}`
   - Parse title and authors from Atom XML response.
2. Fetch the HTML rendering from ar5iv: `https://ar5iv.org/abs/{arxiv_id}`
   - ar5iv converts LaTeX source to clean HTML — far superior to PDF text extraction.
   - Apply readability extraction same as article path.
3. Store in `episode.raw_text`. Title is from arXiv API, not page title.

**Crate:** `reqwest` with `rustls-tls` feature (no OpenSSL dependency in Docker).

### 6.3 Clean Stage

Call the Claude API (`claude-opus-4-6` model) with a source-type-specific prompt.

**Article system prompt:**
```
You are preparing a web article for text-to-speech conversion. 
Transform the provided text so it reads naturally when spoken aloud.

Rules:
- Remove any remaining navigation text, share buttons, author bios, 
  newsletter signup prompts, or other non-article content.
- Fix encoding artifacts (curly quotes, em-dashes are fine; fix broken UTF-8).
- Keep the article's natural structure and flow.
- Do not summarize or omit any article content.
- Do not add commentary.
- Output only the cleaned article text, nothing else.
```

**arXiv system prompt:**
```
You are preparing an academic paper for text-to-speech conversion.
Transform the provided text so it reads naturally when spoken aloud.

Rules:
- Remove all citation markers: [1], [23], (Smith et al., 2019), etc.
- Remove figure and table references: "as shown in Figure 3", "see Table 1" → omit entirely.
- Rewrite inline equations as spoken English:
    \frac{a}{b} → "a over b"
    x^2 → "x squared"  
    \sum_{i=1}^{n} → "the sum from i equals 1 to n of"
    For complex equations, describe what they compute rather than reading symbol-by-symbol.
- Expand abbreviations on first use if the expansion aids comprehension.
- Replace "in the next section" / "as mentioned above" with brief inline context.
- Remove any LaTeX artifacts, section numbering (e.g. "3.2 Method"), footnote markers.
- Keep all substantive content — do not summarize or omit findings, methods, or discussion.
- Output only the cleaned paper text, nothing else.
```

**API call parameters:**
- Model: `claude-opus-4-6`  
- Max tokens: 8192  
- Temperature: 0 (deterministic, not creative)
- Single user message containing `raw_text`

Store response in `episode.cleaned_text`.

**Cost note:** At ~100K chars average paper, this is ~$1.50/paper with Opus. Consider `claude-sonnet-4-6` for articles (cheaper, sufficient quality) and Opus only for arXiv.

### 6.4 TTS Stage

**Chunking:**

Split `cleaned_text` into chunks before sending to TTS. Respect sentence boundaries — never split mid-sentence.

```rust
fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    // Split on ". " or ".\n", accumulate until max_chars,
    // never exceed max_chars per chunk.
    // Typical max_chars: 4000 for OpenAI, 4000 for ElevenLabs Flash.
}
```

**OpenAI TTS:**

- Endpoint: `POST https://api.openai.com/v1/audio/speech`
- Model: `tts-1-hd`
- Voice: `onyx` (deep, good for narration — make configurable via env)
- Response format: `mp3`
- Process chunks sequentially; collect `Vec<Bytes>`.

**ElevenLabs TTS:**

- Endpoint: `POST https://api.elevenlabs.io/v1/text-to-speech/{voice_id}`
- Model: `eleven_flash_v2_5` (best quality/speed for long-form)
- Voice ID: from `ELEVENLABS_VOICE_ID` env var
- Request body: `{"text": "...", "model_id": "eleven_flash_v2_5"}`
- Response: raw MP3 bytes
- Process chunks sequentially; collect `Vec<Bytes>`.

**Stitching:**

Concatenate all MP3 chunk bytes directly — MP3 frames are self-delimiting and concatenation produces valid audio. No ffmpeg needed for basic stitching.

```rust
let audio: Bytes = chunks.into_iter().flatten().collect();
```

**Duration estimation:**

After stitching, estimate duration without decoding:
- OpenAI tts-1-hd at standard rate: ~150 words/minute
- Count words in `cleaned_text`, divide by 150, convert to seconds.
- Store as `episode.duration_secs`.

(Exact duration via MP3 frame parsing is optional — word-count estimate is fine for RSS.)

**Upload:**

Call `StorageClient::upload_episode_audio(episode_id, audio_bytes)` (see Section 9).

Store returned URL in `episode.audio_url`.

### 6.5 Error Handling & Retry

On any stage error:

1. Increment `jobs.attempts`.
2. If `attempts < MAX_JOB_ATTEMPTS`: set `status = 'queued'`, set `run_after = NOW() + (2^attempts * 60 seconds)` (exponential backoff: 1min, 2min, 4min).
3. If `attempts >= MAX_JOB_ATTEMPTS`: set `job.status = 'error'`, set `episode.status = 'error'`, set `episode.error_msg` to the error description.

Always update episode status to reflect the current stage, even on failure.

The retry endpoint (`POST /episodes/:id/retry`) re-inserts a queued job for the failed stage, resets `episode.status` to the stage before the failed one, and clears `episode.error_msg`.

---

## 7. Worker

The worker runs as a `tokio::spawn`ed task inside the same binary as the Axum server. It is not a separate process.

```rust
pub async fn run_worker(pool: PgPool, config: AppConfig, storage: StorageClient) {
    loop {
        match claim_next_job(&pool).await {
            Ok(Some(job)) => {
                tokio::spawn(execute_job(job, pool.clone(), config.clone(), storage.clone()));
            }
            Ok(None) => {
                tokio::time::sleep(Duration::from_secs(config.worker_poll_interval)).await;
            }
            Err(e) => {
                tracing::error!("Worker poll error: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
```

**Job claiming** uses `SELECT FOR UPDATE SKIP LOCKED` to safely handle concurrent workers if the app is scaled to multiple instances:

```sql
SELECT j.id, j.episode_id, j.job_type, j.attempts
FROM jobs j
WHERE j.status = 'queued'
  AND j.run_after <= NOW()
ORDER BY j.created_at ASC
LIMIT 1
FOR UPDATE SKIP LOCKED
```

Immediately after claiming, update `job.status = 'running'` in the same transaction.

**Job execution:**

```rust
async fn execute_job(job: Job, pool: PgPool, config: AppConfig, storage: StorageClient) {
    let result = match job.job_type.as_str() {
        "scrape" => scrape::run(&job, &pool, &config).await,
        "clean"  => clean::run(&job, &pool, &config).await,
        "tts"    => tts::run(&job, &pool, &config, &storage).await,
        _        => Err(anyhow::anyhow!("Unknown job type")),
    };

    match result {
        Ok(_) => complete_job(&pool, &job).await,
        Err(e) => fail_job(&pool, &job, &e.to_string(), config.max_job_attempts).await,
    }
}
```

**Stage transitions** (in `complete_job`):

```
scrape done → insert clean job, update episode.status = 'cleaning'
clean done  → insert tts job,   update episode.status = 'tts'
tts done    → no new job,       update episode.status = 'done', episode.pub_date = NOW()
```

All three updates (job status, episode status, new job insert) happen in a single transaction.

---

## 8. RSS Feed Format

```xml
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>{feed.title}</title>
    <description>{feed.description}</description>
    <link>https://yourapp.fly.dev/feed/{feed.feed_token}/rss.xml</link>
    <language>en-us</language>
    <itunes:author>Personal Podcast</itunes:author>
    <itunes:category text="Technology"/>

    {for each episode}
    <item>
      <title>{episode.title}</title>
      <guid isPermaLink="false">{episode.id}</guid>
      <pubDate>{episode.pub_date in RFC 2822 format}</pubDate>
      <description>{episode.source_url}</description>
      <enclosure
        url="{episode.audio_url}"
        length="0"
        type="audio/mpeg"/>
      <itunes:duration>{episode.duration_secs}</itunes:duration>
    </item>
    {end for}

  </channel>
</rss>
```

Notes:
- `length` in `<enclosure>` is technically required by spec but podcast clients tolerate `0`. Exact byte length would require a DB column — not worth it.
- `<guid>` must be stable and unique — use the episode UUID.
- Generate XML via string templating (e.g. `askama` or simple `format!`) — no XML library needed for output this simple.
- Set `Cache-Control: max-age=300` on the RSS response (5 min) to allow podcast clients to poll frequently without hammering the DB.

---

## 9. Storage Module

Tigris is S3-compatible. Use `aws-sdk-s3` crate with a custom endpoint.

**Bucket configuration:** Public bucket. Audio files are served directly to podcast clients and RSS enclosures. Public URLs never expire (unlike presigned URLs), which is required for RSS compatibility.

**Public URL format:** `https://{BUCKET_NAME}.t3.tigrisfiles.io/{key}`

**Key format:** `episodes/{episode_id}/{sha256_prefix}.mp3`

Use the first 16 hex chars of the SHA-256 of the audio bytes as the filename. This gives content-addressable keys that avoid Tigris's aggressive caching (1 hour default `Cache-Control`) when audio is re-generated on retry.

**Client initialization:**

```rust
use aws_config::BehaviorVersion;
use aws_sdk_s3::{config::{Credentials, Region}, Client};

pub async fn build_s3_client(config: &AppConfig) -> Client {
    let creds = Credentials::new(
        &config.aws_access_key_id,
        &config.aws_secret_access_key,
        None, None, "env"
    );
    let aws_config = aws_config::defaults(BehaviorVersion::latest())
        .credentials_provider(creds)
        .region(Region::new("auto"))
        .endpoint_url(&config.aws_endpoint_url_s3)
        .load()
        .await;
    Client::new(&aws_config)
}
```

**Upload:**

```rust
pub async fn upload_episode_audio(
    client: &Client,
    bucket: &str,
    episode_id: Uuid,
    audio_bytes: Bytes,
) -> Result<String> {
    let hash = hex::encode(&sha2::Sha256::digest(&audio_bytes)[..8]);
    let key = format!("episodes/{}/{}.mp3", episode_id, hash);

    client.put_object()
        .bucket(bucket)
        .key(&key)
        .body(ByteStream::from(audio_bytes))
        .content_type("audio/mpeg")
        .cache_control("public, max-age=31536000, immutable")
        .send()
        .await?;

    Ok(format!("https://{}.t3.tigrisfiles.io/{}", bucket, key))
}
```

**Delete** (used in retry flow when re-generating audio):

```rust
pub async fn delete_object(client: &Client, bucket: &str, url: &str) -> Result<()> {
    // Strip public URL prefix to get key
    let prefix = format!("https://{}.t3.tigrisfiles.io/", bucket);
    let key = url.strip_prefix(&prefix).unwrap_or(url);
    client.delete_object().bucket(bucket).key(key).send().await?;
    Ok(())
}
```

---

## 10. Frontend (Next.js)

The frontend is a thin client over the Axum API. Deploy to Vercel; configure `NEXT_PUBLIC_API_BASE_URL` to point to the Fly.io backend.

### Pages

**`/` — Feed list**

- Lists all feeds (calls `GET /api/v1/feeds` with admin token from env).
- Shows feed title, slug, episode count, RSS URL (copyable).
- Button to create new feed (opens a small form).

**`/feeds/[token]` — Feed view**

- Shows feed title and RSS URL copy button.
- URL submission form: text input + TTS provider selector (OpenAI / ElevenLabs) + Submit.
- On submit: `POST /api/v1/feeds/:token/episodes`, optimistically add episode to list with `pending` status.
- Episode list: title (or URL if no title yet), status badge, created time.
- Status badges: `pending` (gray) · `scraping/cleaning/tts` (yellow, animated) · `done` (green) · `error` (red with error message).
- Poll in-progress episodes every 5 seconds via `setInterval` until `done` or `error`.
- Done episodes show a play button (HTML `<audio>` tag) and duration.

**`/feeds/[token]/episodes/[id]` — Episode detail**

- Full episode info: title, source URL, TTS provider, status, error message if any.
- Audio player.
- Retry button (only shown when `status = 'error'`).

### API Client (`lib/api.ts`)

Typed wrappers over `fetch`. Admin token stored in `ADMIN_TOKEN` env var (server-side only, used in Server Components or API routes — never exposed to client). Feed token comes from the URL.

---

## 11. Fly.io Deployment

### `Dockerfile`

```dockerfile
# Build stage
FROM rust:1.77-slim as builder
WORKDIR /app
COPY backend/ .
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/backend /usr/local/bin/backend
CMD ["backend"]
```

Use `debian:bookworm-slim` not `scratch` — `reqwest` with `rustls` needs CA certificates for HTTPS.

### `fly.toml`

```toml
app = "personal-podcast"
primary_region = "sjc"

[build]
  dockerfile = "Dockerfile"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = true
  auto_start_machines = true

[[vm]]
  memory = "512mb"
  cpu_kind = "shared"
  cpus = 1
```

### Secrets

```bash
fly secrets set \
  DATABASE_URL="..." \
  ANTHROPIC_API_KEY="..." \
  OPENAI_API_KEY="..." \
  ELEVENLABS_API_KEY="..." \
  ELEVENLABS_VOICE_ID="..." \
  ADMIN_TOKEN="$(openssl rand -hex 32)"
```

Tigris secrets are set automatically by `fly storage create`.

### Database

```bash
fly postgres create --name personal-podcast-db --region sjc --initial-cluster-size 1
fly postgres attach personal-podcast-db
```

Run migrations at startup using `sqlx::migrate!()` macro — no separate migration step needed.

### Storage

```bash
fly storage create --public
# Sets: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY,
#       AWS_ENDPOINT_URL_S3, BUCKET_NAME, AWS_REGION
```

---

## 12. Key Crates

```toml
[dependencies]
# Web framework
axum = { version = "0.7", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace"] }

# Database
sqlx = { version = "0.7", features = ["postgres", "runtime-tokio-rustls", "uuid", "time", "migrate"] }

# HTTP client
reqwest = { version = "0.12", features = ["rustls-tls", "json"], default-features = false }

# S3 / Tigris
aws-config = { version = "1", features = ["behavior-version-latest"] }
aws-sdk-s3 = "1"
aws-credential-types = "1"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# IDs
uuid = { version = "1", features = ["v4", "serde"] }

# Time
time = { version = "0.3", features = ["serde"] }

# Error handling
anyhow = "1"
thiserror = "1"

# Crypto (for content-addressed storage keys)
sha2 = "0.10"
hex = "0.4"

# HTML extraction (article readability)
readability = "0.3"   # readability-rs

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

---

## 13. Implementation Order

Build in this order so there's a deployable artifact at each step:

1. **Schema + migrations** — Write SQL, verify with `sqlx-cli`.
2. **Config + AppState** — Load env vars, build `AppState` with pool + storage client.
3. **Feed CRUD routes + RSS endpoint** — Deploy to Fly. RSS returns empty feed. Verify with a podcast client.
4. **Episode submission route** — Accepts URL, inserts episode + first job, returns 202.
5. **Scrape stage** — Implement article and arXiv paths. Test locally with known URLs.
6. **Worker loop** — Wire up job polling and stage transitions. Test scrape end-to-end.
7. **Clean stage** — Claude API integration. Test with sample raw text.
8. **TTS stage (OpenAI first)** — Chunking, API call, stitching, upload to Tigris.
9. **TTS stage (ElevenLabs)** — Add second provider behind the same interface.
10. **Error handling + retry** — Backoff logic, retry endpoint.
11. **Next.js frontend** — Feed list, feed view with polling, episode detail.
12. **End-to-end test** — Submit an arXiv URL, wait for done, add RSS to Overcast.

---

## 14. Open Questions / Future Work

These are explicitly out of scope for the initial implementation:

- **User authentication** — Currently any holder of a feed token can submit episodes. Auth can be added later with Clerk if needed.
- **Arbitrary PDF support** — Scoped out. arXiv is handled via ar5iv HTML; other PDFs are not.
- **Two-voice dialogue format** — ElevenLabs supports multi-speaker; could make dense papers more listenable.
- **Episode length limits** — No cap currently. A 100-page paper could generate very long audio; consider chunking into multiple episodes.
- **Usage tracking** — No analytics on RSS consumption. Could add request logging on the RSS endpoint.
- **Feed artwork** — RSS supports `<itunes:image>`. Not implemented; podcast clients will show a blank cover.
- **Exact MP3 duration** — Currently estimated from word count. Exact duration requires parsing MP3 frame headers.
