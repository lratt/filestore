use chrono::Utc;

#[derive(Debug, FromRow)]
pub struct Upload {
    pub key: String,
    pub filename: String,
    pub expires: chrono::DateTime<Utc>,
    pub date_created: chrono::DateTime<Utc>,
}
