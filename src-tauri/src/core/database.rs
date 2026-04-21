//! Database layer using LMDB + FST.
//!
//! Simplified architecture (Everything-style):
//! - LMDB: stores path → FullMetadata (mmap for fast access)
//! - FST: prefix search index (mmap for fast search)
//! - InvertedIndex: filename search
//!
//! Memory: OS manages LMDB cache via mmap

use std::path::PathBuf;
use std::sync::Arc;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tracing::{info, warn};

use crate::core::lmdb_store::{LmdbStore, FullMetadata};
use crate::core::fst_index::FstIndex;
use crate::core::sharded_fst::ShardedFstIndex;
use crate::core::inverted_index::InvertedIndex;
use crate::core::delta_tracker::DeltaTracker;

pub static LMDB_STORE: Lazy<Arc<LmdbStore>> = Lazy::new(|| {
    Arc::new(LmdbStore::new(&get_data_dir()).unwrap())
});

pub static FST_INDEX: Lazy<Arc<RwLock<FstIndex>>> = Lazy::new(|| {
    Arc::new(RwLock::new(FstIndex::new(&get_data_dir())))
});

pub static SHARDED_FST: Lazy<Arc<ShardedFstIndex>> = Lazy::new(|| {
    Arc::new(ShardedFstIndex::new(&get_data_dir()))
});

pub static INVERTED_INDEX: Lazy<Arc<RwLock<InvertedIndex>>> = Lazy::new(|| {
    Arc::new(RwLock::new(InvertedIndex::new(&get_data_dir())))
});

pub static DELTA_TRACKER: Lazy<Arc<RwLock<DeltaTracker>>> = Lazy::new(|| {
    let tracker = DeltaTracker::load(&get_data_dir().join("delta.json"))
        .unwrap_or_default();
    Arc::new(RwLock::new(tracker))
});

pub fn get_data_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".isearch").join("data")
}

pub fn init_database() -> anyhow::Result<()> {
    let data_dir = get_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    info!("Initializing database at {:?}", data_dir);

    let store = LMDB_STORE.clone();
    match store.len() {
        Ok(count) => info!("LMDB contains {} entries", count),
        Err(e) => warn!("Failed to get LMDB count: {}", e),
    }

    // Load or build FST index
    let fst_index = FST_INDEX.clone();
    let fst_needs_rebuild = fst_index.read().is_empty() ||
        (fst_index.read().len() == 0 && std::path::Path::new(&data_dir.join("paths.fst")).exists());

    if fst_needs_rebuild {
        let paths = store.get_all_paths()?;
        info!("Building FST from {} paths", paths.len());
        if !paths.is_empty() {
            fst_index.write().build_from_paths(&paths)?;
            info!("FST built with {} entries", fst_index.read().len());

            // Also build SHARDED_FST
            SHARDED_FST.build_from_paths(&paths)?;
            info!("SHARDED_FST built");
        }
    }

    // Load or build Inverted Index
    {
        let inverted = INVERTED_INDEX.read();
        if inverted.is_empty() {
            drop(inverted);
            let id_paths = store.get_all_id_paths()?;
            info!("Building Inverted Index from {} paths", id_paths.len());
            if !id_paths.is_empty() {
                let mut inverted_write = INVERTED_INDEX.write();
                inverted_write.set_lmdb_store(store.clone());
                inverted_write.build_from_id_paths(&id_paths)?;
                info!("Inverted Index built with {} tokens", inverted_write.token_count());
            }
        } else {
            drop(inverted);
            let mut inverted_write = INVERTED_INDEX.write();
            inverted_write.set_lmdb_store(store.clone());
        }
    }

    info!("Database initialized (Everything-style: LMDB + FST mmap)");
    Ok(())
}

pub fn rebuild_all_indexes() -> anyhow::Result<()> {
    let paths = LMDB_STORE.get_all_paths()?;
    let id_paths = LMDB_STORE.get_all_id_paths()?;

    info!("Rebuilding all indexes from {} paths", paths.len());

    // Rebuild FST
    FST_INDEX.write().build_from_paths(&paths)?;
    info!("FST rebuilt with {} entries", FST_INDEX.read().len());

    // Also rebuild SHARDED_FST (used for search)
    SHARDED_FST.build_from_paths(&paths)?;
    info!("SHARDED_FST rebuilt");

    // Rebuild Inverted Index
    INVERTED_INDEX.write().build_from_id_paths(&id_paths)?;
    info!("Inverted Index rebuilt with {} tokens", INVERTED_INDEX.read().token_count());

    // Mark delta as indexed
    {
        let mut delta = DELTA_TRACKER.write();
        delta.mark_indexed();
        let _ = delta.save(&get_data_dir().join("delta.json"));
    }

    Ok(())
}

/// File data for batch insert
pub struct FileData {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub created_at: i64,
    pub modified_at: i64,
}

/// Insert multiple files in batch
pub fn insert_files_batch(files: &[FileData]) -> anyhow::Result<usize> {
    if files.is_empty() {
        return Ok(0);
    }

    let store = LMDB_STORE.clone();

    // Build entries with IDs
    let mut entries = Vec::with_capacity(files.len());
    let mut next_id = store.next_id();

    for file in files {
        let metadata = FullMetadata {
            id: next_id,
            size: file.size,
            is_directory: file.is_directory,
            created_at: file.created_at,
            modified_at: file.modified_at,
        };
        entries.push((file.path.clone(), metadata));
        next_id += 1;
    }

    // Update next_id
    store.set_next_id(next_id);

    // Batch insert
    store.insert_batch(&entries)?;

    // Record in delta tracker
    {
        let mut delta = DELTA_TRACKER.write();
        for file in files {
            delta.record_add(file.path.clone());
        }
    }

    Ok(files.len())
}

/// Insert a single file (for watcher)
pub fn insert_file(
    path: &str,
    _filename: &str,
    _extension: Option<&str>,
    size: i64,
    is_directory: bool,
    created_at: i64,
    modified_at: i64,
    _indexed_at: i64,
) -> anyhow::Result<u64> {
    let store = LMDB_STORE.clone();
    let next_id = store.next_id();

    let metadata = FullMetadata {
        id: next_id,
        size: size as u64,
        is_directory,
        created_at,
        modified_at,
    };

    store.insert(path, &metadata)?;

    // Record in delta tracker
    {
        let mut delta = DELTA_TRACKER.write();
        delta.record_add(path.to_string());
    }

    Ok(next_id)
}

/// File metadata with path for search results
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub id: u64,
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub created_at: i64,
    pub modified_at: i64,
}

/// Get full metadata by path (includes path in return)
pub fn get_file_by_path(path: &str) -> anyhow::Result<Option<FileMetadata>> {
    match LMDB_STORE.get_by_path(path)? {
        Some(meta) => Ok(Some(FileMetadata {
            id: meta.id,
            path: path.to_string(),
            size: meta.size,
            is_directory: meta.is_directory,
            created_at: meta.created_at,
            modified_at: meta.modified_at,
        })),
        None => Ok(None),
    }
}

/// Get full metadata by ID
pub fn get_file_by_id(id: u64) -> anyhow::Result<Option<FileMetadata>> {
    match LMDB_STORE.get_by_id(id)? {
        Some((path, meta)) => Ok(Some(FileMetadata {
            id: meta.id,
            path,
            size: meta.size,
            is_directory: meta.is_directory,
            created_at: meta.created_at,
            modified_at: meta.modified_at,
        })),
        None => Ok(None),
    }
}

/// Get all files for listing
pub fn get_all_files(sort_by: &str, sort_desc: bool, limit: usize) -> anyhow::Result<Vec<FileMetadata>> {
    let entries = LMDB_STORE.get_all_entries()?;

    let mut results: Vec<_> = entries
        .into_iter()
        .map(|(path, m)| FileMetadata {
            id: m.id,
            path,
            size: m.size,
            is_directory: m.is_directory,
            created_at: m.created_at,
            modified_at: m.modified_at,
        })
        .collect();

    // Sort
    match sort_by {
        "name" => results.sort_by(|a, b| {
            let name_a = std::path::Path::new(&a.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let name_b = std::path::Path::new(&b.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            name_a.cmp(&name_b)
        }),
        "size" => results.sort_by(|a, b| a.size.cmp(&b.size)),
        "modified" => results.sort_by(|a, b| a.modified_at.cmp(&b.modified_at)),
        "created" => results.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
        _ => results.sort_by(|a, b| {
            let name_a = std::path::Path::new(&a.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let name_b = std::path::Path::new(&b.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            name_a.cmp(&name_b)
        }),
    }

    if sort_desc {
        results.reverse();
    }

    results.truncate(limit);
    Ok(results)
}

/// Iterator for files (for memory-efficient iteration)
pub fn iter_files(
    sort_by: &str,
    sort_desc: bool,
    offset: usize,
    limit: usize,
) -> anyhow::Result<Box<dyn Iterator<Item = FileMetadata>>> {
    let all = get_all_files(sort_by, sort_desc, 1000000)?;
    Ok(Box::new(all.into_iter().skip(offset).take(limit)))
}

/// Get all file paths (for security scanning)
pub fn get_all_file_paths() -> anyhow::Result<Vec<String>> {
    let entries = LMDB_STORE.get_all_entries()?;
    let paths: Vec<String> = entries.into_iter().map(|(path, _)| path).collect();
    Ok(paths)
}

/// Get suggestions (prefix search)
pub fn get_suggestions(prefix: &str, limit: usize) -> anyhow::Result<Vec<String>> {
    let mut results = SHARDED_FST.search_prefix(prefix, limit);

    if results.len() < limit {
        let filename_results = INVERTED_INDEX.read().search_prefix(prefix, limit);
        results.extend(filename_results);
    }

    results.sort();
    results.dedup();
    results.truncate(limit);
    Ok(results)
}

pub fn get_file_count() -> anyhow::Result<i64> {
    let count = LMDB_STORE.len()?;
    Ok(count as i64)
}

pub fn clear_all_files() -> anyhow::Result<()> {
    LMDB_STORE.clear()?;

    // Delete FST file to force rebuild after re-indexing
    let fst_path = get_data_dir().join("paths.fst");
    if fst_path.exists() {
        std::fs::remove_file(&fst_path).ok();
    }

    // Clear FST in memory
    FST_INDEX.write().build_from_paths(&[])?;

    // Clear Inverted Index
    INVERTED_INDEX.write().build_from_id_paths(&[])?;

    // Clear delta tracker
    {
        let mut delta = DELTA_TRACKER.write();
        delta.mark_indexed();
    }

    Ok(())
}

pub fn delete_file_by_path(path: &str) -> anyhow::Result<()> {
    // Record removal
    {
        let mut delta = DELTA_TRACKER.write();
        delta.record_remove(path.to_string());
    }

    LMDB_STORE.delete(path)?;
    Ok(())
}

pub fn get_last_indexed_time() -> anyhow::Result<Option<i64>> {
    let delta = DELTA_TRACKER.read();
    Ok(Some(delta.last_index_time).filter(|&t| t > 0))
}

pub fn set_last_indexed_time(_timestamp: i64) -> anyhow::Result<()> {
    Ok(())
}

pub fn needs_reindex() -> anyhow::Result<bool> {
    let count = get_file_count()?;
    Ok(count == 0)
}

/// Check if FST index needs to be rebuilt
pub fn needs_fst_rebuild() -> bool {
    let fst_path = get_data_dir().join("paths.fst");
    !fst_path.exists()
}

pub fn get_db_path() -> PathBuf {
    get_data_dir()
}

pub fn get_index_stats() -> anyhow::Result<IndexMemoryStats> {
    let lmdb_stats = LMDB_STORE.get_stats()?;
    let fst_size = FST_INDEX.read().file_size();
    let inverted_size = INVERTED_INDEX.read().file_size();
    let file_count = LMDB_STORE.len()?;

    Ok(IndexMemoryStats {
        lmdb_file_count: file_count,
        lmdb_max_entries: lmdb_stats.max_file_count,
        lmdb_max_map_size: lmdb_stats.max_map_size,
        metadata_entry_count: file_count,
        fst_size,
        inverted_index_size: inverted_size,
        estimated_memory_usage: lmdb_stats.max_map_size,
        is_near_capacity: lmdb_stats.is_near_capacity,
    })
}

pub fn check_and_evict_if_needed() -> anyhow::Result<usize> {
    Ok(0)
}

// Re-export for convenience
pub fn get_sharded_fst_index() -> Arc<ShardedFstIndex> {
    SHARDED_FST.clone()
}

pub fn get_inverted_index() -> Arc<RwLock<InvertedIndex>> {
    INVERTED_INDEX.clone()
}

/// Memory statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexMemoryStats {
    pub lmdb_file_count: u64,
    pub lmdb_max_entries: u64,
    pub lmdb_max_map_size: usize,
    pub metadata_entry_count: u64,
    pub fst_size: u64,
    pub inverted_index_size: u64,
    pub estimated_memory_usage: usize,
    pub is_near_capacity: bool,
}

// ============================================================================
// Directory indexing status tracking
// ============================================================================

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Information about an indexed directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDirInfo {
    pub path: String,
    pub file_count: usize,
    pub size_bytes: u64,
    pub last_indexed: i64,
}

/// Get the path to the indexed directories tracking file
pub fn get_indexed_dirs_file() -> PathBuf {
    get_data_dir().join("indexed_dirs.json")
}

/// Save indexed directories to disk
pub fn save_indexed_dirs(dirs: &HashMap<String, IndexedDirInfo>) -> anyhow::Result<()> {
    let path = get_indexed_dirs_file();
    let json = serde_json::to_string_pretty(dirs)?;
    std::fs::write(&path, json)?;
    info!("Saved indexed dirs tracking to {:?}", path);
    Ok(())
}

/// Load indexed directories from disk
pub fn load_indexed_dirs() -> anyhow::Result<HashMap<String, IndexedDirInfo>> {
    let path = get_indexed_dirs_file();
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let json = std::fs::read_to_string(&path)?;
    let dirs: HashMap<String, IndexedDirInfo> = serde_json::from_str(&json)?;
    Ok(dirs)
}

/// Record a directory as indexed
pub fn record_indexed_dir(path: &str, file_count: usize, size_bytes: u64) -> anyhow::Result<()> {
    let mut dirs = load_indexed_dirs()?;

    // Record the main directory
    let info = IndexedDirInfo {
        path: path.to_string(),
        file_count,
        size_bytes,
        last_indexed: chrono::Utc::now().timestamp(),
    };
    info!("[indexed_dir] path=\"{}\" files={} size={}", path, file_count, size_bytes);
    dirs.insert(path.to_string(), info);

    // Also mark immediate subdirectories as indexed (since they were scanned as part of this)
    if let Some(home_path) = dirs::home_dir() {
        let home_str = home_path.to_string_lossy().to_string();
        if path == home_str || path == "/" {
            // We scanned the home directory or root, mark all immediate subdirs
            if let Ok(entries) = std::fs::read_dir(&home_path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        let subdir_path = entry_path.to_string_lossy().to_string();
                        let subdir_name = entry_path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        // Skip hidden directories
                        if !subdir_name.starts_with('.') {
                            if !dirs.contains_key(&subdir_path) {
                                dirs.insert(subdir_path.clone(), IndexedDirInfo {
                                    path: subdir_path,
                                    file_count: 0, // Unknown count per subdir
                                    size_bytes: 0,
                                    last_indexed: chrono::Utc::now().timestamp(),
                                });
                                info!("[indexed_dir] subdir marked as indexed");
                            }
                        }
                    }
                }
            }
        }
    }

    save_indexed_dirs(&dirs)?;
    Ok(())
}

/// Scan existing database and rebuild indexed_dirs.json from file paths
pub fn rebuild_indexed_dirs_from_db() -> anyhow::Result<usize> {
    let paths = LMDB_STORE.get_all_paths()?;
    let mut dirs: HashMap<String, IndexedDirInfo> = HashMap::new();

    // Parse each path and count files per top-level directory
    let home_opt = dirs::home_dir();
    let home_str = home_opt.as_ref().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();

    for path in &paths {
        // Determine which top-level directory this file belongs to
        let top_dir = if path.starts_with(&home_str) {
            // Extract the immediate subdirectory of home
            let relative = &path[home_str.len()..];
            let parts: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
            if parts.is_empty() {
                home_str.clone()
            } else {
                // Find the first non-hidden directory
                let mut found = String::new();
                for part in &parts {
                    if !part.starts_with('.') {
                        found = format!("{}/{}", home_str.trim_end_matches('/'), part);
                        break;
                    }
                }
                if found.is_empty() {
                    home_str.clone()
                } else {
                    found
                }
            }
        } else {
            // Use parent path
            if let Some(parent) = std::path::Path::new(path).parent() {
                parent.to_string_lossy().to_string()
            } else {
                continue;
            }
        };

        // Increment count for this directory
        let entry = dirs.entry(top_dir.clone()).or_insert_with(|| IndexedDirInfo {
            path: top_dir,
            file_count: 0,
            size_bytes: 0,
            last_indexed: 0,
        });
        entry.file_count += 1;
    }

    let count = dirs.len();
    info!("Rebuilt indexed dirs from DB: {} directories, {} total files", count, paths.len());

    // Mark all immediate subdirs of home as indexed too
    if let Some(home_path) = &home_opt {
        if let Ok(entries) = std::fs::read_dir(home_path) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    let subdir_path = entry_path.to_string_lossy().to_string();
                    let subdir_name = entry_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if !subdir_name.starts_with('.') {
                        if !dirs.contains_key(&subdir_path) {
                            dirs.insert(subdir_path.clone(), IndexedDirInfo {
                                path: subdir_path,
                                file_count: 0,
                                size_bytes: 0,
                                last_indexed: 0,
                            });
                        }
                    }
                }
            }
        }
    }

    // Save to disk
    save_indexed_dirs(&dirs)?;

    Ok(count)
}

/// Get top-level directories for display (disk + one level)
fn build_top_level_dirs() -> Vec<DirInfo> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        // macOS top-level directories (under / or /Users/<name>)
        let home = dirs::home_dir();
        if let Some(home_path) = &home {
            // Get the username part
            if let Some(username) = home_path.file_name().and_then(|n| n.to_str()) {
                let home_parent = home_path.parent().unwrap_or(home_path);

                // Home directory parent (usually /Users)
                dirs.push(DirInfo {
                    name: format!("{}/", username),
                    path: home_parent.to_string_lossy().to_string(),
                    is_indexed: false,
                    is_indexing: false,
                    file_count: 0,
                    size_bytes: 0,
                });
            }

            // Common directories inside home
            let common_dirs = ["Desktop", "Documents", "Downloads", "Pictures",
                             "Music", "Movies", "Applications", "Library"];
            for dir_name in &common_dirs {
                let dir_path = home_path.join(dir_name);
                if dir_path.exists() && dir_path.is_dir() {
                    let file_count = count_files_in_dir(&dir_path);
                    dirs.push(DirInfo {
                        name: format!("~/{}", dir_name),
                        path: dir_path.to_string_lossy().to_string(),
                        is_indexed: false,
                        is_indexing: false,
                        file_count,
                        size_bytes: 0,
                    });
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        // For other platforms, just use home directory
        if let Some(home_path) = dirs::home_dir() {
            dirs.push(DirInfo {
                name: "~".to_string(),
                path: home_path.to_string_lossy().to_string(),
                is_indexed: false,
                is_indexing: false,
                file_count: 0,
                size_bytes: 0,
            });
        }
    }

    // Load indexed status from tracking file
    if let Ok(indexed) = load_indexed_dirs() {
        for dir in &mut dirs {
            if let Some(info) = indexed.get(&dir.path) {
                dir.is_indexed = true;
                dir.file_count = info.file_count;
                dir.size_bytes = info.size_bytes;
            }
        }

        // Also add directories from indexed that are NOT in the common list but have files
        for (path, info) in &indexed {
            // Skip if already in dirs (we already processed it above)
            if dirs.iter().any(|d: &DirInfo| &d.path == path) {
                continue;
            }

            // Skip hidden directories and system directories
            let name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if name.starts_with('.') || name.starts_with('C') || name.starts_with('L') {
                continue; // Skip hidden, Cache, Library, etc.
            }

            // Only add if has files
            if info.file_count > 0 {
                let display_name = format!("~/{}", name);
                dirs.push(DirInfo {
                    name: display_name,
                    path: path.clone(),
                    is_indexed: true,
                    is_indexing: false,
                    file_count: info.file_count,
                    size_bytes: info.size_bytes,
                });
            }
        }
    }

    dirs
}

/// Get directory status info for frontend
pub fn get_indexed_dirs_info() -> Vec<DirInfo> {
    // Check if we need to rebuild from database
    let dirs_file = get_indexed_dirs_file();
    if !dirs_file.exists() {
        info!("indexed_dirs.json not found, rebuilding from database...");
        if let Err(e) = rebuild_indexed_dirs_from_db() {
            warn!("Failed to rebuild indexed dirs from DB: {}", e);
        }
    }
    build_top_level_dirs()
}

/// Count files in a directory (fast estimate)
fn count_files_in_dir(path: &PathBuf) -> usize {
    std::fs::read_dir(path)
        .map(|entries| entries.count())
        .unwrap_or(0)
}

/// Directory info for frontend display (internal use)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirInfo {
    pub name: String,
    pub path: String,
    pub is_indexed: bool,
    pub is_indexing: bool,
    pub file_count: usize,
    pub size_bytes: u64,
}
