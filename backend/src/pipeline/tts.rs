use anyhow::{Context, Result};
use bytes::Bytes;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::pipeline::storage::StorageClient;

pub async fn run(
    episode_id: Uuid,
    pool: &sqlx::PgPool,
    config: &AppConfig,
    storage: &StorageClient,
) -> Result<()> {
    let (cleaned_text, tts_provider) = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT cleaned_text, tts_provider FROM episodes WHERE id = $1",
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await?;

    let cleaned_text = cleaned_text.context("No cleaned_text available for TTS")?;
    let provider = tts_provider.unwrap_or_else(|| "openai".into());

    let chunks = chunk_text(&cleaned_text, 4000);
    let mut audio_parts: Vec<Bytes> = Vec::new();

    let client = reqwest::Client::new();

    for chunk in &chunks {
        let audio = match provider.as_str() {
            "elevenlabs" => tts_elevenlabs(&client, config, chunk).await?,
            _ => tts_openai(&client, config, chunk).await?,
        };
        audio_parts.push(audio);
    }

    // Concatenate MP3 chunks
    let total_bytes: Vec<u8> = audio_parts.iter().flat_map(|b| b.to_vec()).collect();
    let audio = Bytes::from(total_bytes);

    // Estimate duration from word count
    let word_count = cleaned_text.split_whitespace().count();
    let duration_secs = (word_count as f64 / 150.0 * 60.0) as i32;

    // Upload to storage
    let audio_url = storage.upload_episode_audio(episode_id, audio).await?;

    sqlx::query(
        "UPDATE episodes SET audio_url = $1, duration_secs = $2 WHERE id = $3",
    )
    .bind(&audio_url)
    .bind(duration_secs)
    .bind(episode_id)
    .execute(pool)
    .await?;

    Ok(())
}

fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for sentence in split_sentences(text) {
        if current.len() + sentence.len() > max_chars && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }
        current.push_str(&sentence);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(text.to_string());
    }
    chunks
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if (ch == '.' || ch == '!' || ch == '?') && current.len() > 1 {
            // Peek: if next char would be a space or newline, this is a sentence boundary
            sentences.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }
    sentences
}

async fn tts_openai(client: &reqwest::Client, config: &AppConfig, text: &str) -> Result<Bytes> {
    let api_key = config
        .openai_api_key
        .as_ref()
        .context("OPENAI_API_KEY not set")?;

    let resp = client
        .post("https://api.openai.com/v1/audio/speech")
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": "tts-1-hd",
            "voice": config.openai_voice,
            "input": text,
            "response_format": "mp3",
        }))
        .send()
        .await?
        .error_for_status()
        .context("OpenAI TTS request failed")?;

    Ok(resp.bytes().await?)
}

async fn tts_elevenlabs(
    client: &reqwest::Client,
    config: &AppConfig,
    text: &str,
) -> Result<Bytes> {
    let api_key = config
        .elevenlabs_api_key
        .as_ref()
        .context("ELEVENLABS_API_KEY not set")?;

    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}",
        config.elevenlabs_voice_id
    );

    let resp = client
        .post(&url)
        .header("xi-api-key", api_key.as_str())
        .json(&serde_json::json!({
            "text": text,
            "model_id": "eleven_flash_v2_5",
        }))
        .send()
        .await?
        .error_for_status()
        .context("ElevenLabs TTS request failed")?;

    Ok(resp.bytes().await?)
}
