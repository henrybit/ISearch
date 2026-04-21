//! Metadata store on disk (path_store.bin).
//!
//! Simple sequential file storage for metadata.
//! Each record is exactly 128 bytes for easy random access.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use anyhow::Result;
use tracing::{info, warn};

/// Fixed record size: 128 bytes per metadata entry
/// Layout: id(8) + size(8) + modified_at(8) + created_at(8) + is_dir(1) + path_len(2) + path(93)
const RECORD_SIZE: usize = 128;

/// Maximum path length: 128 - 35 = 93 bytes
const MAX_PATH_LEN: usize = 93;

/// Metadata record on disk
#[derive(Debug, Clone)]
pub struct DiskMetadata {
    pub id: u64,
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub created_at: i64,
    pub modified_at: i64,
}

impl DiskMetadata {
    pub fn new(
        id: u64,
        path: String,
        size: u64,
        is_directory: bool,
        created_at: i64,
        modified_at: i64,
    ) -> Self {
        Self {
            id,
            path,
            size,
            is_directory,
            created_at,
            modified_at,
        }
    }

    /// Serialize to bytes (exactly RECORD_SIZE bytes)
    fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut bytes = [0u8; RECORD_SIZE];

        // id: u64 (offset 0)
        bytes[0..8].copy_from_slice(&self.id.to_le_bytes());

        // size: u64 (offset 8)
        bytes[8..16].copy_from_slice(&self.size.to_le_bytes());

        // modified_at: i64 (offset 16)
        bytes[16..24].copy_from_slice(&self.modified_at.to_le_bytes());

        // created_at: i64 (offset 24)
        bytes[24..32].copy_from_slice(&self.created_at.to_le_bytes());

        // is_directory: u8 (offset 32)
        bytes[32] = if self.is_directory { 1 } else { 0 };

        // path_len: u16 (offset 33)
        let path_bytes = self.path.as_bytes();
        let path_len = path_bytes.len().min(MAX_PATH_LEN) as u16;
        bytes[33..35].copy_from_slice(&path_len.to_le_bytes());

        // path: variable (offset 35)
        let end = 35 + path_len as usize;
        bytes[35..end].copy_from_slice(&path_bytes[..path_len as usize]);

        bytes
    }

    /// Deserialize from bytes
    fn from_bytes(bytes: &[u8; RECORD_SIZE]) -> Result<Self> {
        let id = u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]);
        let size = u64::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]]);
        let modified_at = i64::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23]]);
        let created_at = i64::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31]]);
        let is_directory = bytes[32] == 1;
        let path_len = u16::from_le_bytes([bytes[33], bytes[34]]) as usize;

        let path = String::from_utf8_lossy(&bytes[35..35 + path_len]).to_string();

        Ok(DiskMetadata {
            id,
            path,
            size,
            is_directory,
            created_at,
            modified_at,
        })
    }
}

/// Metadata store for disk-based storage
pub struct MetadataStore {
    base_path: PathBuf,
    /// In-memory index for id lookup
    index: Vec<(u64, u64)>, // (id, offset)
    /// Current file size (number of records)
    count: usize,
}

impl MetadataStore {
    pub fn new(base_path: &PathBuf) -> Self {
        Self {
            base_path: base_path.clone(),
            index: Vec::new(),
            count: 0,
        }
    }

    fn store_path(&self) -> PathBuf {
        self.base_path.join("path_store.bin")
    }

    fn index_path(&self) -> PathBuf {
        self.base_path.join("path_index.bin")
    }

    /// Open the store, loading the index.
    pub fn open(&mut self) -> Result<()> {
        let store_path = self.store_path();
        let index_path = self.index_path();

        if !store_path.exists() {
            info!("Metadata store does not exist, will be created on first write");
            return Ok(());
        }

        info!("Opening metadata store from {:?}", store_path);

        // Get record count
        let file = File::open(&store_path)?;
        let file_size = file.metadata()?.len();
        self.count = (file_size / RECORD_SIZE as u64) as usize;

        // Load or rebuild index
        if index_path.exists() {
            self.load_index(&index_path)?;
        } else {
            self.rebuild_index(&store_path)?;
        }

        info!("Metadata store opened with {} entries", self.count);
        Ok(())
    }

    fn load_index(&mut self, index_path: &PathBuf) -> Result<()> {
        let file = File::open(index_path)?;
        let mut reader = BufReader::new(file);

        let mut count_buf = [0u8; 8];
        reader.read_exact(&mut count_buf)?;
        let count = u64::from_le_bytes(count_buf) as usize;

        self.index.reserve(count);
        for _ in 0..count {
            let mut entry_buf = [0u8; 16]; // id(8) + offset(8)
            reader.read_exact(&mut entry_buf)?;
            let id = u64::from_le_bytes([entry_buf[0], entry_buf[1], entry_buf[2], entry_buf[3], entry_buf[4], entry_buf[5], entry_buf[6], entry_buf[7]]);
            let offset = u64::from_le_bytes([entry_buf[8], entry_buf[9], entry_buf[10], entry_buf[11], entry_buf[12], entry_buf[13], entry_buf[14], entry_buf[15]]);
            self.index.push((id, offset));
        }

        Ok(())
    }

    fn rebuild_index(&mut self, store_path: &PathBuf) -> Result<()> {
        let file = File::open(store_path)?;
        let file_size = file.metadata()?.len();
        let record_count = (file_size / RECORD_SIZE as u64) as usize;

        self.index.reserve(record_count);
        for i in 0..record_count {
            let offset = (i * RECORD_SIZE) as u64;

            // Read first 8 bytes to get id
            let mut id_buf = [0u8; 8];
            let mut reader = BufReader::new(File::open(store_path)?);
            reader.seek(SeekFrom::Start(offset))?;
            reader.read_exact(&mut id_buf)?;
            let id = u64::from_le_bytes(id_buf);

            self.index.push((id, offset));
        }

        // Sort by id for binary search
        self.index.sort_by_key(|(id, _)| *id);

        // Save index
        self.save_index()?;

        Ok(())
    }

    fn save_index(&self) -> Result<()> {
        let index_path = self.index_path();
        let file = File::create(&index_path)?;
        let mut writer = BufWriter::new(file);

        writer.write_all(&(self.index.len() as u64).to_le_bytes())?;

        for (id, offset) in &self.index {
            writer.write_all(&id.to_le_bytes())?;
            writer.write_all(&offset.to_le_bytes())?;
        }

        writer.flush()?;
        Ok(())
    }

    /// Get metadata by ID (O(log n) lookup via binary search)
    pub fn get(&self, id: u64) -> Result<Option<DiskMetadata>> {
        // Binary search in index
        match self.index.binary_search_by_key(&id, |(id, _)| *id) {
            Ok(idx) => {
                let (_, offset) = self.index[idx];
                self.read_record(offset)
            }
            Err(_) => Ok(None),
        }
    }

    fn read_record(&self, offset: u64) -> Result<Option<DiskMetadata>> {
        let store_path = self.store_path();
        let file = File::open(&store_path)?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;

        let mut bytes = [0u8; RECORD_SIZE];
        reader.read_exact(&mut bytes)?;

        Ok(Some(DiskMetadata::from_bytes(&bytes)?))
    }

    /// Insert metadata.
    pub fn insert(&mut self, metadata: &DiskMetadata) -> Result<()> {
        let store_path = self.store_path();

        // Open file in append mode
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&store_path)?;

        let mut writer = BufWriter::new(file);

        // Calculate offset (end of file)
        let offset = writer.get_ref().metadata()?.len();

        // Write record
        writer.write_all(&metadata.to_bytes())?;
        writer.flush()?;

        // Update index
        let pos = self.index.len();
        if let Err(idx) = self.index.binary_search_by_key(&metadata.id, |(id, _)| *id) {
            self.index.insert(idx, (metadata.id, offset));
        } else {
            // Already exists - shouldn't happen in normal use
            self.index[pos] = (metadata.id, offset);
        }

        self.count += 1;

        // Save index every 1000 entries
        if self.count % 1000 == 0 {
            self.save_index()?;
        }

        Ok(())
    }

    /// Insert multiple records in batch
    pub fn insert_batch(&mut self, records: &[DiskMetadata]) -> Result<()> {
        let store_path = self.store_path();

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&store_path)?;

        let mut writer = BufWriter::new(file);

        let mut offset = writer.get_ref().metadata()?.len();

        for metadata in records {
            writer.write_all(&metadata.to_bytes())?;

            if let Err(idx) = self.index.binary_search_by_key(&metadata.id, |(id, _)| *id) {
                self.index.insert(idx, (metadata.id, offset));
            }

            offset += RECORD_SIZE as u64;
            self.count += 1;
        }

        writer.flush()?;
        self.save_index()?;

        Ok(())
    }

    /// Get count of entries
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterate over all entries
    pub fn iter_all(&self) -> Result<Box<dyn Iterator<Item = Result<DiskMetadata>>>> {
        let store_path = self.store_path();
        let file = File::open(&store_path)?;
        let file_size = file.metadata()?.len();

        Ok(Box::new(MetadataIterator {
            file,
            current_offset: 0,
            total_size: file_size,
        }))
    }

    /// Clear all data
    pub fn clear(&mut self) -> Result<()> {
        let store_path = self.store_path();
        let index_path = self.index_path();

        if store_path.exists() {
            std::fs::remove_file(&store_path)?;
        }
        if index_path.exists() {
            std::fs::remove_file(&index_path)?;
        }

        self.index.clear();
        self.count = 0;

        Ok(())
    }
}

/// Iterator over all metadata records
struct MetadataIterator {
    file: File,
    current_offset: u64,
    total_size: u64,
}

impl Iterator for MetadataIterator {
    type Item = Result<DiskMetadata>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_offset >= self.total_size {
            return None;
        }

        let mut reader = BufReader::new(&self.file);
        if reader.seek(SeekFrom::Start(self.current_offset)).is_err() {
            return None;
        }

        let mut bytes = [0u8; RECORD_SIZE];
        match reader.read_exact(&mut bytes) {
            Ok(()) => {
                self.current_offset += RECORD_SIZE as u64;
                Some(Ok(DiskMetadata::from_bytes(&bytes).unwrap()))
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_metadata_serialization() {
        let metadata = DiskMetadata::new(
            123,
            "/home/user/test.txt".to_string(),
            1024,
            false,
            1000000,
            2000000,
        );

        let bytes = metadata.to_bytes();
        let restored = DiskMetadata::from_bytes(&bytes).unwrap();

        assert_eq!(metadata.id, restored.id);
        assert_eq!(metadata.path, restored.path);
        assert_eq!(metadata.size, restored.size);
        assert_eq!(metadata.is_directory, restored.is_directory);
    }

    #[test]
    fn test_insert_and_get() {
        let temp_dir = tempdir().unwrap();
        let mut store = MetadataStore::new(&temp_dir.path().to_path_buf());

        let metadata = DiskMetadata::new(
            1,
            "/test/file.txt".to_string(),
            2048,
            false,
            1000,
            2000,
        );

        store.insert(&metadata).unwrap();

        let retrieved = store.get(1).unwrap().unwrap();
        assert_eq!(retrieved.path, "/test/file.txt");
        assert_eq!(retrieved.size, 2048);
    }
}
