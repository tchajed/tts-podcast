use anyhow::{Context, Result};

use crate::config::AppConfig;

pub async fn run(
    episode_id: &str,
    pool: &sqlx::SqlitePool,
    config: &AppConfig,
) -> Result<()> {
    let cleaned_text = sqlx::query_scalar::<_, Option<String>>(
        "SELECT cleaned_text FROM episodes WHERE id = $1",
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await?;

    let cleaned_text = cleaned_text.context("No cleaned_text available for summarization")?;

    let input_doc = tts_lib::Document {
        cleaned_text: Some(cleaned_text),
        ..Default::default()
    };

    let provider = config.make_provider();
    let doc = tts_lib::summarize::summarize(&input_doc, &provider).await?;

    let transcript = doc
        .transcript
        .context("No transcript returned from summarization")?;
    let word_count = doc.word_count.unwrap_or(0) as i32;

    sqlx::query("UPDATE episodes SET transcript = $1, word_count = $2 WHERE id = $3")
        .bind(&transcript)
        .bind(word_count)
        .bind(episode_id)
        .execute(pool)
        .await?;

    Ok(())
}
