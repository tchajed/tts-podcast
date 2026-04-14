use anyhow::{Context, Result};
use base64::Engine;
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const TTS_CONCURRENCY: usize = 4;

/// TTS configuration.
pub struct TtsConfig {
    pub google_api_key: String,
    pub voice: String,
}

impl TtsConfig {
    pub fn new(google_api_key: String) -> Self {
        Self {
            google_api_key,
            voice: "en-US-Journey-D".to_string(),
        }
    }

    pub fn with_voice(mut self, voice: String) -> Self {
        self.voice = voice;
        self
    }
}

/// Result of TTS synthesis.
pub struct TtsResult {
    pub audio: Bytes,
    pub duration_secs: u32,
    pub chunks_total: usize,
}

/// Progress callback called after each chunk completes.
pub type ProgressCallback = Arc<dyn Fn(usize, usize) + Send + Sync>;

/// Synthesize text to MP3 audio using Google Cloud TTS.
pub async fn synthesize(
    text: &str,
    config: &TtsConfig,
    on_progress: Option<ProgressCallback>,
) -> Result<TtsResult> {
    let chunks = chunk_text(text, 4000);
    let total_chunks = chunks.len();
    let word_count = text.split_whitespace().count();

    tracing::info!(
        "TTS starting: {word_count} words, {total_chunks} chunks (~{:.0}s estimated)",
        word_count as f64 * 0.13
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let completed = Arc::new(AtomicUsize::new(0));
    let on_progress = on_progress.clone();

    let results: Vec<Result<(usize, Bytes)>> = stream::iter(chunks.into_iter().enumerate())
        .map(|(i, chunk)| {
            let client = client.clone();
            let completed = completed.clone();
            let on_progress = on_progress.clone();
            async move {
                let chunk_words = chunk.split_whitespace().count();
                tracing::info!("TTS chunk {}/{} ({chunk_words} words)", i + 1, total_chunks);
                let audio = tts_google(&client, config, &chunk).await?;
                let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(ref cb) = on_progress {
                    cb(done, total_chunks);
                }
                Ok::<_, anyhow::Error>((i, audio))
            }
        })
        .buffer_unordered(TTS_CONCURRENCY)
        .collect()
        .await;

    let mut indexed: Vec<(usize, Bytes)> = results.into_iter().collect::<Result<_>>()?;
    indexed.sort_by_key(|(i, _)| *i);
    let audio_parts: Vec<Bytes> = indexed.into_iter().map(|(_, b)| b).collect();

    // Concatenate MP3 chunks
    let total_bytes: Vec<u8> = audio_parts.iter().flat_map(|b| b.to_vec()).collect();
    let audio = Bytes::from(total_bytes);

    let duration_secs = mp3_duration::from_read(&mut std::io::Cursor::new(&audio[..]))
        .map(|d| d.as_secs() as u32)
        .unwrap_or_else(|_| (word_count as f64 / 150.0 * 60.0) as u32);

    Ok(TtsResult {
        audio,
        duration_secs,
        chunks_total: total_chunks,
    })
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
            sentences.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }
    sentences
}

async fn tts_google(
    client: &reqwest::Client,
    config: &TtsConfig,
    text: &str,
) -> Result<Bytes> {
    let url = format!(
        "https://texttospeech.googleapis.com/v1/text:synthesize?key={}",
        config.google_api_key
    );

    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "input": { "text": text },
            "voice": {
                "languageCode": "en-US",
                "name": config.voice,
            },
            "audioConfig": { "audioEncoding": "MP3" },
        }))
        .send()
        .await?
        .error_for_status()
        .context("Google TTS request failed")?;

    let body: serde_json::Value = resp.json().await?;
    let audio_b64 = body["audioContent"]
        .as_str()
        .context("No audioContent in Google TTS response")?;

    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_b64)
        .context("Failed to decode Google TTS audio")?;

    Ok(Bytes::from(audio_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_sentences_basic() {
        let result = split_sentences("Hello world. How are you? Fine!");
        assert_eq!(result, vec!["Hello world.", " How are you?", " Fine!"]);
    }

    #[test]
    fn test_split_sentences_no_punctuation() {
        let result = split_sentences("No ending punctuation");
        assert_eq!(result, vec!["No ending punctuation"]);
    }

    #[test]
    fn test_chunk_text_single_chunk() {
        let text = "Short text.";
        let chunks = chunk_text(text, 100);
        assert_eq!(chunks, vec!["Short text."]);
    }

    #[test]
    fn test_chunk_text_splits_on_sentence_boundary() {
        let text = "First sentence. Second sentence. Third sentence.";
        let chunks = chunk_text(text, 20);
        assert_eq!(chunks.len(), 3);
    }
}
