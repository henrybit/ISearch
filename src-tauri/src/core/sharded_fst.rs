//! Simplified FST index (single file, no sharding).
//!
//! ULTRA-SIMPLIFIED: Single FST file with streaming reads.
//! Removed sharding complexity - memory stays bounded regardless of size.

use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;

use tracing::info;

use crate::core::fst_index::FstIndex;

/// Simplified FST index that wraps single-file FstIndex.
/// All searches use streaming reads for minimal memory.
pub struct ShardedFstIndex {
    /// Single FST index
    index: RwLock<Arc<FstIndex>>,
    /// Base path
    base_path: PathBuf,
}

impl ShardedFstIndex {
    pub fn new(base_path: &PathBuf) -> Self {
        Self {
            index: RwLock::new(Arc::new(FstIndex::new(base_path))),
            base_path: base_path.clone(),
        }
    }

    /// Build from paths
    pub fn build_from_paths(&self, paths: &[String]) -> anyhow::Result<()> {
        info!("Building FST index from {} paths", paths.len());

        let mut index = FstIndex::new(&self.base_path);
        index.build_from_paths(paths)?;

        *self.index.write() = Arc::new(index);

        info!("FST index built");
        Ok(())
    }

    /// Search prefix
    pub fn search_prefix(&self, prefix: &str, limit: usize) -> Vec<String> {
        self.index.read()
            .search_prefix(prefix, limit)
            .unwrap_or_default()
    }

    /// Search all (for regex/fuzzy - scans all entries)
    pub fn search_all(&self, limit: usize) -> Vec<String> {
        self.index.read()
            .search_prefix("", limit)
            .unwrap_or_default()
    }

    /// Search filename (substring match)
    pub fn search_filename(&self, filename: &str, limit: usize) -> Vec<String> {
        eprintln!("[DEBUG SHARDED] search_filename called with '{}', limit={}", filename, limit);
        let results = self.index.read()
            .search_filename(filename, limit)
            .unwrap_or_default();
        eprintln!("[DEBUG SHARDED] search_filename returned {} results", results.len());
        results
    }

    #[cfg(feature = "levenshtein")]
    pub fn search_fuzzy(&self, query: &str, limit: usize) -> Vec<String> {
        self.index.read()
            .search_fuzzy(query, limit)
            .unwrap_or_default()
    }

    #[cfg(not(feature = "levenshtein"))]
    pub fn search_fuzzy(&self, query: &str, limit: usize) -> Vec<String> {
        self.search_prefix(query, limit)
    }

    /// Check if path exists
    pub fn contains(&self, path: &str) -> bool {
        self.index.read().contains(path)
    }

    /// Total entries
    pub fn len(&self) -> usize {
        self.index.read().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.index.read().is_empty()
    }

    /// Number of shards (always 1 now)
    pub fn shard_count(&self) -> usize {
        1
    }

    /// Save
    pub fn save(&self) -> anyhow::Result<()> {
        self.index.read().save()
    }

    /// File size on disk
    pub fn file_size(&self) -> u64 {
        self.index.read().file_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_build_and_search() {
        let temp_dir = tempfile::tempdir().unwrap();
        let index = ShardedFstIndex::new(&temp_dir.path().to_path_buf());

        let paths = vec![
            "alpha/file.txt".to_string(),
            "beta/file.txt".to_string(),
            "gamma/file.txt".to_string(),
        ];

        index.build_from_paths(&paths).unwrap();

        assert!(index.contains("alpha/file.txt"));
        assert!(index.contains("beta/file.txt"));
        assert!(!index.contains("zeta/file.txt"));
    }
}
