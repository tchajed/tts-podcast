use anyhow::{Context, Result};
use serde_json::json;

/// Default Gemini model for text tasks. `flash-latest` is the stable alias
/// that points to the current flash model and is less prone to 503s than
/// a pinned minor version.
pub const DEFAULT_MODEL: &str = "gemini-flash-latest";

/// Send a text-in / text-out request to Gemini.
pub async fn chat(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    system: Option<&str>,
    user_message: &str,
    max_output_tokens: u32,
) -> Result<String> {
    let mut body = json!({
        "contents": [{
            "parts": [{ "text": user_message }]
        }],
        "generationConfig": {
            "temperature": 0.0,
            "maxOutputTokens": max_output_tokens,
        }
    });

    if let Some(sys) = system {
        body["systemInstruction"] = json!({
            "parts": [{ "text": sys }]
        });
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );

    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Gemini API failed ({status}): {body}");
    }

    let body: serde_json::Value = resp.json().await?;
    let parts = body["candidates"][0]["content"]["parts"]
        .as_array()
        .context("No parts in Gemini response")?;

    let text: String = parts
        .iter()
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        let finish_reason = body["candidates"][0]["finishReason"].as_str().unwrap_or("unknown");
        anyhow::bail!("Empty response from Gemini (finishReason={finish_reason})");
    }

    Ok(text)
}
