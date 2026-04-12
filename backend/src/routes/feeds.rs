use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/feeds", post(create_feed).get(list_feeds))
        .route(
            "/api/v1/feeds/{feed_token}",
            get(get_feed).delete(delete_feed),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateFeedRequest {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_tts")]
    pub tts_default: String,
}

fn default_tts() -> String {
    "openai".into()
}

#[derive(Debug, Serialize, FromRow)]
pub struct FeedRow {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub feed_token: Uuid,
    pub tts_default: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
pub struct FeedResponse {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub feed_token: Uuid,
    pub tts_default: String,
    pub rss_url: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, FromRow)]
pub struct FeedListItem {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub tts_default: String,
    pub created_at: OffsetDateTime,
    pub episode_count: i64,
}

fn require_admin(headers: &HeaderMap, admin_token: &str) -> AppResult<()> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token != admin_token {
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

async fn create_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateFeedRequest>,
) -> AppResult<(StatusCode, Json<FeedResponse>)> {
    require_admin(&headers, &state.config.admin_token)?;

    if !matches!(req.tts_default.as_str(), "openai" | "elevenlabs") {
        return Err(AppError::BadRequest(
            "tts_default must be 'openai' or 'elevenlabs'".into(),
        ));
    }

    let row = sqlx::query_as::<_, FeedRow>(
        "INSERT INTO feeds (slug, title, description, tts_default)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(&req.slug)
    .bind(&req.title)
    .bind(&req.description)
    .bind(&req.tts_default)
    .fetch_one(&state.pool)
    .await?;

    let rss_url = format!("{}/feed/{}/rss.xml", state.config.public_url, row.feed_token);

    Ok((
        StatusCode::CREATED,
        Json(FeedResponse {
            id: row.id,
            slug: row.slug,
            title: row.title,
            description: row.description,
            feed_token: row.feed_token,
            tts_default: row.tts_default,
            rss_url,
            created_at: row.created_at,
        }),
    ))
}

async fn list_feeds(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<FeedListItem>>> {
    require_admin(&headers, &state.config.admin_token)?;

    let feeds = sqlx::query_as::<_, FeedListItem>(
        "SELECT f.id, f.slug, f.title, f.description, f.tts_default, f.created_at,
                COUNT(e.id) as episode_count
         FROM feeds f
         LEFT JOIN episodes e ON e.feed_id = f.id
         GROUP BY f.id
         ORDER BY f.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(feeds))
}

#[derive(Debug, Serialize)]
pub struct FeedWithEpisodes {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub tts_default: String,
    pub rss_url: String,
    pub episodes: Vec<EpisodeSummary>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct EpisodeSummary {
    pub id: Uuid,
    pub title: String,
    pub source_url: String,
    pub source_type: String,
    pub status: String,
    pub audio_url: Option<String>,
    pub duration_secs: Option<i32>,
    pub tts_provider: Option<String>,
    pub error_msg: Option<String>,
    pub pub_date: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

async fn get_feed(
    State(state): State<AppState>,
    Path(feed_token): Path<Uuid>,
) -> AppResult<Json<FeedWithEpisodes>> {
    let feed = sqlx::query_as::<_, FeedRow>("SELECT * FROM feeds WHERE feed_token = $1")
        .bind(feed_token)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)?;

    let episodes = sqlx::query_as::<_, EpisodeSummary>(
        "SELECT id, title, source_url, source_type, status, audio_url,
                duration_secs, tts_provider, error_msg, pub_date, created_at
         FROM episodes WHERE feed_id = $1
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .bind(feed.id)
    .fetch_all(&state.pool)
    .await?;

    let rss_url = format!("{}/feed/{}/rss.xml", state.config.public_url, feed.feed_token);

    Ok(Json(FeedWithEpisodes {
        id: feed.id,
        slug: feed.slug,
        title: feed.title,
        description: feed.description,
        tts_default: feed.tts_default,
        rss_url,
        episodes,
    }))
}

async fn delete_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(feed_token): Path<Uuid>,
) -> AppResult<StatusCode> {
    require_admin(&headers, &state.config.admin_token)?;

    let result = sqlx::query("DELETE FROM feeds WHERE feed_token = $1")
        .bind(feed_token)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}
