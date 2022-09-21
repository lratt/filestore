use crate::{
    error::{any_error, AppError},
    model::Upload,
    util::gen_key,
};
use aws_sdk_s3 as s3;
use axum::{
    body::StreamBody,
    extract::{Multipart, Path},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::IntoResponse,
    Extension,
};
use chrono::Utc;
use s3::types::ByteStream;
use sqlx::PgPool;

#[derive(Debug)]
enum FilePartResponse {
    Skip,
    Ok(Upload),
}

async fn handle_file_part(
    mut field: axum::extract::multipart::Field<'_>,
    s3_client: s3::Client,
    pg_pool: PgPool,
) -> anyhow::Result<FilePartResponse> {
    let file_name = match field.file_name() {
        Some(file_name) => file_name.to_owned(),
        None => return Ok(FilePartResponse::Skip),
    };
    let mime_type = mime_guess::from_path(&file_name).first_or_octet_stream();

    let body = {
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = field.chunk().await? {
            buf.append(&mut chunk.to_vec());
        }
        ByteStream::from(buf)
    };

    let mut key = gen_key();

    'key_loop: loop {
        if let Err(s3::types::SdkError::ServiceError { err, .. }) = s3_client
            .head_object()
            .bucket("uploads")
            .key(&key)
            .send()
            .await
        {
            if err.is_not_found() {
                break 'key_loop;
            }
        }

        key = gen_key();
    }

    let upload = sqlx::query_as!(
        Upload,
        r#"INSERT INTO uploads (key, filename, expires) VALUES ($1, $2, $3) RETURNING *"#,
        key,
        file_name,
        Utc::now() + chrono::Duration::days(7)
    )
    .fetch_one(&pg_pool)
    .await?;

    s3_client
        .put_object()
        .bucket("uploads")
        .body(body)
        .key(&key)
        .content_type(mime_type.to_string())
        .content_disposition(format!("inline; filename=\"{}\"", &file_name))
        .send()
        .await?;

    Ok(FilePartResponse::Ok(upload))
}

pub async fn upload(
    mut multipart: Multipart,
    Extension(s3_client): Extension<s3::Client>,
    Extension(pg_pool): Extension<PgPool>,
) -> Result<impl IntoResponse, AppError> {
    let mut uploads = vec![];

    while let Some(field) = multipart.next_field().await.map_err(any_error)? {
        if let Some(field_name) = field.name() {
            match field_name {
                "expires" => todo!("parse expiration"),
                "file" => match handle_file_part(field, s3_client.clone(), pg_pool.clone())
                    .await
                    .map_err(any_error)?
                {
                    FilePartResponse::Skip => continue,
                    FilePartResponse::Ok(upload) => {
                        uploads.push(upload);
                    }
                },
                _ => continue,
            }
        }
    }

    Ok(uploads
        .into_iter()
        .map(|u| format!("{}: http://127.0.0.1:5000/{}\r\n", &u.filename, &u.key))
        .collect::<String>())
}

pub async fn get_object(
    Path(key): Path<String>,
    Extension(s3_client): Extension<s3::Client>,
    Extension(pg_pool): Extension<PgPool>,
) -> Result<impl IntoResponse, AppError> {
    let obj = s3_client
        .get_object()
        .bucket("uploads")
        .key(&key)
        .send()
        .await
        .map_err(any_error)?;

    match query_as!(Upload, "SELECT * FROM uploads WHERE key = $1", key)
        .fetch_one(&pg_pool)
        .await
    {
        Err(sqlx::Error::RowNotFound) => return Ok(StatusCode::NOT_FOUND.into_response()),
        u => u,
    }
    .map_err(any_error)?;

    let mut headers = HeaderMap::new();
    if let Some(cd) = obj.content_disposition {
        headers.insert(CONTENT_DISPOSITION, cd.parse().unwrap());
    }
    if let Some(ct) = obj.content_type {
        headers.insert(CONTENT_TYPE, ct.parse().unwrap());
    }
    let async_read = obj.body.into_async_read();
    let stream = tokio_util::io::ReaderStream::new(async_read);
    let body = StreamBody::new(stream);

    Ok((headers, body).into_response())
}
