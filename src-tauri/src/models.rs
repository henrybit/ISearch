use std::path::PathBuf;
use chrono::DateTime;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub id: i64,
    pub path: PathBuf,
    pub filename: String,
    pub extension: Option<String>,
    pub size: u64,
    pub is_directory: bool,
    pub created_at: DateTime<chrono::Utc>,
    pub modified_at: DateTime<chrono::Utc>,
    pub indexed_at: DateTime<chrono::Utc>,
}
