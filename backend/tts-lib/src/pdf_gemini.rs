use anyhow::{Context, Result};
use base64::Engine;

use crate::Document;

const PDF_PROMPT: &str = r#"Extract all text from this academic paper for text-to-speech conversion.

Rules:
- Extract all text content in reading order (top-to-bottom, respecting column layout).
- For two-column layouts, complete the left column before the right column.
- Ignore page numbers, headers, footers, and running titles.
- Ignore figure captions and table captions — replace with "[Figure omitted]" or "[Table omitted]".
- Preserve paragraph breaks between pages.
- Output only the extracted text, nothing else.

At the very start of your output, on the first line only, write the paper's title prefixed with "TITLE: ". Then a blank line, then the full extracted text."#;

pub const DEFAULT_MODEL: &str = "gemini-flash-latest";

/// Extract text from a PDF using Gemini in a single API call (no page splitting).
/// Gemini accepts the entire PDF inline as base64 data.
/// Returns a Document with title and raw_text populated.
pub async fn extract(pdf_path: &str, google_api_key: &str) -> Result<Document> {
    extract_with_model(pdf_path, google_api_key, DEFAULT_MODEL).await
}

pub async fn extract_with_model(
    pdf_path: &str,
    google_api_key: &str,
    model: &str,
) -> Result<Document> {
    let pdf_bytes = tokio::fs::read(pdf_path).await
        .with_context(|| format!("Failed to read PDF file {pdf_path}"))?;

    let size_mb = pdf_bytes.len() as f64 / (1024.0 * 1024.0);
    tracing::info!("Extracting PDF via Gemini ({:.2} MB)", size_mb);

    if pdf_bytes.len() > 50 * 1024 * 1024 {
        anyhow::bail!("PDF exceeds Gemini 50MB inline limit ({:.1} MB)", size_mb);
    }

    let pdf_b64 = base64::engine::general_purpose::STANDARD.encode(&pdf_bytes);

    let request = serde_json::json!({
        "contents": [{
            "parts": [
                {
                    "inline_data": {
                        "mime_type": "application/pdf",
                        "data": pdf_b64,
                    }
                },
                { "text": PDF_PROMPT }
            ]
        }],
        "generationConfig": {
            "temperature": 0.0,
            "maxOutputTokens": 65536,
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, google_api_key
    );

    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Gemini API failed ({status}): {body}");
    }

    let body: serde_json::Value = resp.json().await?;

    // Extract text from response. Gemini can return multiple parts.
    let parts = body["candidates"][0]["content"]["parts"]
        .as_array()
        .context("No parts in Gemini response")?;

    let full_text: String = parts
        .iter()
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");

    if full_text.is_empty() {
        let finish_reason = body["candidates"][0]["finishReason"].as_str().unwrap_or("unknown");
        anyhow::bail!("Empty text extracted from PDF (finishReason={finish_reason})");
    }

    // Parse title if present in the "TITLE: ..." prefix
    let (title, raw_text) = parse_title_and_text(&full_text);

    tracing::info!("Gemini extracted {} chars", raw_text.len());

    Ok(Document {
        title: Some(title),
        source_type: "pdf".to_string(),
        raw_text: Some(raw_text),
        ..Default::default()
    })
}

fn parse_title_and_text(full: &str) -> (String, String) {
    if let Some(rest) = full.strip_prefix("TITLE: ") {
        if let Some(newline_idx) = rest.find('\n') {
            let title = rest[..newline_idx].trim().to_string();
            let text = rest[newline_idx..].trim_start().to_string();
            return (title, text);
        }
    }
    ("Untitled PDF".to_string(), full.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_title_and_text_with_prefix() {
        let input = "TITLE: Spanner: Google's Database\n\nAbstract\nThis paper describes...";
        let (title, text) = parse_title_and_text(input);
        assert_eq!(title, "Spanner: Google's Database");
        assert!(text.starts_with("Abstract"));
    }

    #[test]
    fn test_parse_title_and_text_without_prefix() {
        let input = "No title prefix here. Just content.";
        let (title, text) = parse_title_and_text(input);
        assert_eq!(title, "Untitled PDF");
        assert_eq!(text, input);
    }
}
