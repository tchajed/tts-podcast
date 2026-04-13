use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

/// PDF extraction via Claude vision.
/// Reads pages as images (rendered externally or via pdfium), sends to Claude
/// for text extraction. For now, we read the raw PDF bytes and send them
/// directly to Claude as a document (Claude supports PDF input).
pub async fn run(
    episode_id: &str,
    pool: &sqlx::SqlitePool,
    config: &AppConfig,
) -> Result<()> {
    let pdf_path = format!("/tmp/{}.pdf", episode_id);

    let pdf_bytes = tokio::fs::read(&pdf_path)
        .await
        .context("Failed to read temp PDF file")?;

    let pdf_b64 = base64::engine::general_purpose::STANDARD.encode(&pdf_bytes);

    // Send PDF to Claude vision for text extraction
    let client = reqwest::Client::new();
    let request = ClaudeRequest {
        model: "claude-sonnet-4-6".to_string(),
        max_tokens: 8192,
        temperature: 0.0,
        system: PDF_SYSTEM_PROMPT.to_string(),
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: vec![
                ContentBlock::Image {
                    r#type: "image".to_string(),
                    source: ImageSource {
                        r#type: "base64".to_string(),
                        media_type: "application/pdf".to_string(),
                        data: pdf_b64,
                    },
                },
                ContentBlock::Text {
                    r#type: "text".to_string(),
                    text: "Extract all text from this PDF document.".to_string(),
                },
            ],
        }],
    };

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &config.anthropic_api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await?
        .error_for_status()
        .context("Claude API request failed for PDF extraction")?;

    let claude_resp: ClaudeResponse = resp.json().await?;
    let raw_text = claude_resp
        .content
        .iter()
        .map(|ResponseBlock::Text { text }| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    if raw_text.is_empty() {
        anyhow::bail!("Empty text extracted from PDF");
    }

    // Extract title if episode title is still "PDF Upload"
    let current_title =
        sqlx::query_scalar::<_, String>("SELECT title FROM episodes WHERE id = $1")
            .bind(episode_id)
            .fetch_one(pool)
            .await?;

    let title = if current_title == "PDF Upload" {
        extract_title_from_text(&client, config, &raw_text).await?
    } else {
        current_title
    };

    sqlx::query("UPDATE episodes SET title = $1, raw_text = $2 WHERE id = $3")
        .bind(&title)
        .bind(&raw_text)
        .bind(episode_id)
        .execute(pool)
        .await?;

    // Clean up temp PDF
    let _ = tokio::fs::remove_file(&pdf_path).await;

    Ok(())
}

async fn extract_title_from_text(
    client: &reqwest::Client,
    config: &AppConfig,
    text: &str,
) -> Result<String> {
    // Use first ~2000 chars
    let snippet: String = text.chars().take(2000).collect();

    let request = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 100,
        "temperature": 0.0,
        "messages": [{
            "role": "user",
            "content": format!(
                "What is the title of this document? Output only the title, nothing else.\n\n{}",
                snippet
            ),
        }],
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &config.anthropic_api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await?
        .error_for_status()?;

    let claude_resp: ClaudeResponse = resp.json().await?;
    let title = claude_resp
        .content
        .first()
        .map(|ResponseBlock::Text { text }| text.clone())
        .unwrap_or_else(|| "Untitled PDF".into());

    Ok(title.trim().to_string())
}

const PDF_SYSTEM_PROMPT: &str = r#"You are extracting text from a PDF document for text-to-speech conversion.

Rules:
- Extract all text content in reading order (top-to-bottom, respecting column layout).
- For two-column layouts, complete the left column before the right column.
- Ignore page numbers, headers, footers, and running titles.
- Ignore figure captions and table captions — replace with "[Figure omitted]" or "[Table omitted]".
- Preserve paragraph breaks.
- Output only the extracted text, nothing else."#;

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    system: String,
    messages: Vec<ClaudeMessage>,
}

#[derive(Serialize)]
struct ClaudeMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ContentBlock {
    Image {
        r#type: String,
        source: ImageSource,
    },
    Text {
        r#type: String,
        text: String,
    },
}

#[derive(Serialize)]
struct ImageSource {
    r#type: String,
    media_type: String,
    data: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ResponseBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
}
