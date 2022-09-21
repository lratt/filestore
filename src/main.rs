#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use anyhow::Context;
use aws_sdk_s3 as s3;
use axum::{http::Uri, Extension};
use futures_util::TryStreamExt;
use sqlx::PgPool;
use std::{net::SocketAddr, str::FromStr, time::Duration};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

mod error;
mod handler;
mod model;
mod util;

#[macro_use]
extern crate sqlx;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

async fn delete_expired(pg_pool: PgPool, s3_client: s3::Client) -> anyhow::Result<()> {
    let mut tx = pg_pool.begin().await?;
    let mut get_stream =
        query!(r"SELECT * FROM uploads WHERE expires < current_timestamp").fetch_many(&pg_pool);
    while let Some(sqlx::Either::Right(upload)) = get_stream.try_next().await? {
        s3_client
            .delete_object()
            .bucket("uploads")
            .key(&upload.key)
            .send()
            .await?;
        query!("DELETE FROM uploads WHERE key = $1", &upload.key)
            .execute(&mut tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let s3_endpoint = std::env::var("S3_ENDPOINT").with_context(|| "S3_ENDPOINT missing")?;
    let s3_region = std::env::var("S3_REGION").with_context(|| "S3_REGION missing")?;
    let s3_access_key = std::env::var("S3_ACCESS_KEY").with_context(|| "S3_ACCESS_KEY missing")?;
    let s3_secret_key = std::env::var("S3_SECRET_KEY").with_context(|| "S3_SECRET_KEY missing")?;

    let aws_cfg = aws_config::from_env()
        .endpoint_resolver(s3::Endpoint::immutable(Uri::from_str(&s3_endpoint)?))
        .region(s3::Region::new(s3_region))
        .credentials_provider(s3::Credentials::new(
            s3_access_key,
            s3_secret_key,
            None,
            None,
            "zvis",
        ))
        .load()
        .await;

    let s3_client = s3::Client::new(&aws_cfg);
    let pg_pool = sqlx::postgres::PgPoolOptions::default()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await?;

    MIGRATOR.run(&pg_pool).await?;

    // delete expired uploads every 30 seconds
    {
        let pg_pool = pg_pool.clone();
        let s3_client = s3_client.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = delete_expired(pg_pool.clone(), s3_client.clone()).await {
                    error!(?e, "failed to delete expired uploads");
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
    }

    let router = axum::Router::new()
        .route("/", axum::routing::post(handler::upload))
        .route("/:key", axum::routing::get(handler::get_object))
        .layer(Extension(s3_client))
        .layer(Extension(pg_pool))
        .layer(TraceLayer::new_for_http());

    let addr = std::env::var("LISTEN")
        .ok()
        .and_then(|g| g.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 5000)));

    info!("listening on {}", &addr);
    axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}
