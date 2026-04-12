use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::get,
    Router,
};
use sqlx::FromRow;

use crate::error::{AppError, AppResult};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/feed/{feed_token}/rss.xml", get(rss_feed))
}

#[derive(FromRow)]
struct FeedInfo {
    id: String,
    title: String,
    description: String,
    feed_token: String,
}

#[derive(FromRow)]
struct RssEpisode {
    id: String,
    title: String,
    source_url: Option<String>,
    audio_url: Option<String>,
    image_url: Option<String>,
    duration_secs: Option<i32>,
    pub_date: Option<String>,
}

async fn rss_feed(
    State(state): State<AppState>,
    Path(feed_token): Path<String>,
) -> AppResult<impl IntoResponse> {
    let feed = sqlx::query_as::<_, FeedInfo>(
        "SELECT id, title, description, feed_token FROM feeds WHERE feed_token = $1",
    )
    .bind(&feed_token)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let episodes = sqlx::query_as::<_, RssEpisode>(
        "SELECT id, title, source_url, audio_url, image_url, duration_secs, pub_date
         FROM episodes
         WHERE feed_id = $1 AND status = 'done' AND audio_url IS NOT NULL
         ORDER BY pub_date DESC
         LIMIT 50",
    )
    .bind(&feed.id)
    .fetch_all(&state.pool)
    .await?;

    let feed_link = format!(
        "{}/feed/{}/rss.xml",
        state.config.public_url, feed.feed_token
    );

    let mut items = String::new();
    for ep in &episodes {
        let pub_date = ep.pub_date.as_deref().unwrap_or("");
        let duration = ep.duration_secs.unwrap_or(0);
        let audio_url = ep.audio_url.as_deref().unwrap_or("");
        let description = ep
            .source_url
            .as_deref()
            .unwrap_or("PDF upload");

        let image_tag = if let Some(ref img_url) = ep.image_url {
            format!(
                "\n      <itunes:image href=\"{}\"/>",
                xml_escape(img_url)
            )
        } else {
            String::new()
        };

        items.push_str(&format!(
            r#"    <item>
      <title>{title}</title>
      <guid isPermaLink="false">{id}</guid>
      <pubDate>{pub_date}</pubDate>
      <description>{description}</description>
      <enclosure url="{audio_url}" length="0" type="audio/mpeg"/>
      <itunes:duration>{duration}</itunes:duration>{image_tag}
    </item>
"#,
            title = xml_escape(&ep.title),
            id = ep.id,
            pub_date = pub_date,
            description = xml_escape(description),
            audio_url = xml_escape(audio_url),
            duration = duration,
            image_tag = image_tag,
        ));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>{title}</title>
    <description>{description}</description>
    <link>{link}</link>
    <language>en-us</language>
    <itunes:author>Personal Podcast</itunes:author>
    <itunes:category text="Technology"/>
{items}  </channel>
</rss>"#,
        title = xml_escape(&feed.title),
        description = xml_escape(&feed.description),
        link = xml_escape(&feed_link),
        items = items,
    );

    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/rss+xml; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "max-age=300"),
        ],
        xml,
    ))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
