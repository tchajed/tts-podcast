use anyhow::{Context, Result};

use crate::config::AppConfig;

pub async fn run(
    episode_id: &str,
    pool: &sqlx::SqlitePool,
    _config: &AppConfig,
) -> Result<()> {
    let (source_url, source_type) = sqlx::query_as::<_, (Option<String>, String)>(
        "SELECT source_url, source_type FROM episodes WHERE id = $1",
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await?;

    let source_url = source_url.context("No source_url for scrape stage")?;

    let doc = tts_lib::scrape::scrape(&source_url, &source_type).await?;

    let title = doc.title.as_deref().unwrap_or(&source_url);
    let raw_text = doc
        .raw_text
        .as_ref()
        .context("No text extracted from URL")?;

    sqlx::query("UPDATE episodes SET title = $1, raw_text = $2 WHERE id = $3")
        .bind(title)
        .bind(raw_text)
        .bind(episode_id)
        .execute(pool)
        .await?;

    Ok(())
}
