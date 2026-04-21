use walkdir::WalkDir;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Context;
use tracing::{info, warn};
use serde::{Deserialize, Serialize};

use crate::core::database::{self, FileData};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexError {
    pub path: String,
    pub error_type: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexErrors {
    pub permission_denied: Vec<String>,
    pub other_errors: Vec<IndexError>,
    pub total_errors: usize,
}

pub struct Indexer {
    root_path: PathBuf,
    is_indexing: Arc<Mutex<bool>>,
}

impl Indexer {
    pub fn new(root_path: PathBuf) -> Self {
        Self {
            root_path,
            is_indexing: Arc::new(Mutex::new(false)),
        }
    }

    pub fn is_indexing(&self) -> bool {
        *self.is_indexing.lock().unwrap()
    }

    fn system_time_to_timestamp(time: SystemTime) -> i64 {
        time.duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn index<P: AsRef<Path>>(&self, root: P, mut progress_callback: Option<&mut dyn FnMut(usize, &str)>) -> anyhow::Result<(usize, IndexErrors)> {
        let mut indexing = self.is_indexing.lock().unwrap();
        if *indexing {
            anyhow::bail!("Indexing already in progress");
        }
        *indexing = true;
        drop(indexing);

        info!(operation = "index_start", path = %root.as_ref().display(), "Indexing started");

        database::init_database()
            .context("Failed to initialize database")?;

        // Clear existing index
        if let Err(e) = database::clear_all_files() {
            warn!("Failed to clear existing files: {}", e);
        }

        let root_path = root.as_ref();
        let mut count = 0;

        // Error tracking
        let mut permission_denied: Vec<String> = Vec::new();
        let mut other_errors: Vec<IndexError> = Vec::new();

        // Batch processing for faster indexing
        const BATCH_SIZE: usize = 5000;
        let mut batch: Vec<FileData> = Vec::with_capacity(BATCH_SIZE);

        for entry in WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
        {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let path = e.path().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    let io_error = e.into_io_error();

                    if let Some(io_err) = io_error {
                        let error_type = io_err.kind();
                        let is_permission_denied = matches!(error_type,
                            std::io::ErrorKind::PermissionDenied
                        );

                        if is_permission_denied {
                            if permission_denied.len() < 50 {
                                permission_denied.push(path.clone());
                            }
                        } else {
                            other_errors.push(IndexError {
                                path: path.clone(),
                                error_type: format!("{:?}", error_type),
                                message: io_err.to_string(),
                            });
                        }
                    }
                    continue;
                }
            };

            let path = entry.path();

            // Get metadata
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    let path_str = path.to_string_lossy().to_string();
                    let io_err = e.io_error();

                    if let Some(io_error) = io_err {
                        let error_type = io_error.kind();

                        let is_permission_denied = matches!(error_type,
                            std::io::ErrorKind::PermissionDenied
                        );

                        if is_permission_denied {
                            if permission_denied.len() < 50 {
                                permission_denied.push(path_str);
                            }
                        } else {
                            other_errors.push(IndexError {
                                path: path_str,
                                error_type: format!("{:?}", error_type),
                                message: io_error.to_string(),
                            });
                        }
                    } else {
                        other_errors.push(IndexError {
                            path: path_str,
                            error_type: "Unknown".to_string(),
                            message: e.to_string(),
                        });
                    }
                    continue;
                }
            };

            let is_dir = path.is_dir();
            let size = metadata.len() as u64;
            let created = Self::system_time_to_timestamp(metadata.created().unwrap_or(UNIX_EPOCH));
            let modified = Self::system_time_to_timestamp(metadata.modified().unwrap_or(UNIX_EPOCH));

            let path_str = path.to_string_lossy().to_string();

            // Add to batch
            batch.push(FileData {
                path: path_str,
                size,
                is_directory: is_dir,
                created_at: created,
                modified_at: modified,
            });

            // Flush batch when full
            if batch.len() >= BATCH_SIZE {
                match database::insert_files_batch(&batch) {
                    Ok(inserted) => count += inserted,
                    Err(e) => {
                        for file in batch.iter() {
                            other_errors.push(IndexError {
                                path: file.path.clone(),
                                error_type: "DBError".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }
                batch.clear();

                if let Some(ref mut callback) = progress_callback {
                    callback(count, &format!("Indexed {} files...", count));
                }
            }
        }

        // Flush remaining batch
        if !batch.is_empty() {
            match database::insert_files_batch(&batch) {
                Ok(inserted) => count += inserted,
                Err(e) => {
                    for file in batch.iter() {
                        other_errors.push(IndexError {
                            path: file.path.clone(),
                            error_type: "DBError".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }

        // Only rebuild indexes if FST doesn't exist (first time)
        // This saves significant time for subsequent indexes
        if database::needs_fst_rebuild() {
            info!("FST not found, rebuilding indexes...");
            if let Err(e) = database::rebuild_all_indexes() {
                warn!("Failed to rebuild indexes: {}", e);
            }
        } else {
            info!("FST exists, skipping index rebuild (use force reindex to rebuild)");
        }

        let errors = IndexErrors {
            total_errors: permission_denied.len() + other_errors.len(),
            permission_denied,
            other_errors,
        };

        let mut indexing = self.is_indexing.lock().unwrap();
        *indexing = false;

        info!(operation = "index_complete", file_count = count, error_count = errors.total_errors, "Indexing completed");
        Ok((count, errors))
    }

    /// Reindex only files that have been modified since the last index
    pub fn reindex_modified<P: AsRef<Path>>(&self, root: P, mut progress_callback: Option<&mut dyn FnMut(usize, &str)>) -> anyhow::Result<(usize, IndexErrors)> {
        let mut indexing = self.is_indexing.lock().unwrap();
        if *indexing {
            anyhow::bail!("Indexing already in progress");
        }
        *indexing = true;
        drop(indexing);

        info!(operation = "reindex_modified_start", path = %root.as_ref().display(), "Reindexing modified files started");

        database::init_database()
            .context("Failed to initialize database")?;

        // Get the index creation time to filter files
        let index_created_at = match database::get_last_indexed_time()? {
            Some(ts) => ts,
            None => {
                info!("No index creation time found, doing full reindex");
                return self.index(root, progress_callback);
            }
        };

        let root_path = root.as_ref();
        let mut count = 0;

        // Error tracking
        let mut permission_denied: Vec<String> = Vec::new();
        let mut other_errors: Vec<IndexError> = Vec::new();

        // Batch processing
        const BATCH_SIZE: usize = 5000;
        let mut batch: Vec<FileData> = Vec::with_capacity(BATCH_SIZE);

        for entry in WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
        {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let path = e.path().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    let io_error = e.into_io_error();

                    if let Some(io_err) = io_error {
                        let error_type = io_err.kind();
                        let is_permission_denied = matches!(error_type,
                            std::io::ErrorKind::PermissionDenied
                        );

                        if is_permission_denied {
                            if permission_denied.len() < 50 {
                                permission_denied.push(path.clone());
                            }
                        } else {
                            other_errors.push(IndexError {
                                path: path.clone(),
                                error_type: format!("{:?}", error_type),
                                message: io_err.to_string(),
                            });
                        }
                    }
                    continue;
                }
            };

            let path = entry.path();

            // Get metadata
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    let path_str = path.to_string_lossy().to_string();
                    let io_err = e.io_error();

                    if let Some(io_error) = io_err {
                        let error_type = io_error.kind();

                        let is_permission_denied = matches!(error_type,
                            std::io::ErrorKind::PermissionDenied
                        );

                        if is_permission_denied {
                            if permission_denied.len() < 50 {
                                permission_denied.push(path_str);
                            }
                        } else {
                            other_errors.push(IndexError {
                                path: path_str,
                                error_type: format!("{:?}", error_type),
                                message: io_error.to_string(),
                            });
                        }
                    } else {
                        other_errors.push(IndexError {
                            path: path_str,
                            error_type: "Unknown".to_string(),
                            message: e.to_string(),
                        });
                    }
                    continue;
                }
            };

            let modified = Self::system_time_to_timestamp(metadata.modified().unwrap_or(UNIX_EPOCH));

            // Skip files that haven't been modified since last index
            if modified <= index_created_at {
                continue;
            }

            let is_dir = path.is_dir();
            let size = metadata.len() as u64;
            let created = Self::system_time_to_timestamp(metadata.created().unwrap_or(UNIX_EPOCH));

            let path_str = path.to_string_lossy().to_string();

            // Add to batch
            batch.push(FileData {
                path: path_str,
                size,
                is_directory: is_dir,
                created_at: created,
                modified_at: modified,
            });

            // Flush batch when full
            if batch.len() >= BATCH_SIZE {
                match database::insert_files_batch(&batch) {
                    Ok(inserted) => count += inserted,
                    Err(e) => {
                        for file in batch.iter() {
                            other_errors.push(IndexError {
                                path: file.path.clone(),
                                error_type: "DBError".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }
                batch.clear();

                if let Some(ref mut callback) = progress_callback {
                    callback(count, &format!("Reindexed {} files...", count));
                }
            }
        }

        // Flush remaining batch
        if !batch.is_empty() {
            match database::insert_files_batch(&batch) {
                Ok(inserted) => count += inserted,
                Err(e) => {
                    for file in batch.iter() {
                        other_errors.push(IndexError {
                            path: file.path.clone(),
                            error_type: "DBError".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }

        // Rebuild all indexes after all data is written
        if let Err(e) = database::rebuild_all_indexes() {
            warn!("Failed to rebuild indexes: {}", e);
        }

        let errors = IndexErrors {
            total_errors: permission_denied.len() + other_errors.len(),
            permission_denied,
            other_errors,
        };

        let mut indexing = self.is_indexing.lock().unwrap();
        *indexing = false;

        info!(operation = "reindex_modified_complete", file_count = count, error_count = errors.total_errors, "Reindexing completed");
        Ok((count, errors))
    }
}
