use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::get,
    Router,
};
use sqlx::FromRow;
use time::format_description::well_known::Rfc2822;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/feed/{feed_token}/rss.xml", get(rss_feed))
}

#[derive(FromRow)]
struct FeedInfo {
    id: Uuid,
    title: String,
    description: String,
    feed_token: Uuid,
}

#[derive(FromRow)]
struct RssEpisode {
    id: Uuid,
    title: String,
    source_url: String,
    audio_url: Option<String>,
    duration_secs: Option<i32>,
    pub_date: Option<OffsetDateTime>,
}

async fn rss_feed(
    State(state): State<AppState>,
    Path(feed_token): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let feed = sqlx::query_as::<_, FeedInfo>(
        "SELECT id, title, description, feed_token FROM feeds WHERE feed_token = $1",
    )
    .bind(feed_token)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let episodes = sqlx::query_as::<_, RssEpisode>(
        "SELECT id, title, source_url, audio_url, duration_secs, pub_date
         FROM episodes
         WHERE feed_id = $1 AND status = 'done' AND audio_url IS NOT NULL
         ORDER BY pub_date DESC
         LIMIT 50",
    )
    .bind(feed.id)
    .fetch_all(&state.pool)
    .await?;

    let feed_link = format!(
        "{}/feed/{}/rss.xml",
        state.config.public_url, feed.feed_token
    );

    let mut items = String::new();
    for ep in &episodes {
        let pub_date = ep
            .pub_date
            .map(|d| d.format(&Rfc2822).unwrap_or_default())
            .unwrap_or_default();

        let duration = ep.duration_secs.unwrap_or(0);
        let audio_url = ep.audio_url.as_deref().unwrap_or("");

        items.push_str(&format!(
            r#"    <item>
      <title>{title}</title>
      <guid isPermaLink="false">{id}</guid>
      <pubDate>{pub_date}</pubDate>
      <description>{source_url}</description>
      <enclosure url="{audio_url}" length="0" type="audio/mpeg"/>
      <itunes:duration>{duration}</itunes:duration>
    </item>
"#,
            title = xml_escape(&ep.title),
            id = ep.id,
            pub_date = pub_date,
            source_url = xml_escape(&ep.source_url),
            audio_url = xml_escape(audio_url),
            duration = duration,
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
