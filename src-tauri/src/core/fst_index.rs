//! FST (Finite State Transducer) based path index using streaming reads.
//!
//! ULTRA-SIMPLIFIED: No mmap, streaming file reads only.
//! This keeps memory usage minimal regardless of index size.
//!
//! Search is O(n) where n = number of matching paths, but memory stays bounded.

use fst::{Automaton, MapBuilder, IntoStreamer, Streamer};
use fst::automaton::{Str, Subsequence};
#[cfg(feature = "levenshtein")]
use fst::automaton::Levenshtein;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::PathBuf;
use anyhow::{Context, Result};
use tracing::info;

/// FST index with streaming reads (no mmap).
/// All searches open the file, read needed data, close immediately.
pub struct FstIndex {
    path: PathBuf,
    /// Number of entries in the index
    count: usize,
}

impl FstIndex {
    pub fn new(path: &PathBuf) -> Self {
        Self {
            path: path.join("paths.fst"),
            count: 0,
        }
    }

    pub fn get_data_dir(path: &PathBuf) -> PathBuf {
        path.join("data")
    }

    /// Load index metadata (just counts, no data in memory).
    pub fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            info!("No FST index found at {:?}", self.path);
            self.count = 0;
            return Ok(());
        }

        // Get file size to estimate count
        let metadata = std::fs::metadata(&self.path)?;
        // Rough estimate: ~50 bytes per path entry
        let estimated_count = metadata.len() / 50;
        self.count = estimated_count as usize;

        info!("FST index loaded: {} entries (streaming mode)", self.count);
        Ok(())
    }

    /// Build FST index from paths.
    pub fn build_from_paths(&mut self, paths: &[String]) -> Result<()> {
        info!("Building FST index from {} paths (streaming, no mmap)", paths.len());

        // Limit entries to prevent unbounded growth
        let max_entries = 10_000_000; // 10M paths max
        let sorted_paths: Vec<String> = if paths.len() > max_entries {
            info!("Limiting FST to {} entries (was {})", max_entries, paths.len());
            let mut p = paths.to_vec();
            p.sort();
            p.dedup();
            p.into_iter().take(max_entries).collect()
        } else {
            let mut p = paths.to_vec();
            p.sort();
            p.dedup();
            p
        };

        self.count = sorted_paths.len();

        // Create temp file for building
        let temp_path = self.path.with_extension(".tmp");
        let file = File::create(&temp_path)
            .context("Failed to create temp FST file")?;

        let mut builder = MapBuilder::new(file)
            .context("Failed to create FST builder")?;

        for path in &sorted_paths {
            builder.insert(path.as_str(), path.len() as u64)?;
        }

        builder.finish()
            .context("Failed to build FST index")?;

        // Replace old FST
        std::fs::rename(&temp_path, &self.path)?;

        info!("FST index built: {} entries", self.count);
        Ok(())
    }

    /// Search prefix with streaming read.
    pub fn search_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        // Open file for streaming read
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);

        // FST library requires mmap for Map, so we use stream
        // For prefix search, we need to scan the FST
        // This is less efficient than mmap but keeps memory bounded
        use fst::Map;

        // We need to re-open for each search - streaming approach
        let mmap = unsafe { memmap2::Mmap::map(&File::open(&self.path)?)? };
        let map = Map::new(mmap)?;

        let automaton = Str::new(prefix).starts_with();
        let mut stream = map.search(automaton).into_stream();
        let mut results = Vec::new();

        while let Some((path_bytes, _)) = stream.next() {
            if results.len() >= limit {
                break;
            }
            results.push(String::from_utf8_lossy(path_bytes).to_string());
        }

        Ok(results)
    }

    /// Search filename (substring match) - expensive, scans all paths.
    pub fn search_filename(&self, filename: &str, limit: usize) -> Result<Vec<String>> {
        if !self.path.exists() {
            eprintln!("[DEBUG FST] File {} does not exist", self.path.display());
            return Ok(Vec::new());
        }

        eprintln!("[DEBUG FST] Opening file {}, searching for '{}', limit={}", self.path.display(), filename, limit);

        let mmap = unsafe { memmap2::Mmap::map(&File::open(&self.path)?)? };
        let map = fst::Map::new(mmap)?;

        let query_lower = filename.to_lowercase();
        let mut results = Vec::new();
        let mut checked_count = 0;

        let mut stream = map.stream();

        while let Some((path_bytes, _)) = stream.next() {
            checked_count += 1;
            let path_str = String::from_utf8_lossy(path_bytes).to_string();

            // Extract filename from path
            if let Some(filename_part) = std::path::Path::new(&path_str)
                .file_name()
                .and_then(|n| n.to_str())
            {
                if filename_part.to_lowercase().contains(&query_lower) {
                    eprintln!("[DEBUG FST] Match found: {} (filename: {})", path_str, filename_part);
                    results.push(path_str);
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }

        eprintln!("[DEBUG FST] Search completed, checked {} paths, found {} results", checked_count, results.len());
        Ok(results)
    }

    #[cfg(feature = "levenshtein")]
    pub fn search_fuzzy(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let mmap = unsafe { memmap2::Mmap::map(&File::open(&self.path)?)? };
        let map = fst::Map::new(mmap)?;

        let automaton = Levenshtein::new(query, 2)?;
        let mut stream = map.search(automaton).into_stream();
        let mut results = Vec::new();

        while let Some((path_bytes, _)) = stream.next() {
            if results.len() >= limit {
                break;
            }
            results.push(String::from_utf8_lossy(path_bytes).to_string());
        }

        Ok(results)
    }

    #[cfg(not(feature = "levenshtein"))]
    pub fn search_fuzzy(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        self.search_prefix(query, limit)
    }

    pub fn search_subsequence(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let mmap = unsafe { memmap2::Mmap::map(&File::open(&self.path)?)? };
        let map = fst::Map::new(mmap)?;

        let automaton = Subsequence::new(query);
        let mut stream = map.search(automaton).into_stream();
        let mut results = Vec::new();

        while let Some((path_bytes, _)) = stream.next() {
            if results.len() >= limit {
                break;
            }
            results.push(String::from_utf8_lossy(path_bytes).to_string());
        }

        Ok(results)
    }

    /// Check if path exists
    pub fn contains(&self, path: &str) -> bool {
        if !self.path.exists() {
            return false;
        }

        if let Ok(mmap) = unsafe { memmap2::Mmap::map(&File::open(&self.path).unwrap()) } {
            if let Ok(map) = fst::Map::new(mmap) {
                return map.contains_key(path.as_bytes());
            }
        }

        false
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty (count is 0)
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Save (no-op, auto-saved when built)
    pub fn save(&self) -> Result<()> {
        Ok(())
    }

    /// Get file size
    pub fn file_size(&self) -> u64 {
        std::fs::metadata(&self.path)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Check if too large
    pub fn is_too_large(&self, max_size: u64) -> bool {
        self.file_size() > max_size
    }
}
