//! LMDB-based full metadata storage.
//!
//! Simplified architecture: Everything in LMDB
//! - path (key) → FullMetadata (value)
//! - LMDB uses mmap internally for fast access
//! - FST provides prefix search via mmap
//!
//! Memory: Uses mmap, OS manages caching automatically

use heed::{EnvOpenOptions, Database};
use heed::types::{Str, SerdeBincode};
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use anyhow::Context;
use tracing::info;

/// Default max LMDB map size: 500MB
/// Stores path → FullMetadata (id, size, mtime, is_dir, created)
/// With ~150 bytes per entry, this holds ~3M entries
const DEFAULT_MAX_MAP_SIZE: usize = 500 * 1024 * 1024;

/// Maximum entries
const DEFAULT_MAX_ENTRIES: usize = 3_000_000;

/// Full metadata stored in LMDB
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FullMetadata {
    pub id: u64,
    pub size: u64,
    pub is_directory: bool,
    pub created_at: i64,
    pub modified_at: i64,
}

/// LMDB database type
pub type HeedDatabase = Database<Str, SerdeBincode<FullMetadata>>;

pub struct LmdbStore {
    env: Arc<heed::Env>,
    db: Arc<HeedDatabase>,
    next_id: Arc<RwLock<u64>>,
    max_map_size: usize,
    max_entries: usize,
}

impl LmdbStore {
    pub fn new(path: &PathBuf) -> anyhow::Result<Self> {
        Self::with_config(path, DEFAULT_MAX_MAP_SIZE, DEFAULT_MAX_ENTRIES)
    }

    pub fn with_config(path: &PathBuf, max_map_size: usize, max_entries: usize) -> anyhow::Result<Self> {
        std::fs::create_dir_all(path)?;

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(1)
                .map_size(max_map_size)
                .open(path)
                .context("Failed to open LMDB environment")?
        };

        let mut wtxn = env.write_txn()?;
        let db: Database<Str, SerdeBincode<FullMetadata>> = env
            .create_database(&mut wtxn, Some("metadata"))?;
        wtxn.commit()?;

        info!("LMDB metadata store initialized at {:?} with max_map_size={}MB",
              path, max_map_size / 1024 / 1024);

        Ok(Self {
            env: Arc::new(env),
            db: Arc::new(db),
            next_id: Arc::new(RwLock::new(1)),
            max_map_size,
            max_entries,
        })
    }

    /// Insert full metadata, returns existing ID if path exists
    pub fn insert(&self, path: &str, metadata: &FullMetadata) -> anyhow::Result<u64> {
        let mut rw_txn = self.env.write_txn()?;

        // Check if exists first
        if let Some(existing) = self.db.get(&rw_txn, path)? {
            rw_txn.commit()?;
            return Ok(existing.id);
        }

        let id = metadata.id;
        self.db.put(&mut rw_txn, path, metadata)?;
        rw_txn.commit()?;
        Ok(id)
    }

    /// Batch insert - much faster for indexing
    pub fn insert_batch(&self, entries: &[(String, FullMetadata)]) -> anyhow::Result<Vec<u64>> {
        let mut rw_txn = self.env.write_txn()?;
        let mut ids = Vec::with_capacity(entries.len());

        for (path, metadata) in entries {
            if let Some(existing) = self.db.get(&rw_txn, path)? {
                ids.push(existing.id);
                continue;
            }
            self.db.put(&mut rw_txn, path, metadata)?;
            ids.push(metadata.id);
        }

        rw_txn.commit()?;
        Ok(ids)
    }

    /// Get metadata by path
    pub fn get_by_path(&self, path: &str) -> anyhow::Result<Option<FullMetadata>> {
        let ro_txn = self.env.read_txn()?;
        Ok(self.db.get(&ro_txn, path)?)
    }

    /// Get metadata by ID (requires scan)
    pub fn get_by_id(&self, id: u64) -> anyhow::Result<Option<(String, FullMetadata)>> {
        let ro_txn = self.env.read_txn()?;
        let mut iter = self.db.iter(&ro_txn)?;

        while let Some(result) = iter.next() {
            if let Ok((path, metadata)) = result {
                if metadata.id == id {
                    return Ok(Some((path.to_string(), metadata)));
                }
            }
        }
        Ok(None)
    }

    /// Get path by ID (for inverted index path resolution)
    pub fn get_path_by_id(&self, id: u64) -> anyhow::Result<Option<String>> {
        Ok(self.get_by_id(id)?.map(|(path, _)| path))
    }

    /// Delete by path
    pub fn delete(&self, path: &str) -> anyhow::Result<()> {
        let mut rw_txn = self.env.write_txn()?;
        self.db.delete(&mut rw_txn, path)?;
        rw_txn.commit()?;
        Ok(())
    }

    /// Clear all entries
    pub fn clear(&self) -> anyhow::Result<()> {
        let mut rw_txn = self.env.write_txn()?;
        self.db.clear(&mut rw_txn)?;
        rw_txn.commit()?;

        let mut next_id = self.next_id.write();
        *next_id = 1;
        Ok(())
    }

    /// Get entry count
    pub fn len(&self) -> anyhow::Result<u64> {
        let ro_txn = self.env.read_txn()?;
        Ok(self.db.len(&ro_txn)?)
    }

    /// Check if empty
    pub fn is_empty(&self) -> anyhow::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Get all paths and metadata for indexing
    pub fn get_all_entries(&self) -> anyhow::Result<Vec<(String, FullMetadata)>> {
        let ro_txn = self.env.read_txn()?;
        let mut iter = self.db.iter(&ro_txn)?;
        let mut results = Vec::new();

        while let Some(result) = iter.next() {
            if let Ok((path, metadata)) = result {
                results.push((path.to_string(), metadata));
            }
        }
        Ok(results)
    }

    /// Get all paths (for FST building)
    pub fn get_all_paths(&self) -> anyhow::Result<Vec<String>> {
        let ro_txn = self.env.read_txn()?;
        let mut iter = self.db.iter(&ro_txn)?;
        let mut paths = Vec::new();

        while let Some(result) = iter.next() {
            if let Ok((path, _)) = result {
                paths.push(path.to_string());
            }
        }
        Ok(paths)
    }

    /// Get all (id, path) pairs for inverted index
    pub fn get_all_id_paths(&self) -> anyhow::Result<Vec<(u64, String)>> {
        let ro_txn = self.env.read_txn()?;
        let mut iter = self.db.iter(&ro_txn)?;
        let mut results = Vec::new();

        while let Some(result) = iter.next() {
            if let Ok((path, metadata)) = result {
                results.push((metadata.id, path.to_string()));
            }
        }
        Ok(results)
    }

    /// Get next available ID
    pub fn next_id(&self) -> u64 {
        let mut id = self.next_id.write();
        let next = *id;
        *id += 1;
        next
    }

    /// Set next ID (for rebuilding)
    pub fn set_next_id(&self, id: u64) {
        let mut next = self.next_id.write();
        *next = id;
    }

    /// Check if path exists
    pub fn contains(&self, path: &str) -> anyhow::Result<bool> {
        let ro_txn = self.env.read_txn()?;
        Ok(self.db.get(&ro_txn, path)?.is_some())
    }

    /// Get stats
    pub fn get_stats(&self) -> anyhow::Result<IndexStats> {
        let count = self.len()?;
        Ok(IndexStats {
            file_count: count,
            max_file_count: self.max_entries as u64,
            max_map_size: self.max_map_size,
            is_near_capacity: count as usize > self.max_entries * 9 / 10,
        })
    }
}

/// Memory statistics
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IndexStats {
    pub file_count: u64,
    pub max_file_count: u64,
    pub max_map_size: usize,
    pub is_near_capacity: bool,
}
