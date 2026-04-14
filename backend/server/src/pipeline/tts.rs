use anyhow::{Context, Result};

use crate::config::AppConfig;
use crate::pipeline::storage::StorageClient;

pub async fn run(
    episode_id: &str,
    pool: &sqlx::SqlitePool,
    config: &AppConfig,
    storage: &StorageClient,
) -> Result<()> {
    let (transcript, cleaned_text) = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT transcript, cleaned_text FROM episodes WHERE id = $1",
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await?;

    let tts_text = transcript
        .or(cleaned_text)
        .context("No text available for TTS")?;

    let tts_config = tts_lib::tts::TtsConfig::new(config.google_tts_api_key.clone())
        .with_voice(config.google_tts_voice.clone());

    // Set up progress tracking
    let pool_clone = pool.clone();
    let ep_id = episode_id.to_string();
    let on_progress: tts_lib::tts::ProgressCallback = std::sync::Arc::new(move |done, total| {
        let pool = pool_clone.clone();
        let ep_id = ep_id.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE episodes SET tts_chunks_done = $1, tts_chunks_total = $2 WHERE id = $3")
                .bind(done as i32)
                .bind(total as i32)
                .bind(&ep_id)
                .execute(&pool)
                .await;
        });
    });

    let result = tts_lib::tts::synthesize(&tts_text, &tts_config, Some(on_progress)).await?;

    // Upload to storage
    let audio_url = storage.upload_episode_audio(episode_id, result.audio).await?;

    sqlx::query("UPDATE episodes SET audio_url = $1, duration_secs = $2 WHERE id = $3")
        .bind(&audio_url)
        .bind(result.duration_secs as i32)
        .bind(episode_id)
        .execute(pool)
        .await?;

    Ok(())
}
