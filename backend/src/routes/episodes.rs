use axum::{
    extract::{Path, State},
    http::StatusCode,
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
        .route(
            "/api/v1/feeds/{feed_token}/episodes",
            post(submit_episode),
        )
        .route(
            "/api/v1/feeds/{feed_token}/episodes/{episode_id}",
            get(get_episode).delete(delete_episode),
        )
        .route(
            "/api/v1/feeds/{feed_token}/episodes/{episode_id}/retry",
            post(retry_episode),
        )
}

#[derive(Debug, Deserialize)]
pub struct SubmitEpisodeRequest {
    pub url: String,
    pub tts_provider: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubmitEpisodeResponse {
    pub id: Uuid,
    pub status: String,
    pub source_url: String,
    pub source_type: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct EpisodeResponse {
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

fn detect_source_type(url: &str) -> &'static str {
    if url.contains("arxiv.org/abs/") || url.contains("ar5iv.org") {
        "arxiv"
    } else {
        "article"
    }
}

fn extract_arxiv_id(url: &str) -> Option<String> {
    let patterns = ["arxiv.org/abs/", "ar5iv.org/abs/"];
    for pat in patterns {
        if let Some(idx) = url.find(pat) {
            let rest = &url[idx + pat.len()..];
            let id: String = rest.chars().take_while(|c| *c != '/' && *c != '?').collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

/// Resolve the feed ID from a feed_token, returning NotFound if invalid.
async fn resolve_feed(pool: &sqlx::PgPool, feed_token: Uuid) -> AppResult<Uuid> {
    let row = sqlx::query_scalar::<_, Uuid>("SELECT id FROM feeds WHERE feed_token = $1")
        .bind(feed_token)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row)
}

async fn submit_episode(
    State(state): State<AppState>,
    Path(feed_token): Path<Uuid>,
    Json(req): Json<SubmitEpisodeRequest>,
) -> AppResult<(StatusCode, Json<SubmitEpisodeResponse>)> {
    let feed_id = resolve_feed(&state.pool, feed_token).await?;

    let source_type = detect_source_type(&req.url);

    // Determine TTS provider
    let tts_provider = match &req.tts_provider {
        Some(p) if p == "openai" || p == "elevenlabs" => p.clone(),
        Some(p) => {
            return Err(AppError::BadRequest(format!(
                "Invalid tts_provider: {p}"
            )));
        }
        None => {
            let default = sqlx::query_scalar::<_, String>(
                "SELECT tts_default FROM feeds WHERE id = $1",
            )
            .bind(feed_id)
            .fetch_one(&state.pool)
            .await?;
            default
        }
    };

    // Derive an initial title from the URL
    let title = if source_type == "arxiv" {
        extract_arxiv_id(&req.url)
            .map(|id| format!("arXiv:{id}"))
            .unwrap_or_else(|| req.url.clone())
    } else {
        req.url.clone()
    };

    let mut tx = state.pool.begin().await?;

    let episode_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO episodes (feed_id, title, source_url, source_type, tts_provider, status)
         VALUES ($1, $2, $3, $4, $5, 'pending')
         RETURNING id",
    )
    .bind(feed_id)
    .bind(&title)
    .bind(&req.url)
    .bind(source_type)
    .bind(&tts_provider)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO jobs (episode_id, job_type, status)
         VALUES ($1, 'scrape', 'queued')",
    )
    .bind(episode_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmitEpisodeResponse {
            id: episode_id,
            status: "pending".into(),
            source_url: req.url,
            source_type: source_type.into(),
        }),
    ))
}

async fn get_episode(
    State(state): State<AppState>,
    Path((feed_token, episode_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<EpisodeResponse>> {
    let feed_id = resolve_feed(&state.pool, feed_token).await?;

    let ep = sqlx::query_as::<_, EpisodeResponse>(
        "SELECT id, title, source_url, source_type, status, audio_url,
                duration_secs, tts_provider, error_msg, pub_date, created_at
         FROM episodes WHERE id = $1 AND feed_id = $2",
    )
    .bind(episode_id)
    .bind(feed_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(ep))
}

async fn delete_episode(
    State(state): State<AppState>,
    Path((feed_token, episode_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    let feed_id = resolve_feed(&state.pool, feed_token).await?;

    let result = sqlx::query("DELETE FROM episodes WHERE id = $1 AND feed_id = $2")
        .bind(episode_id)
        .bind(feed_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn retry_episode(
    State(state): State<AppState>,
    Path((feed_token, episode_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    let feed_id = resolve_feed(&state.pool, feed_token).await?;

    let (status, _error_msg) = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, error_msg FROM episodes WHERE id = $1 AND feed_id = $2",
    )
    .bind(episode_id)
    .bind(feed_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if status != "error" {
        return Err(AppError::BadRequest(
            "Can only retry episodes in error state".into(),
        ));
    }

    let failed_job_type = sqlx::query_scalar::<_, String>(
        "SELECT job_type FROM jobs WHERE episode_id = $1 AND status = 'error'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(episode_id)
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or_else(|| "scrape".into());

    let new_status = match failed_job_type.as_str() {
        "scrape" => "pending",
        "clean" => "scraping",
        "tts" => "cleaning",
        _ => "pending",
    };

    let mut tx = state.pool.begin().await?;

    sqlx::query("UPDATE episodes SET status = $1, error_msg = NULL WHERE id = $2")
        .bind(new_status)
        .bind(episode_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO jobs (episode_id, job_type, status)
         VALUES ($1, $2, 'queued')",
    )
    .bind(episode_id)
    .bind(&failed_job_type)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(serde_json::json!({
        "id": episode_id,
        "status": new_status,
        "retrying_stage": failed_job_type,
    })))
}
