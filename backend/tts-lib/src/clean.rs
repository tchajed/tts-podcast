use anyhow::Result;

use crate::{Document, Provider};

const ARTICLE_SYSTEM_PROMPT: &str = r#"You are preparing a web article for text-to-speech conversion.
Transform the provided text so it reads naturally when spoken aloud.

Rules:
- Remove any remaining navigation text, share buttons, author bios,
  newsletter signup prompts, or other non-article content.
- Fix encoding artifacts. Curly quotes and em-dashes are fine.
- Keep the article's natural structure and flow.
- Do not summarize or omit any article content.
- Do not add commentary.
- Output only the cleaned article text, nothing else."#;

const ACADEMIC_SYSTEM_PROMPT: &str = r#"You are preparing an academic paper for text-to-speech conversion.
Transform the provided text so it reads naturally when spoken aloud.

Rules:
- Remove all citation markers: [1], [23], (Smith et al., 2019), etc.
- Remove figure and table references: "as shown in Figure 3" → omit entirely.
- Rewrite inline equations as spoken English:
    \frac{a}{b} → "a over b"
    x^2 → "x squared"
    \sum_{i=1}^{n} → "the sum from i equals 1 to n of"
    For complex equations, describe what they compute rather than reading symbol-by-symbol.
- Expand abbreviations on first use if the expansion aids comprehension.
- Replace "in the next section" / "as mentioned above" with brief inline context.
- Remove LaTeX artifacts, section numbering (e.g. "3.2 Method"), footnote markers.
- Omit the bibliography / references section entirely.
- Omit appendices, supplementary material, and acknowledgments (everything after the conclusion).
- Keep all substantive content from the main body — do not summarize or omit findings, methods, or discussion.
- Output only the cleaned text, nothing else."#;

/// Clean raw text for TTS. Dispatches to the configured provider.
pub async fn clean(doc: &Document, provider: &Provider) -> Result<Document> {
    let raw_text = doc
        .raw_text
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No raw_text available for cleaning"))?;

    let system_prompt = match doc.source_type.as_str() {
        "arxiv" | "pdf" => ACADEMIC_SYSTEM_PROMPT,
        _ => ARTICLE_SYSTEM_PROMPT,
    };

    // For Claude, use Opus for academic content, Sonnet for articles.
    let claude_model = match doc.source_type.as_str() {
        "arxiv" | "pdf" => "claude-opus-4-6",
        _ => "claude-sonnet-4-6",
    };

    let client = reqwest::Client::new();
    let cleaned_text = provider
        .chat(&client, claude_model, Some(system_prompt), raw_text, 8192)
        .await?;

    let word_count = cleaned_text.split_whitespace().count();
    tracing::info!("Cleaning complete: {word_count} words");

    Ok(Document {
        cleaned_text: Some(cleaned_text),
        word_count: Some(word_count),
        ..doc.clone()
    })
}
