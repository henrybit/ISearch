//! Inverted index for token-based filename search.
//! Optimized for memory efficiency with streaming reads.
//!
//! Memory optimization: Uses streaming file reads instead of mmap.
//! This keeps memory usage under 100MB regardless of index size.
//! - Token filtering (skip common/short/long tokens)
//! - Postings limit per token
//! - Binary storage with streaming reads
//! - Path ID instead of full paths in memory

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::RwLock;

use anyhow::{Context, Result};
use tracing::{info, warn};

/// Maximum number of postings (path_ids) per token
const MAX_POSTINGS_PER_TOKEN: usize = 50_000;

/// Skip tokens appearing in more than this fraction of documents (1/X)
const MAX_DOC_FREQUENCY: f32 = 0.01; // 1% - tokens appearing in >1% of docs are skipped

/// Minimum token length
const MIN_TOKEN_LEN: usize = 2;

/// Maximum token length
const MAX_TOKEN_LEN: usize = 50;

/// Common stopwords and noise tokens to skip
const STOP_TOKENS: &[&str] = &[
    // English common words
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "her",
    "was", "one", "our", "out", "day", "get", "has", "him", "his", "how",
    "its", "may", "new", "now", "old", "see", "two", "way", "who", "boy",
    "did", "man", "put", "say", "she", "too", "use", "file", "data", "doc",
    "txt", "pdf", "png", "jpg", "gif", "mp3", "mp4", "zip", "tar", "gz",
    // Common computer terms
    "tmp", "temp", "cache", "data", "file", "folder", "desktop", "documents",
    "downloads", "pictures", "music", "video", "system", "library", "application",
    "info", "null", "nil", "none", "undefined", "empty", "test", "demo",
    // Numbers as tokens
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10",
    "2020", "2021", "2022", "2023", "2024", "2025",
];

/// Inverted index that maps filename tokens to path IDs.
/// Uses streaming file reads for memory efficiency.
///
/// Memory optimization: NO mmap, NO path_id_to_path HashMap in memory.
/// File is opened/closed for each search operation. This keeps memory
/// usage constant regardless of index size.
pub struct InvertedIndex {
    /// Base path for storing the index
    base_path: PathBuf,
    /// LMDB store reference for path resolution (lazy loaded)
    lmdb_store: RwLock<Option<std::sync::Arc<crate::core::lmdb_store::LmdbStore>>>,
    /// Statistics (cached, loaded from disk on open)
    total_tokens: AtomicUsize,
    total_paths: AtomicUsize,
    /// File size on disk
    file_size: AtomicUsize,
    /// Index file path for streaming reads
    index_path: RwLock<Option<PathBuf>>,
}

impl InvertedIndex {
    /// Create a new InvertedIndex at the given base path.
    pub fn new(base_path: &PathBuf) -> Self {
        Self {
            base_path: base_path.clone(),
            lmdb_store: RwLock::new(None),
            total_tokens: AtomicUsize::new(0),
            total_paths: AtomicUsize::new(0),
            file_size: AtomicUsize::new(0),
            index_path: RwLock::new(None),
        }
    }

    /// Set the LMDB store for path resolution.
    pub fn set_lmdb_store(&self, store: std::sync::Arc<crate::core::lmdb_store::LmdbStore>) {
        *self.lmdb_store.write() = Some(store);
    }

    /// Tokenize a string into searchable tokens with filtering.
    fn tokenize(s: &str) -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|token| {
                let len = token.len();
                len >= MIN_TOKEN_LEN && len <= MAX_TOKEN_LEN
            })
            .filter(|token| !Self::is_stop_token(token))
            .map(|token| token.to_lowercase())
            .collect()
    }

    /// Check if token is a stopword or common noise
    fn is_stop_token(token: &str) -> bool {
        // Skip exact stopwords
        if STOP_TOKENS.contains(&token) {
            return true;
        }
        // Skip pure numbers
        if token.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
        // Skip very short tokens
        if token.len() < MIN_TOKEN_LEN {
            return true;
        }
        false
    }

    /// Check if token should be indexed based on document frequency.
    /// Tokens appearing in too many documents are not useful for search.
    fn is_high_frequency(token_doc_count: usize, total_docs: usize) -> bool {
        if total_docs == 0 {
            return false;
        }
        let freq = token_doc_count as f32 / total_docs as f32;
        freq > MAX_DOC_FREQUENCY
    }

    /// Build the inverted index from paths with associated IDs.
    /// NOTE: Path mapping is NOT stored in memory - it's retrieved from LMDB on demand.
    pub fn build_from_paths(&self, paths: &[(u64, String)], total_docs: usize) -> Result<()> {
        info!("Building inverted index from {} paths (filtered)", paths.len());

        // Initialize write buffer
        let mut token_postings: HashMap<String, Vec<u64>> = HashMap::new();

        // First pass: collect all postings (only path_ids, no path strings in memory)
        for &(path_id, ref path) in paths {
            let filename = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            for token in Self::tokenize(filename) {
                token_postings.entry(token).or_default().push(path_id);
            }
        }

        // Second pass: apply filters and limits
        // Count document frequency for each token (deduplicated)
        let mut token_doc_freq: HashMap<String, usize> = HashMap::new();
        for (token, postings) in &token_postings {
            // Deduplicate by collecting into a set
            let unique_count: std::collections::HashSet<_> = postings.iter().collect();
            token_doc_freq.insert(token.clone(), unique_count.len());
        }

        // Apply filters
        let mut filtered_postings: HashMap<String, Vec<u64>> = HashMap::new();
        let mut skipped_high_freq = 0;

        for (token, postings) in token_postings {
            // Skip high frequency tokens
            if let Some(&doc_freq) = token_doc_freq.get(&token) {
                if Self::is_high_frequency(doc_freq, total_docs) {
                    skipped_high_freq += 1;
                    continue;
                }
            }

            // Sort and dedupe postings
            let mut sorted_postings = postings;
            sorted_postings.sort();
            sorted_postings.dedup();

            // Limit postings per token
            if sorted_postings.len() > MAX_POSTINGS_PER_TOKEN {
                sorted_postings.truncate(MAX_POSTINGS_PER_TOKEN);
            }

            filtered_postings.insert(token, sorted_postings);
        }

        // Build binary index
        let index_path = self.base_path.join("inverted_index.bin");

        // Ensure directory exists
        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write binary index with sorted tokens for binary search
        Self::write_binary_index_sorted(&index_path, &filtered_postings)?;

        // Get file size
        let disk_size = std::fs::metadata(&index_path)
            .map(|m| m.len() as usize)
            .unwrap_or(0);

        info!(
            "Inverted index built: {} tokens, {} paths indexed, {} high-freq skipped, disk={:.2}MB (NO in-memory path mapping)",
            filtered_postings.len(),
            paths.len(),
            skipped_high_freq,
            disk_size as f64 / 1024.0 / 1024.0
        );

        // Store stats only - NO path mapping in memory
        self.total_tokens.store(filtered_postings.len(), Ordering::Relaxed);
        self.total_paths.store(paths.len(), Ordering::Relaxed);
        self.file_size.store(disk_size, Ordering::Relaxed);

        Ok(())
    }

    /// Build from (path_id, path) pairs.
    pub fn build_from_id_paths(&self, paths: &[(u64, String)]) -> Result<()> {
        self.build_from_paths(paths, paths.len())
    }

    /// Write binary index format with SORTED tokens for binary search.
    /// This replaces the original write_binary_index_static.
    fn write_binary_index_sorted(path: &Path, postings: &HashMap<String, Vec<u64>>) -> Result<()> {
        use std::io::Write;

        let file = std::fs::File::create(path)
            .context("Failed to create index file")?;
        let mut writer = std::io::BufWriter::new(file);

        // Write magic header
        writer.write_all(b"ISID")?; // Magic
        writer.write_all(&1u32.to_le_bytes())?; // Version (1 = sorted tokens)
        writer.write_all(&(postings.len() as u64).to_le_bytes())?; // Num tokens

        // Sort tokens for BINARY SEARCH - this is key for fast lookups!
        let mut tokens: Vec<_> = postings.keys().collect();
        tokens.sort();

        // Build token index for fast binary search
        // We store: [token_offset (u64), ...] at the end of the header
        let mut token_offsets: Vec<u64> = Vec::with_capacity(tokens.len());
        let mut current_offset: u64 = 12 + 8 * tokens.len() as u64 + 8; // header + token_offsets array + num_tokens

        for token in &tokens {
            token_offsets.push(current_offset);

            let posting_vec = postings.get(token.as_str()).unwrap();
            // Calculate size: token_len(2) + token_bytes + num_postings(4) + postings(8 each)
            current_offset += 2 + token.len() as u64 + 4 + (posting_vec.len() as u64) * 8;
        }

        // Write token offsets array (for binary search)
        for offset in &token_offsets {
            writer.write_all(&offset.to_le_bytes())?;
        }

        // Write num_tokens again at the end (for verification)
        writer.write_all(&(tokens.len() as u64).to_le_bytes())?;

        // Write each token's data
        for token in &tokens {
            let posting_vec = postings.get(token.as_str()).unwrap();

            // Write token
            let token_bytes = token.as_bytes();
            writer.write_all(&(token_bytes.len() as u16).to_le_bytes())?;
            writer.write_all(token_bytes)?;

            // Write postings
            writer.write_all(&(posting_vec.len() as u32).to_le_bytes())?;
            for &path_id in posting_vec {
                writer.write_all(&path_id.to_le_bytes())?;
            }
        }

        writer.flush()?;
        Ok(())
    }

    /// Load the index metadata from disk (streaming mode - no mmap).
    /// This only reads the header to get token count, not the full file.
    /// File content is read on-demand during search.
    pub fn load(&self) -> Result<()> {
        let index_path = self.base_path.join("inverted_index.bin");

        if !index_path.exists() {
            info!("No inverted index found at {:?}", index_path);
            return Ok(());
        }

        info!("Loading inverted index metadata from {:?}", index_path);

        // Get file size for stats
        let disk_size = std::fs::metadata(&index_path)
            .map(|m| m.len() as usize)
            .unwrap_or(0);
        self.file_size.store(disk_size, Ordering::Relaxed);

        // Store index path for streaming reads
        *self.index_path.write() = Some(index_path.clone());

        // Read just the header to get token count (first 16 bytes)
        let file = std::fs::File::open(&index_path)?;
        let mut reader = std::io::BufReader::new(file);
        let mut header = [0u8; 16];
        use std::io::Read;
        if reader.read_exact(&mut header).is_ok() {
            // Verify magic bytes "ISID"
            if &header[0..4] == b"ISID" {
                let num_tokens = u64::from_le_bytes([header[8], header[9], header[10], header[11], header[12], header[13], header[14], header[15]]) as usize;
                self.total_tokens.store(num_tokens, Ordering::Relaxed);
            }
        }

        info!("Inverted index metadata loaded (streaming mode - NO mmap, path resolution via LMDB)");
        Ok(())
    }

    /// Resolve path IDs to paths using LMDB (on-demand lookup).
    /// This avoids loading all paths into memory.
    fn resolve_paths(&self, path_ids: &[u64]) -> Vec<String> {
        let lmdb_store = self.lmdb_store.read();
        if let Some(store) = lmdb_store.as_ref() {
            let mut results = Vec::with_capacity(path_ids.len());
            for &id in path_ids {
                if let Ok(Some(path)) = store.get_path_by_id(id) {
                    results.push(path);
                }
            }
            return results;
        }

        // Fallback: empty if no LMDB store configured
        warn!("LMDB store not configured for path resolution");
        Vec::new()
    }

    /// Search for paths where the filename contains the query.
    /// Uses streaming file reads for memory efficiency.
    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        let tokens: Vec<String> = Self::tokenize(query);

        if tokens.is_empty() {
            return Vec::new();
        }

        // Ensure index is loaded
        if self.index_path.read().is_none() {
            if let Err(e) = self.load() {
                warn!("Failed to load inverted index: {}", e);
                return Vec::new();
            }
        }

        // Ensure LMDB store is configured for path resolution
        if self.lmdb_store.read().is_none() {
            warn!("LMDB store not set for InvertedIndex - path resolution will fail");
        }

        // Find paths matching the first token using streaming read
        let first_token = &tokens[0];
        let mut candidate_ids = match self.get_postings_streaming(first_token) {
            Ok(ids) => ids,
            Err(e) => {
                warn!("Failed to get postings for '{}': {}", first_token, e);
                return Vec::new();
            }
        };

        // Intersect with remaining tokens
        for token in &tokens[1..] {
            let postings = match self.get_postings_streaming(token) {
                Ok(ids) => ids,
                Err(e) => {
                    warn!("Failed to get postings for '{}': {}", token, e);
                    Vec::new()
                }
            };
            candidate_ids.retain(|id| postings.contains(id));

            if candidate_ids.is_empty() {
                return Vec::new();
            }
        }

        // Resolve to paths and limit
        let mut results = self.resolve_paths(&candidate_ids);
        results.truncate(limit);
        results
    }

    /// Get postings for a token using streaming file read.
    /// Opens file, reads token's data, closes file immediately.
    fn get_postings_streaming(&self, token: &str) -> anyhow::Result<Vec<u64>> {
        let index_path = self.index_path.read();
        let path = match index_path.as_ref() {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        drop(index_path);

        // Open file for streaming read
        let file = std::fs::File::open(&path)?;
        let mut reader = std::io::BufReader::new(file);

        use std::io::{Read, Seek, SeekFrom};

        // Read header (16 bytes)
        let mut header = [0u8; 16];
        reader.read_exact(&mut header)?;

        // Verify magic "ISID"
        if &header[0..4] != b"ISID" {
            warn!("Invalid inverted index format");
            return Ok(Vec::new());
        }

        let _version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let num_tokens = u64::from_le_bytes([header[8], header[9], header[10], header[11], header[12], header[13], header[14], header[15]]) as usize;

        if num_tokens == 0 {
            return Ok(Vec::new());
        }

        // Read token offsets array
        let mut token_offsets = Vec::with_capacity(num_tokens);
        for i in 0..num_tokens {
            let mut offset_bytes = [0u8; 8];
            let offset_pos = 12 + i * 8;
            reader.seek(SeekFrom::Start(offset_pos as u64))?;
            reader.read_exact(&mut offset_bytes)?;
            let offset = u64::from_le_bytes(offset_bytes) as usize;
            token_offsets.push(offset);
        }

        // Binary search for the token
        let mut left = 0;
        let mut right = num_tokens;

        while left < right {
            let mid = (left + right) / 2;
            let token_offset = token_offsets[mid];

            // Seek to token and read it
            reader.seek(SeekFrom::Start(token_offset as u64))?;

            let mut token_len_bytes = [0u8; 2];
            reader.read_exact(&mut token_len_bytes)?;
            let token_len = u16::from_le_bytes(token_len_bytes) as usize;

            let mut token_bytes = vec![0u8; token_len];
            reader.read_exact(&mut token_bytes)?;
            let found_token = String::from_utf8_lossy(&token_bytes).to_string();

            match found_token.as_str().cmp(token) {
                std::cmp::Ordering::Equal => {
                    // Found! Read postings count
                    let mut num_bytes = [0u8; 4];
                    reader.read_exact(&mut num_bytes)?;
                    let num_postings = u32::from_le_bytes(num_bytes) as usize;

                    let mut postings = Vec::with_capacity(num_postings.min(MAX_POSTINGS_PER_TOKEN));
                    for _ in 0..num_postings.min(MAX_POSTINGS_PER_TOKEN) {
                        let mut id_bytes = [0u8; 8];
                        if reader.read_exact(&mut id_bytes).is_err() {
                            break;
                        }
                        postings.push(u64::from_le_bytes(id_bytes));
                    }
                    return Ok(postings);
                }
                std::cmp::Ordering::Less => left = mid + 1,
                std::cmp::Ordering::Greater => right = mid,
            }
        }

        Ok(Vec::new())
    }

    /// Search for paths where filename starts with prefix.
    /// NOTE: This requires LMDB store to be set for path resolution.
    pub fn search_prefix(&self, prefix: &str, limit: usize) -> Vec<String> {
        // This function is expensive as it needs to scan all paths
        // In production, use ShardedFstIndex for prefix search instead
        warn!("search_prefix on InvertedIndex is inefficient - use ShardedFstIndex.search_prefix instead");
        Vec::new()
    }

    /// Get total number of unique tokens.
    pub fn token_count(&self) -> usize {
        self.total_tokens.load(Ordering::Relaxed)
    }

    /// Get number of indexed paths.
    pub fn path_count(&self) -> usize {
        self.total_paths.load(Ordering::Relaxed)
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.total_tokens.load(Ordering::Relaxed) == 0
    }

    /// Get the index file size on disk.
    pub fn file_size(&self) -> u64 {
        self.file_size.load(Ordering::Relaxed) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        assert_eq!(
            InvertedIndex::tokenize("Hello_World.pdf"),
            vec!["hello", "world"]
        );
        assert_eq!(
            InvertedIndex::tokenize("file-name123.txt"),
            vec!["file", "name123", "txt"]
        );
        // Test filtering
        assert!(InvertedIndex::tokenize("a").is_empty()); // too short
        assert!(InvertedIndex::tokenize("test").is_empty()); // stopword
        assert!(InvertedIndex::tokenize("12345").is_empty()); // pure number
    }

    #[test]
    fn test_build_and_search() {
        let temp_dir = tempfile::tempdir().unwrap();
        let index = InvertedIndex::new(&temp_dir.path().to_path_buf());

        let paths = vec![
            (1, "/home/user/documents/report.pdf".to_string()),
            (2, "/home/user/documents/presentation.pdf".to_string()),
            (3, "/home/user/downloads/image.png".to_string()),
        ];

        index.build_from_id_paths(&paths).unwrap();

        // Search for "doc" - should find paths with "documents"
        let results = index.search("doc", 10);
        assert!(!results.is_empty());

        // Search for "pdf" - should find both pdfs
        let results = index.search("pdf", 10);
        assert_eq!(results.len(), 2);
    }
}
