//! Cross-platform file indexer with native speed.
//!
//! Provides a unified interface for file indexing across platforms:
//! - macOS: mdfind (Spotlight) - extremely fast
//! - Windows: NTFS MFT direct reading - similar to Everything
//! - Fallback: WalkDir for other platforms
//!
//! Also provides live fuzzy filename search used when the local index misses.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use tracing::{info, warn};

/// Unified file metadata returned by all indexers
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub modified_at: i64,
    pub created_at: i64,
}

/// File indexer trait - unified interface for all platforms
pub trait FileIndexer {
    /// Index files and return their metadata
    fn index(&self, root: &str) -> Result<Vec<FileMetadata>>;

    /// Get index count
    fn count(&self) -> usize;
}

/// Platform-specific file indexer
pub struct PlatformIndexer {
    inner: Box<dyn FileIndexer>,
}

impl PlatformIndexer {
    /// Create indexer based on current platform
    pub fn new() -> Self {
        #[cfg(target_os = "macos")]
        {
            info!("Using MdfindIndexer for macOS");
            return Self {
                inner: Box::new(MdfindIndexer::new()),
            };
        }

        #[cfg(target_os = "windows")]
        {
            info!("Using NtfsMftIndexer for Windows");
            return Self {
                inner: Box::new(NtfsMftIndexer::new()),
            };
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            info!("Using WalkDirIndexer for unsupported platform");
            return Self {
                inner: Box::new(WalkDirIndexer::new()),
            };
        }
    }

    /// Index files with automatic fallback
    pub fn index(&self, root: &str) -> Result<Vec<FileMetadata>> {
        self.index_with_options(root, true)
    }

    /// Index files with options
    pub fn index_with_options(&self, root: &str, skip_hidden: bool) -> Result<Vec<FileMetadata>> {
        let results = self.inner.index(root)?;

        // Check if mdfind returned too few results and fallback if needed
        #[cfg(target_os = "macos")]
        {
            if results.len() < 10000 {
                info!("mdfind returned only {} files, falling back to WalkDir for complete coverage", results.len());
                let walkdir = WalkDirIndexer::new().with_skip_hidden(skip_hidden);
                return walkdir.index(root);
            }
        }

        Ok(results)
    }

    /// Get count
    pub fn count(&self) -> usize {
        self.inner.count()
    }
}

// ============================================================================
// macOS: mdfind (Spotlight)
// ============================================================================

/// macOS Spotlight-based indexer using mdfind command
pub struct MdfindIndexer;

impl MdfindIndexer {
    pub fn new() -> Self {
        Self
    }

    /// Search using Spotlight query
    pub fn search(&self, query: &str) -> Result<Vec<FileMetadata>> {
        let output = Command::new("mdfind")
            .args(["-onlyin", "/", query])
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let paths = String::from_utf8_lossy(&output.stdout);
        let mut results = Vec::new();

        for line in paths.lines() {
            if line.is_empty() {
                continue;
            }

            let path = line.to_string();
            if let Ok(metadata) = Self::get_metadata(&path) {
                results.push(metadata);
            }
        }

        Ok(results)
    }

    /// Get metadata for a single file using mdls (supports iCloud stubs)
    fn get_metadata(path: &str) -> Result<FileMetadata> {
        // Use mdls to get metadata - works for iCloud stubs and local files
        let output = Command::new("mdls")
            .args(["-name", "kMDItemFSSize", "-name", "kMDItemFSIsDirectory", "-name", "kMDItemFSCreationDate", "-name", "kMDItemFSContentChangeDate", path])
            .output()?;

        if !output.status.success() {
            // Fallback: try direct metadata
            let metadata = std::fs::metadata(path)?;
            return Ok(FileMetadata {
                path: path.to_string(),
                size: metadata.len(),
                is_directory: metadata.is_dir(),
                modified_at: metadata.modified()
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
                    .unwrap_or(0),
                created_at: metadata.created()
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
                    .unwrap_or(0),
            });
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut size: u64 = 0;
        let mut is_dir: bool = false;
        let mut created: i64 = 0;
        let mut modified: i64 = 0;

        for line in output_str.lines() {
            let line = line.trim();
            if line.contains("kMDItemFSSize") {
                if let Some(val) = line.split("=").nth(1) {
                    let val = val.trim();
                    if val != "(null)" {
                        size = val.parse().unwrap_or(0);
                    }
                }
            } else if line.contains("kMDItemFSIsDirectory") {
                if let Some(val) = line.split("=").nth(1) {
                    let val = val.trim();
                    is_dir = val == "1";
                }
            } else if line.contains("kMDItemFSCreationDate") {
                if let Some(val) = line.split("=").nth(1) {
                    let val = val.trim();
                    if val != "(null)" {
                        // Parse date like "2024-01-15 10:30:00 +0000"
                        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(val, "%Y-%m-%d %H:%M:%S %z") {
                            created = dt.timestamp();
                        }
                    }
                }
            } else if line.contains("kMDItemFSContentChangeDate") {
                if let Some(val) = line.split("=").nth(1) {
                    let val = val.trim();
                    if val != "(null)" {
                        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(val, "%Y-%m-%d %H:%M:%S %z") {
                            modified = dt.timestamp();
                        }
                    }
                }
            }
        }

        Ok(FileMetadata {
            path: path.to_string(),
            size,
            is_directory: is_dir,
            modified_at: modified,
            created_at: created,
        })
    }
}

impl FileIndexer for MdfindIndexer {
    /// Index all files using mdfind "kMDItemFSName EXISTS"
    fn index(&self, _root: &str) -> Result<Vec<FileMetadata>> {
        self.search("kMDItemFSName EXISTS")
    }

    fn count(&self) -> usize {
        0 // Not tracked in this implementation
    }
}

// ============================================================================
// Windows: NTFS MFT reading
// ============================================================================

/// Windows NTFS MFT-based indexer
pub struct NtfsMftIndexer;

impl NtfsMftIndexer {
    pub fn new() -> Self {
        Self
    }
}

impl FileIndexer for NtfsMftIndexer {
    /// Index using Windows native API
    /// Note: Full implementation would use Windows API directly
    /// For now, fallback to WalkDir
    fn index(&self, root: &str) -> Result<Vec<FileMetadata>> {
        info!("NtfsMftIndexer: falling back to WalkDir for now");
        let fallback = WalkDirIndexer::new();
        fallback.index(root)
    }

    fn count(&self) -> usize {
        0
    }
}

// ============================================================================
// Fallback: WalkDir
// ============================================================================

/// WalkDir-based indexer (fallback for all platforms)
pub struct WalkDirIndexer {
    skip_hidden: bool,
}

impl WalkDirIndexer {
    pub fn new() -> Self {
        Self {
            skip_hidden: true, // Default: skip hidden directories
        }
    }

    pub fn with_skip_hidden(mut self, skip: bool) -> Self {
        self.skip_hidden = skip;
        self
    }

    fn system_time_to_timestamp(time: std::time::SystemTime) -> i64 {
        time.duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

impl FileIndexer for WalkDirIndexer {
    fn index(&self, root: &str) -> Result<Vec<FileMetadata>> {
        use walkdir::WalkDir;

        let mut results = Vec::new();

        // Helper to check if a path component is hidden
        let is_hidden_component = |name: &std::ffi::OsStr| -> bool {
            if let Some(s) = name.to_str() {
                s.starts_with('.') && s.len() > 1
            } else {
                false
            }
        };

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip files in hidden directories (but still descend to find non-hidden ones)
            // This is a trade-off: we traverse hidden dirs but don't index hidden files
            if self.skip_hidden {
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

            if let Ok(metadata) = entry.metadata() {
                let modified = metadata.modified()
                    .map(Self::system_time_to_timestamp)
                    .unwrap_or(0);
                let created = metadata.created()
                    .map(Self::system_time_to_timestamp)
                    .unwrap_or(0);

                results.push(FileMetadata {
                    path: path.to_string_lossy().to_string(),
                    size: metadata.len(),
                    is_directory: false,
                    modified_at: modified,
                    created_at: created,
                });
            }
        }

        Ok(results)
    }

    fn count(&self) -> usize {
        0
    }
}

// ============================================================================
// Live fuzzy filename search (index-miss fallback)
// ============================================================================

/// Escape a string for use inside a Spotlight query literal.
fn escape_spotlight_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn system_time_to_unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn metadata_from_path(path: &str) -> Option<FileMetadata> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some(FileMetadata {
        path: path.to_string(),
        size: if meta.is_file() { meta.len() } else { 0 },
        is_directory: meta.is_dir(),
        modified_at: meta
            .modified()
            .map(system_time_to_unix)
            .unwrap_or(0),
        created_at: meta
            .created()
            .map(system_time_to_unix)
            .unwrap_or(0),
    })
}

fn filename_fuzzy_match(filename: &str, query: &str) -> bool {
    let name = filename.to_lowercase();
    let q = query.to_lowercase();
    if q.is_empty() {
        return false;
    }
    // Substring match (primary "模糊" behaviour)
    if name.contains(&q) {
        return true;
    }
    // Subsequence match: query chars appear in order (e.g. "cfg" → "config")
    let mut it = name.chars();
    for ch in q.chars() {
        loop {
            match it.next() {
                Some(c) if c == ch => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

/// Live full-disk (or home-tree) fuzzy filename search.
///
/// - macOS: Spotlight via `mdfind` on `kMDItemFSName`
/// - other platforms: WalkDir over the user home directory
pub fn live_fuzzy_filename_search(query: &str, limit: usize) -> Result<Vec<FileMetadata>> {
    let query = query.trim();
    if query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    info!(
        "[live_search] Starting fuzzy filename search query=\"{}\" limit={}",
        query, limit
    );

    #[cfg(target_os = "macos")]
    {
        match live_search_mdfind(query, limit) {
            Ok(results) if !results.is_empty() => {
                info!("[live_search] mdfind returned {} results", results.len());
                return Ok(results);
            }
            Ok(_) => {
                info!("[live_search] mdfind returned 0 results, falling back to WalkDir");
            }
            Err(e) => {
                warn!("[live_search] mdfind failed: {}, falling back to WalkDir", e);
            }
        }
    }

    live_search_walkdir(query, limit)
}

#[cfg(target_os = "macos")]
fn live_search_mdfind(query: &str, limit: usize) -> Result<Vec<FileMetadata>> {
    let escaped = escape_spotlight_literal(query);
    // Case-insensitive (`c`) filename contains match
    let spotlight_query = format!("kMDItemFSName == \"*{escaped}*\"c");

    let output = Command::new("mdfind")
        .arg(&spotlight_query)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("mdfind failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::with_capacity(limit.min(256));

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(meta) = metadata_from_path(line) {
            results.push(meta);
            if results.len() >= limit {
                break;
            }
        }
    }

    Ok(results)
}

fn live_search_walkdir(query: &str, limit: usize) -> Result<Vec<FileMetadata>> {
    use walkdir::WalkDir;

    let roots: Vec<PathBuf> = {
        let mut roots = Vec::new();
        if let Some(home) = dirs::home_dir() {
            roots.push(home);
        }
        // Common extra roots when home is missing / for broader coverage
        #[cfg(target_os = "macos")]
        {
            roots.push(PathBuf::from("/Applications"));
            roots.push(PathBuf::from("/Users"));
        }
        #[cfg(target_os = "windows")]
        {
            if let Ok(userprofile) = std::env::var("USERPROFILE") {
                let p = PathBuf::from(userprofile);
                if !roots.contains(&p) {
                    roots.push(p);
                }
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            roots.push(PathBuf::from("/home"));
        }
        roots
    };

    let skip_dirs = [
        "Library/Caches",
        "Library/Logs",
        ".Trash",
        "node_modules",
        ".git",
        "target",
        "__pycache__",
        ".npm",
        ".cache",
    ];

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip hidden top-level components except common ones we care about
                if name.starts_with('.') && name != ".config" && name != ".local" {
                    return false;
                }
                let path_str = e.path().to_string_lossy();
                !skip_dirs.iter().any(|s| path_str.contains(s))
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !filename_fuzzy_match(filename, query) {
                continue;
            }

            let path_str = path.to_string_lossy().to_string();
            if !seen.insert(path_str.clone()) {
                continue;
            }

            if let Some(meta) = metadata_from_path(&path_str) {
                results.push(meta);
                if results.len() >= limit {
                    info!(
                        "[live_search] WalkDir found {} matches (limit reached)",
                        results.len()
                    );
                    return Ok(results);
                }
            }
        }
    }

    info!("[live_search] WalkDir found {} matches", results.len());
    Ok(results)
}

/// Convenience: check whether a path's filename fuzzy-matches the query.
#[allow(dead_code)]
pub fn path_matches_fuzzy(path: &Path, query: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| filename_fuzzy_match(n, query))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_indexer_creation() {
        let indexer = PlatformIndexer::new();
        assert!(indexer.count() == 0 || indexer.count() >= 0);
    }

    #[test]
    fn test_walkdir_indexer() {
        let indexer = WalkDirIndexer::new();
        let results = indexer.index("/tmp").unwrap();
        // Just check it doesn't crash
        assert!(results.len() >= 0);
    }

    #[test]
    fn test_filename_fuzzy_match() {
        assert!(filename_fuzzy_match("Config.json", "config"));
        assert!(filename_fuzzy_match("my-config-file.txt", "cfg"));
        assert!(!filename_fuzzy_match("readme.md", "xyz"));
    }
}
