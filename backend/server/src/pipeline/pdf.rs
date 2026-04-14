use anyhow::Result;

use crate::config::AppConfig;

pub async fn run(
    episode_id: &str,
    pool: &sqlx::SqlitePool,
    config: &AppConfig,
) -> Result<()> {
    let pdf_path = format!("/data/{}.pdf", episode_id);

    let doc = match config.pdf_extractor.as_str() {
        "gemini" => tts_lib::pdf_gemini::extract(&pdf_path, &config.google_studio_api_key).await?,
        _ => tts_lib::pdf::extract(&pdf_path, &config.anthropic_api_key).await?,
    };

    let title = doc.title.as_deref().unwrap_or("Untitled PDF");
    let raw_text = doc
        .raw_text
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No text extracted from PDF"))?;

    // Update title only if still "PDF Upload"
    let current_title =
        sqlx::query_scalar::<_, String>("SELECT title FROM episodes WHERE id = $1")
            .bind(episode_id)
            .fetch_one(pool)
            .await?;

    let final_title = if current_title == "PDF Upload" {
        title
    } else {
        &current_title
    };

    sqlx::query("UPDATE episodes SET title = $1, raw_text = $2 WHERE id = $3")
        .bind(final_title)
        .bind(raw_text)
        .bind(episode_id)
        .execute(pool)
        .await?;

    // Clean up the PDF file (page dir already cleaned by tts-lib)
    let _ = tokio::fs::remove_file(&pdf_path).await;

    Ok(())
}
