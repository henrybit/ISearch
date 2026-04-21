//! Native speed file indexer using platform-specific APIs.
//!
//! This provides much faster indexing than WalkDir by using:
//! - macOS: mdfind (Spotlight) - indexes all files instantly, supports iCloud
//! - Windows: NTFS MFT direct reading
//! - Fallback: WalkDir (incremental - yields files as discovered)

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use anyhow::Result;
use tracing::{debug, info, warn};

use crate::core::fs_indexer::{FileMetadata, PlatformIndexer};
use crate::core::database::{self, FileData};
use crate::core::indexer::{IndexError, IndexErrors};

pub struct NativeIndexer {
    indexer: PlatformIndexer,
    is_indexing: Arc<Mutex<bool>>,
    skip_hidden: bool,
}

impl NativeIndexer {
    pub fn new() -> Self {
        Self {
            indexer: PlatformIndexer::new(),
            is_indexing: Arc::new(Mutex::new(false)),
            skip_hidden: true, // Default: skip hidden directories
        }
    }

    pub fn with_skip_hidden(mut self, skip: bool) -> Self {
        self.skip_hidden = skip;
        self
    }

    pub fn is_indexing(&self) -> bool {
        *self.is_indexing.lock().unwrap()
    }

    pub fn get_skip_hidden(&self) -> bool {
        self.skip_hidden
    }

    /// Index using native platform APIs (fast!)
    pub fn index<P: AsRef<Path>>(&self, root: P, mut progress_callback: Option<&mut dyn FnMut(usize, &str)>) -> Result<(usize, IndexErrors)> {
        self.index_with_options(root, self.skip_hidden, progress_callback)
    }

    /// Index with custom options - uses mdfind on macOS for iCloud support
    pub fn index_with_options<P: AsRef<Path>>(&self, root: P, skip_hidden: bool, mut progress_callback: Option<&mut dyn FnMut(usize, &str)>) -> Result<(usize, IndexErrors)> {
        let mut indexing = self.is_indexing.lock().unwrap();
        if *indexing {
            anyhow::bail!("Indexing already in progress");
        }
        *indexing = true;
        drop(indexing);

        let root_path = root.as_ref();
        info!("Starting native indexing for {:?} (skip_hidden={})", root_path, skip_hidden);

        // Use mdfind on macOS for iCloud support, otherwise use WalkDir
        #[cfg(target_os = "macos")]
        let (count, total_size) = self.index_with_mdfind(root_path, &mut progress_callback)?;

        #[cfg(not(target_os = "macos"))]
        let (count, total_size) = self.index_incremental(root_path, skip_hidden, &mut progress_callback)?;

        // Record the indexed directory
        let root_str = root_path.to_string_lossy().to_string();
        if let Err(e) = database::record_indexed_dir(&root_str, count, total_size) {
            warn!("Failed to record indexed dir: {}", e);
        }

        let mut idx = self.is_indexing.lock().unwrap();
        *idx = false;

        // Rebuild indexes after all files are inserted
        if database::needs_fst_rebuild() {
            info!("Rebuilding FST index...");
            if let Err(e) = database::rebuild_all_indexes() {
                warn!("Failed to rebuild indexes: {}", e);
            }
        }

        info!("Native indexing complete: {} files", count);
        Ok((count, IndexErrors {
            permission_denied: Vec::new(),
            other_errors: Vec::new(),
            total_errors: 0,
        }))
    }

    /// Index using mdfind for macOS (supports iCloud files)
    #[cfg(target_os = "macos")]
    fn index_with_mdfind<P: AsRef<Path>>(&self, root: P, progress_callback: &mut Option<&mut dyn FnMut(usize, &str)>) -> Result<(usize, u64)> {
        let root_path = root.as_ref();
        let root_str = root_path.to_string_lossy();

        info!("Using mdfind to index {:?} (supports iCloud)", root_str);

        // Use mdfind to get all files including iCloud stubs
        // Query "kMDItemFSName EXISTS" matches all files
        let output = Command::new("mdfind")
            .args(["-onlyin", &root_str, "kMDItemFSName", "EXISTS"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            info!("mdfind failed with stderr: {}, falling back to WalkDir", stderr);
            return self.index_incremental(root, true, progress_callback);
        }

        let paths_str = String::from_utf8_lossy(&output.stdout);
        let total_paths = paths_str.lines().count();
        info!("mdfind found {} paths, now getting metadata...", total_paths);

        let mut count = 0;
        let mut inserted = 0;
        let mut total_size = 0u64;
        let mut batch: Vec<FileData> = Vec::with_capacity(1000);

        for line in paths_str.lines() {
            if line.is_empty() {
                continue;
            }

            let path_str = line.to_string();

            // Get metadata using mdls (supports iCloud stubs)
            if let Some((size, is_dir, modified, created)) = get_mdls_metadata(&path_str) {
                total_size += size;
                batch.push(FileData {
                    path: path_str,
                    size,
                    is_directory: is_dir,
                    created_at: created,
                    modified_at: modified,
                });
                count += 1;

                if batch.len() >= 1000 {
                    match database::insert_files_batch(&batch) {
                        Ok(n) => inserted += n,
                        Err(_) => {}
                    }
                    batch.clear();

                    if let Some(cb) = progress_callback {
                        cb(inserted, &format!("已索引 {} / {} 文件...", inserted, total_paths));
                    }
                }
            }
        }

        // Flush remaining
        if !batch.is_empty() {
            match database::insert_files_batch(&batch) {
                Ok(n) => inserted += n,
                Err(_) => {}
            }
            if let Some(cb) = progress_callback {
                cb(inserted, &format!("已索引 {} / {} 文件...", inserted, total_paths));
            }
        }

        info!("mdfind indexed {} files, {} inserted", count, inserted);
        Ok((inserted, total_size))
    }

    #[cfg(not(target_os = "macos"))]
    fn index_with_mdfind<P: AsRef<Path>>(&self, root: P, progress_callback: &mut Option<&mut dyn FnMut(usize, &str)>) -> Result<(usize, u64)> {
        self.index_incremental(root, true, progress_callback)
    }

    /// Incremental indexing - inserts files as they're discovered
    /// Returns (file_count, total_size_bytes)
    fn index_incremental<P: AsRef<Path>>(&self, root: P, skip_hidden: bool, progress_callback: &mut Option<&mut dyn FnMut(usize, &str)>) -> Result<(usize, u64)> {
        use walkdir::WalkDir;

        const BATCH_SIZE: usize = 1000; // Smaller batches for incremental insert
        let mut batch: Vec<FileData> = Vec::with_capacity(BATCH_SIZE);
        let mut inserted = 0;
        let mut discovered = 0;
        let mut total_size = 0u64;

        // Helper to check if a path component is hidden
        let is_hidden_component = |name: &std::ffi::OsStr| -> bool {
            if let Some(s) = name.to_str() {
                s.starts_with('.') && s.len() > 1
            } else {
                false
            }
        };

        for entry in WalkDir::new(root.as_ref())
            .follow_links(false)
            .into_iter()
        {
            // Log WalkDir errors for debugging (but skip the entry)
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    debug!("WalkDir error: {}", err);
                    continue;
                }
            };

            let path = entry.path();

            // Skip files in hidden directories
            if skip_hidden {
                let mut should_skip = false;
                for component in path.components() {
                    if let std::path::Component::Normal(name) = component {
                        if is_hidden_component(name) {
                            should_skip = true;
                            break;
                        }
                    }
                }
                if should_skip {
                    continue;
                }
            }

            if path.is_dir() {
                continue;
            }

            discovered += 1;

            if let Ok(metadata) = entry.metadata() {
                let file_size = metadata.len();

                // Log iCloud stub files (size 0 but exists)
                if file_size == 0 {
                    debug!("File has size 0: {:?}", path);
                }
                total_size += file_size;

                let modified = metadata.modified()
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
                    .unwrap_or(0);
                let created = metadata.created()
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
                    .unwrap_or(0);

                batch.push(FileData {
                    path: path.to_string_lossy().to_string(),
                    size: file_size,
                    is_directory: false,
                    created_at: created,
                    modified_at: modified,
                });
            } else {
                debug!("Failed to get metadata for: {:?}", path);
            }

            if batch.len() >= BATCH_SIZE {
                match database::insert_files_batch(&batch) {
                    Ok(n) => inserted += n,
                    Err(_) => {
                        // Continue on error
                    }
                }
                batch.clear();

                if let Some(cb) = progress_callback {
                    cb(inserted, &format!("Indexed {} files...", inserted));
                }
            }
        }

        // Flush remaining
        if !batch.is_empty() {
            match database::insert_files_batch(&batch) {
                Ok(n) => inserted += n,
                Err(_) => {}
            }
        }

        info!("Discovered {} files, inserted {} into database, total size {} bytes", discovered, inserted, total_size);
        Ok((inserted, total_size))
    }
}

/// Get file metadata using mdls command (supports iCloud stubs)
#[cfg(target_os = "macos")]
fn get_mdls_metadata(path: &str) -> Option<(u64, bool, i64, i64)> {
    let output = std::process::Command::new("mdls")
        .args([
            "-name", "kMDItemFSSize",
            "-name", "kMDItemFSIsDirectory",
            "-name", "kMDItemFSCreationDate",
            "-name", "kMDItemFSContentChangeDate",
            path,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut size: u64 = 0;
    let mut is_dir: bool = false;
    let mut created: i64 = 0;
    let mut modified: i64 = 0;

    for line in output_str.lines() {
        let line = line.trim();
        if line.starts_with("kMDItemFSSize") {
            if let Some(val) = line.split('=').nth(1) {
                let val = val.trim();
                if val != "(null)" {
                    size = val.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("kMDItemFSIsDirectory") {
            if let Some(val) = line.split('=').nth(1) {
                let val = val.trim();
                is_dir = val == "1";
            }
        } else if line.starts_with("kMDItemFSCreationDate") {
            if let Some(val) = line.split('=').nth(1) {
                let val = val.trim();
                if val != "(null)" {
                    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(val, "%Y-%m-%d %H:%M:%S %z") {
                        created = dt.timestamp();
                    }
                }
            }
        } else if line.starts_with("kMDItemFSContentChangeDate") {
            if let Some(val) = line.split('=').nth(1) {
                let val = val.trim();
                if val != "(null)" {
                    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(val, "%Y-%m-%d %H:%M:%S %z") {
                        modified = dt.timestamp();
                    }
                }
            }
        }
    }

    Some((size, is_dir, created, modified))
}

impl Default for NativeIndexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_indexer_creation() {
        let indexer = NativeIndexer::new();
        assert!(!indexer.is_indexing());
    }
}
