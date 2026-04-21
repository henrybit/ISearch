//! Search engine with memory-efficient pagination.
//!
//! ULTRA-SIMPLIFIED: Results are paginated to limit memory usage.
//! Max 10MB buffer for search results.

use std::path::PathBuf;
use anyhow::Result;
use tracing::{debug, info};

use crate::core::database;
use crate::models::FileEntry;

/// Maximum result buffer size (10MB)
const MAX_RESULT_BUFFER: usize = 10 * 1024 * 1024;

/// Average bytes per entry for buffer calculation
const ESTIMATED_BYTES_PER_ENTRY: usize = 500;

/// Max entries in result buffer
const MAX_ENTRIES_IN_BUFFER: usize = MAX_RESULT_BUFFER / ESTIMATED_BYTES_PER_ENTRY;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub path_filter: Option<String>,
    pub ext_filter: Option<String>,
    pub regex_filter: Option<String>,
    pub fuzzy: bool,
    pub size_filter: Option<SizeFilter>,
    pub date_filter: Option<DateFilter>,
    pub sort_by: SortBy,
    pub sort_desc: bool,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub enum SizeFilter {
    GreaterThan(u64),
    LessThan(u64),
    Equals(u64),
}

#[derive(Debug, Clone)]
pub enum DateFilter {
    ModifiedAfter(i64),
    ModifiedBefore(i64),
    CreatedAfter(i64),
    CreatedBefore(i64),
}

#[derive(Debug, Clone)]
pub enum SortBy {
    Name,
    Size,
    Modified,
    Created,
    Relevance,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            path_filter: None,
            ext_filter: None,
            regex_filter: None,
            fuzzy: false,
            size_filter: None,
            date_filter: None,
            sort_by: SortBy::Relevance,
            sort_desc: true,
            limit: 1000,
            offset: 0,
        }
    }
}

pub struct SearchEngine;

impl SearchEngine {
    pub fn new() -> Self {
        Self
    }

    fn parse_size(size_str: &str) -> Option<u64> {
        let size_str = size_str.trim();

        if size_str.len() < 2 {
            return size_str.parse().ok();
        }

        let number: f64 = size_str[..size_str.len()-2].parse().ok()?;
        let unit = &size_str[size_str.len()-2..].to_uppercase();

        match unit.as_str() {
            "KB" => Some((number * 1024.0) as u64),
            "MB" => Some((number * 1024.0 * 1024.0) as u64),
            "GB" => Some((number * 1024.0 * 1024.0 * 1024.0) as u64),
            "TB" => Some((number * 1024.0 * 1024.0 * 1024.0 * 1024.0) as u64),
            "B" => Some(number as u64),
            "K" => Some((number * 1024.0) as u64),
            "M" => Some((number * 1024.0 * 1024.0) as u64),
            "G" => Some((number * 1024.0 * 1024.0 * 1024.0) as u64),
            "T" => Some((number * 1024.0 * 1024.0 * 1024.0 * 1024.0) as u64),
            _ => size_str.parse().ok(),
        }
    }

    pub fn parse_query(query_str: &str) -> SearchQuery {
        let mut query = SearchQuery::default();
        let query_str = query_str.trim();

        if query_str.is_empty() {
            return query;
        }

        let mut remaining = String::new();
        let parts: Vec<&str> = query_str.split_whitespace().collect();

        for part in parts {
            if part.starts_with("path:") {
                query.path_filter = Some(part[5..].to_string());
            } else if part.starts_with("ext:") || part.starts_with("suffix:") {
                let ext = part[part.find(':').unwrap() + 1..].to_string();
                if !ext.is_empty() {
                    query.ext_filter = Some(ext.trim_start_matches('.').to_lowercase());
                }
            } else if part.starts_with("regex:") {
                query.regex_filter = Some(part[6..].to_string());
            } else if part == "fuzzy" || part.starts_with("fuzzy:") {
                query.fuzzy = true;
                if part.contains(':') {
                    let keyword = part.split_at(part.find(':').unwrap() + 1).1;
                    if !remaining.is_empty() {
                        remaining.push(' ');
                    }
                    remaining.push_str(keyword);
                }
            } else if part.starts_with("size:>") {
                if let Some(size) = Self::parse_size(&part[6..]) {
                    query.size_filter = Some(SizeFilter::GreaterThan(size));
                }
            } else if part.starts_with("size:<") {
                if let Some(size) = Self::parse_size(&part[6..]) {
                    query.size_filter = Some(SizeFilter::LessThan(size));
                }
            } else if part.starts_with("size:") {
                if let Some(size) = Self::parse_size(&part[5..]) {
                    query.size_filter = Some(SizeFilter::Equals(size));
                }
            } else if part.starts_with("modified:>") {
                query.date_filter = Some(DateFilter::ModifiedAfter(0));
            } else if part.starts_with("modified:<") {
                query.date_filter = Some(DateFilter::ModifiedBefore(i64::MAX));
            } else if part.starts_with("created:>") {
                query.date_filter = Some(DateFilter::CreatedAfter(0));
            } else if part.starts_with("created:<") {
                query.date_filter = Some(DateFilter::CreatedBefore(i64::MAX));
            } else if part.starts_with("sort:") {
                let sort_part = &part[5..].to_lowercase();
                let (field, desc) = if sort_part.ends_with(":desc") {
                    (&sort_part[..sort_part.len() - 5], true)
                } else if sort_part.ends_with(":asc") {
                    (&sort_part[..sort_part.len() - 4], false)
                } else {
                    (sort_part.as_str(), query.sort_desc)
                };

                query.sort_by = match field {
                    "name" | "filename" => SortBy::Name,
                    "size" => SortBy::Size,
                    "modified" | "date" => SortBy::Modified,
                    "created" => SortBy::Created,
                    _ => SortBy::Relevance,
                };
                query.sort_desc = desc;
            } else if part == "desc" || part == "descending" {
                query.sort_desc = true;
            } else if part == "asc" || part == "ascending" {
                query.sort_desc = false;
            } else {
                if !remaining.is_empty() {
                    remaining.push(' ');
                }
                remaining.push_str(part);
            }
        }

        query.text = remaining;

        if query.text.is_empty() && matches!(query.sort_by, SortBy::Relevance) {
            query.sort_by = SortBy::Name;
        }

        query
    }

    pub fn search(&self, query: &SearchQuery) -> Result<Vec<FileEntry>> {
        eprintln!("[DEBUG] search() called with query: {:?}", query);

        info!("[search] Searching with query: {:?}", query);

        // If no text query, return all files
        if query.text.is_empty() && query.ext_filter.is_none() && query.regex_filter.is_none() {
            info!("[search] No query text, returning all files");
            return self.get_all_files(query);
        }

        let sharded_fst = database::get_sharded_fst_index();
        let inverted_index = database::get_inverted_index();

        let fst_empty = sharded_fst.is_empty();
        let fst_len = sharded_fst.len();
        let inv_empty = inverted_index.read().is_empty();
        eprintln!("[DEBUG] FST is_empty: {}, len: {}, inverted_index is_empty: {}", fst_empty, fst_len, inv_empty);
        info!("[search] FST is_empty: {}, len: {}, inverted_index is_empty: {}", fst_empty, fst_len, inv_empty);

        // Search using indexes, or fallback to direct LMDB search if indexes are empty
        let search_results = if fst_empty || inv_empty {
            // Indexes not ready - fallback to direct search
            info!("[search] Indexes empty (fst_empty={}, inv_empty={}), falling back to direct LMDB search", fst_empty, inv_empty);
            self.direct_search(&query.text, query.limit * 2)?
        } else if query.fuzzy {
            info!("[search] Using fuzzy search");
            sharded_fst.search_fuzzy(&query.text, query.limit * 2)
        } else if query.text.contains('*') || query.text.contains('?') {
            let prefix = query.text.trim_end_matches('*').trim_end_matches('?');
            info!("[search] Using prefix search with prefix: {}", prefix);
            sharded_fst.search_prefix(prefix, query.limit * 2)
        } else {
            // Use search_filename for substring matching on filename
            eprintln!("[DEBUG] Using filename search for: {}", query.text);
            info!("[search] Using filename search for: {}", query.text);
            let mut results = sharded_fst.search_filename(&query.text, query.limit * 2);
            info!("[search] FST filename search returned {} results", results.len());

            // If no results, try inverted index
            if results.is_empty() {
                let inv_results = inverted_index.read().search(&query.text, query.limit * 2);
                info!("[search] Inverted index search returned {} results", inv_results.len());
                results = inv_results;
            }

            results
        };

        info!("[search] Total search_results (paths): {}", search_results.len());

        // Calculate effective limit based on memory budget
        let effective_limit = query.limit.min(MAX_ENTRIES_IN_BUFFER);
        let search_to_fetch: Vec<String> = search_results
            .into_iter()
            .skip(query.offset)
            .take(effective_limit)
            .collect();

        // Fetch metadata for each path
        let mut entries = Vec::new();
        for path in search_to_fetch {
            if let Ok(Some(meta)) = database::get_file_by_path(&path) {
                let path_buf = PathBuf::from(&meta.path);
                let filename = path_buf
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let extension = path_buf
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_string());

                entries.push(FileEntry {
                    id: meta.id as i64,
                    path: path_buf,
                    filename,
                    extension,
                    size: meta.size,
                    is_directory: meta.is_directory,
                    created_at: chrono::DateTime::from_timestamp(meta.created_at, 0).unwrap_or_default(),
                    modified_at: chrono::DateTime::from_timestamp(meta.modified_at, 0).unwrap_or_default(),
                    indexed_at: chrono::DateTime::from_timestamp(0, 0).unwrap_or_default(),
                });
            }
        }

        // Apply filters
        self.apply_filters(&mut entries, query);

        // Sort
        self.sort_entries(&mut entries, query);

        Ok(entries)
    }

    fn get_all_files(&self, query: &SearchQuery) -> Result<Vec<FileEntry>> {
        let sort_by_str = match query.sort_by {
            SortBy::Name => "name",
            SortBy::Size => "size",
            SortBy::Modified => "modified",
            SortBy::Created => "created",
            SortBy::Relevance => "name",
        };

        // Use iterator for memory efficiency
        let effective_limit = query.limit.min(MAX_ENTRIES_IN_BUFFER);
        let iter = database::iter_files(sort_by_str, query.sort_desc, query.offset, effective_limit)?;

        let entries: Vec<FileEntry> = iter.map(|meta| {
            let path_buf = PathBuf::from(&meta.path);
            let filename = path_buf.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let extension = path_buf.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string());

            FileEntry {
                id: meta.id as i64,
                path: path_buf,
                filename,
                extension,
                size: meta.size,
                is_directory: meta.is_directory,
                created_at: chrono::DateTime::from_timestamp(meta.created_at, 0).unwrap_or_default(),
                modified_at: chrono::DateTime::from_timestamp(meta.modified_at, 0).unwrap_or_default(),
                indexed_at: chrono::DateTime::from_timestamp(0, 0).unwrap_or_default(),
            }
        }).collect();

        Ok(entries)
    }

    /// Direct search in LMDB when indexes are not ready
    fn direct_search(&self, text: &str, limit: usize) -> Result<Vec<String>> {
        use crate::core::database::LMDB_STORE;

        let entries = LMDB_STORE.get_all_entries()?;
        let text_lower = text.to_lowercase();

        let mut results: Vec<String> = entries
            .into_iter()
            .filter(|(_, _meta)| {
                // This filter is applied below
                true
            })
            .filter_map(|(path, _meta)| {
                // Check if path contains the search text (case insensitive)
                if path.to_lowercase().contains(&text_lower) {
                    Some(path)
                } else {
                    None
                }
            })
            .take(limit)
            .collect();

        Ok(results)
    }

    fn apply_filters(&self, entries: &mut Vec<FileEntry>, query: &SearchQuery) {
        // Extension filter
        if let Some(ref ext) = query.ext_filter {
            entries.retain(|e| {
                if let Some(ref file_ext) = e.extension {
                    file_ext.to_lowercase() == *ext
                } else {
                    false
                }
            });
        }

        // Regex filter
        if let Some(ref regex_pattern) = query.regex_filter {
            if let Ok(re) = regex::Regex::new(regex_pattern) {
                entries.retain(|e| {
                    re.is_match(&e.filename) || re.is_match(&e.path.to_string_lossy())
                });
            }
        }

        // Path filter
        if let Some(ref path_filter) = query.path_filter {
            entries.retain(|e| {
                e.path.to_string_lossy().contains(path_filter)
            });
        }

        // Size filter
        if let Some(ref size_filter) = query.size_filter {
            entries.retain(|e| match size_filter {
                SizeFilter::GreaterThan(s) => e.size > *s,
                SizeFilter::LessThan(s) => e.size < *s,
                SizeFilter::Equals(s) => e.size == *s,
            });
        }
    }

    fn sort_entries(&self, entries: &mut Vec<FileEntry>, query: &SearchQuery) {
        match query.sort_by {
            SortBy::Name => entries.sort_by(|a, b| a.filename.cmp(&b.filename)),
            SortBy::Size => entries.sort_by(|a, b| a.size.cmp(&b.size)),
            SortBy::Modified => entries.sort_by(|a, b| a.modified_at.cmp(&b.modified_at)),
            SortBy::Created => entries.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
            SortBy::Relevance => {}
        }

        if query.sort_desc {
            entries.reverse();
        }
    }
}
