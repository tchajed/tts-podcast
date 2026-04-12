use anyhow::{Context, Result};
use reqwest::Client;
use url::Url;
use uuid::Uuid;

use crate::config::AppConfig;

pub async fn run(
    episode_id: Uuid,
    pool: &sqlx::PgPool,
    _config: &AppConfig,
) -> Result<()> {
    let (source_url, source_type) = sqlx::query_as::<_, (String, String)>(
        "SELECT source_url, source_type FROM episodes WHERE id = $1",
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await?;

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let (title, raw_text) = match source_type.as_str() {
        "arxiv" => scrape_arxiv(&client, &source_url).await?,
        _ => scrape_article(&client, &source_url).await?,
    };

    sqlx::query("UPDATE episodes SET title = $1, raw_text = $2 WHERE id = $3")
        .bind(&title)
        .bind(&raw_text)
        .bind(episode_id)
        .execute(pool)
        .await?;

    Ok(())
}

fn extract_readable(html: &str, url_str: &str) -> Result<(String, String)> {
    let url = Url::parse(url_str).context("Invalid URL")?;
    let mut cursor = std::io::Cursor::new(html.as_bytes());
    let product = readability::extractor::extract(&mut cursor, &url)
        .map_err(|e| anyhow::anyhow!("Readability extraction failed: {:?}", e))?;

    let title = if product.title.is_empty() {
        url_str.to_string()
    } else {
        product.title
    };

    Ok((title, product.text))
}

async fn scrape_article(client: &Client, url: &str) -> Result<(String, String)> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let html = resp.text().await?;
    extract_readable(&html, url)
}

async fn scrape_arxiv(client: &Client, url: &str) -> Result<(String, String)> {
    let arxiv_id = extract_arxiv_id(url)
        .context("Could not extract arXiv ID from URL")?;

    // Fetch metadata from arXiv API
    let api_url = format!("https://export.arxiv.org/api/query?id_list={arxiv_id}");
    let api_resp = client.get(&api_url).send().await?.error_for_status()?;
    let api_xml = api_resp.text().await?;

    let title = parse_arxiv_title(&api_xml).unwrap_or_else(|| format!("arXiv:{arxiv_id}"));

    // Fetch HTML from ar5iv
    let ar5iv_url = format!("https://ar5iv.org/abs/{arxiv_id}");
    let html_resp = client.get(&ar5iv_url).send().await?.error_for_status()?;
    let html = html_resp.text().await?;

    let (_extracted_title, text) = extract_readable(&html, &ar5iv_url)?;

    Ok((title, text))
}

fn extract_arxiv_id(url: &str) -> Option<String> {
    let patterns = ["arxiv.org/abs/", "ar5iv.org/abs/"];
    for pat in patterns {
        if let Some(idx) = url.find(pat) {
            let rest = &url[idx + pat.len()..];
            let id: String = rest.chars().take_while(|c| *c != '/' && *c != '?').collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn parse_arxiv_title(xml: &str) -> Option<String> {
    // Simple extraction — find <title> inside <entry>
    let entry_start = xml.find("<entry>")?;
    let after_entry = &xml[entry_start..];
    let title_start = after_entry.find("<title>")? + 7;
    let title_end = after_entry[title_start..].find("</title>")?;
    let title = &after_entry[title_start..title_start + title_end];
    Some(title.trim().replace('\n', " "))
}
