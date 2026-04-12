use anyhow::Result;
use sqlx::{FromRow, PgPool};
use std::time::Duration;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::pipeline::storage::StorageClient;

#[derive(Debug, FromRow)]
struct Job {
    id: Uuid,
    episode_id: Uuid,
    job_type: String,
    attempts: i32,
}

pub async fn run_worker(pool: PgPool, config: AppConfig, storage: StorageClient) {
    loop {
        match claim_next_job(&pool).await {
            Ok(Some(job)) => {
                let pool = pool.clone();
                let config = config.clone();
                let storage = storage.clone();
                tokio::spawn(async move {
                    execute_job(job, &pool, &config, &storage).await;
                });
            }
            Ok(None) => {
                tokio::time::sleep(Duration::from_secs(config.worker_poll_interval)).await;
            }
            Err(e) => {
                tracing::error!("Worker poll error: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn claim_next_job(pool: &PgPool) -> Result<Option<Job>> {
    let mut tx = pool.begin().await?;

    let job = sqlx::query_as::<_, Job>(
        "SELECT id, episode_id, job_type, attempts
         FROM jobs
         WHERE status = 'queued' AND run_after <= NOW()
         ORDER BY created_at ASC
         LIMIT 1
         FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(ref job) = job {
        sqlx::query("UPDATE jobs SET status = 'running' WHERE id = $1")
            .bind(job.id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(job)
}

async fn execute_job(job: Job, pool: &PgPool, config: &AppConfig, storage: &StorageClient) {
    tracing::info!(
        "Executing job {} (type={}, episode={}, attempt={})",
        job.id,
        job.job_type,
        job.episode_id,
        job.attempts + 1
    );

    // Update episode status to reflect current stage
    let stage_status = match job.job_type.as_str() {
        "scrape" => "scraping",
        "clean" => "cleaning",
        "tts" => "tts",
        _ => "error",
    };

    if let Err(e) = sqlx::query("UPDATE episodes SET status = $1 WHERE id = $2")
        .bind(stage_status)
        .bind(job.episode_id)
        .execute(pool)
        .await
    {
        tracing::error!("Failed to update episode status: {e}");
        return;
    }

    let result = match job.job_type.as_str() {
        "scrape" => crate::pipeline::scrape::run(job.episode_id, pool, config).await,
        "clean" => crate::pipeline::clean::run(job.episode_id, pool, config).await,
        "tts" => crate::pipeline::tts::run(job.episode_id, pool, config, storage).await,
        other => Err(anyhow::anyhow!("Unknown job type: {other}")),
    };

    match result {
        Ok(_) => {
            if let Err(e) = complete_job(pool, &job).await {
                tracing::error!("Failed to complete job {}: {e}", job.id);
            }
        }
        Err(e) => {
            tracing::error!("Job {} failed: {e:?}", job.id);
            if let Err(e2) = fail_job(pool, &job, &e.to_string(), config.max_job_attempts).await {
                tracing::error!("Failed to record job failure {}: {e2}", job.id);
            }
        }
    }
}

async fn complete_job(pool: &PgPool, job: &Job) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE jobs SET status = 'done' WHERE id = $1")
        .bind(job.id)
        .execute(&mut *tx)
        .await?;

    // Transition to next stage
    match job.job_type.as_str() {
        "scrape" => {
            sqlx::query(
                "INSERT INTO jobs (episode_id, job_type, status) VALUES ($1, 'clean', 'queued')",
            )
            .bind(job.episode_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE episodes SET status = 'cleaning' WHERE id = $1")
                .bind(job.episode_id)
                .execute(&mut *tx)
                .await?;
        }
        "clean" => {
            sqlx::query(
                "INSERT INTO jobs (episode_id, job_type, status) VALUES ($1, 'tts', 'queued')",
            )
            .bind(job.episode_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE episodes SET status = 'tts' WHERE id = $1")
                .bind(job.episode_id)
                .execute(&mut *tx)
                .await?;
        }
        "tts" => {
            sqlx::query(
                "UPDATE episodes SET status = 'done', pub_date = NOW() WHERE id = $1",
            )
            .bind(job.episode_id)
            .execute(&mut *tx)
            .await?;
        }
        _ => {}
    }

    tx.commit().await?;

    tracing::info!(
        "Job {} completed (type={}, episode={})",
        job.id,
        job.job_type,
        job.episode_id,
    );

    Ok(())
}

async fn fail_job(
    pool: &PgPool,
    job: &Job,
    error_msg: &str,
    max_attempts: i32,
) -> Result<()> {
    let new_attempts = job.attempts + 1;
    let mut tx = pool.begin().await?;

    if new_attempts < max_attempts {
        // Retry with exponential backoff
        let backoff_secs = 60 * (1 << new_attempts); // 2min, 4min, 8min...
        sqlx::query(
            "UPDATE jobs SET status = 'queued', attempts = $1,
             run_after = NOW() + make_interval(secs => $2)
             WHERE id = $3",
        )
        .bind(new_attempts)
        .bind(backoff_secs as f64)
        .bind(job.id)
        .execute(&mut *tx)
        .await?;

        tracing::warn!(
            "Job {} attempt {}/{} failed, retrying in {backoff_secs}s: {error_msg}",
            job.id,
            new_attempts,
            max_attempts,
        );
    } else {
        // Permanent failure
        sqlx::query("UPDATE jobs SET status = 'error', attempts = $1 WHERE id = $2")
            .bind(new_attempts)
            .bind(job.id)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "UPDATE episodes SET status = 'error', error_msg = $1 WHERE id = $2",
        )
        .bind(error_msg)
        .bind(job.episode_id)
        .execute(&mut *tx)
        .await?;

        tracing::error!(
            "Job {} permanently failed after {max_attempts} attempts: {error_msg}",
            job.id,
        );
    }

    tx.commit().await?;
    Ok(())
}
