//! Delta tracker for tracking changes since last index.
//! Records added, removed, and modified files for incremental updates.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Tracks file changes since the last full index.
/// Used for incremental reindexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaTracker {
    /// Files added since last index
    pub added: Vec<String>,
    /// Files removed since last index
    pub removed: Vec<String>,
    /// Files modified since last index (path -> previous modified_time)
    pub modified: HashMap<String, i64>,
    /// Timestamp of last full index
    pub last_index_time: i64,
    /// Timestamp when delta was created
    pub delta_created_at: i64,
}

impl Default for DeltaTracker {
    fn default() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
            modified: HashMap::new(),
            last_index_time: 0,
            delta_created_at: Utc::now().timestamp(),
        }
    }
}

impl DeltaTracker {
    /// Load delta tracker from a file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            info!("No delta file found, starting fresh");
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)
            .context("Failed to read delta file")?;

        let tracker: DeltaTracker = serde_json::from_str(&content)
            .context("Failed to parse delta file JSON")?;

        info!("Loaded delta tracker: {} added, {} removed, {} modified",
              tracker.added.len(), tracker.removed.len(), tracker.modified.len());

        Ok(tracker)
    }

    /// Save delta tracker to a file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize delta tracker")?;

        std::fs::write(path, json)
            .context("Failed to write delta file")?;

        info!("Delta tracker saved to {:?}", path);
        Ok(())
    }

    /// Record a file as added.
    pub fn record_add(&mut self, path: String) {
        // Don't track if already in added list
        if !self.added.contains(&path) {
            // Remove from removed if it was there (moved file)
            self.removed.retain(|p| p != &path);
            self.added.push(path);
        }
    }

    /// Record a file as removed.
    pub fn record_remove(&mut self, path: String) {
        // Don't track if already in removed list
        if !self.removed.contains(&path) {
            // Remove from added if it was there (added and then removed before indexing)
            self.added.retain(|p| p != &path);
            self.removed.push(path);
        }
    }

    /// Record a file as modified.
    pub fn record_modify(&mut self, path: String, previous_modified_time: i64) {
        // Remove from added if it was there (new file that was modified)
        self.added.retain(|p| p != &path);
        self.modified.insert(path, previous_modified_time);
    }

    /// Mark a full index as complete and clear the delta.
    pub fn mark_indexed(&mut self) {
        self.last_index_time = Utc::now().timestamp();
        self.added.clear();
        self.removed.clear();
        self.modified.clear();
        self.delta_created_at = self.last_index_time;
    }

    /// Get the number of pending changes.
    pub fn pending_changes(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }

    /// Check if there are any pending changes.
    pub fn has_changes(&self) -> bool {
        self.pending_changes() > 0
    }

    /// Get a summary of changes for logging.
    pub fn summary(&self) -> String {
        format!("+{} -{} ~{}",
                self.added.len(),
                self.removed.len(),
                self.modified.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_record_add() {
        let mut tracker = DeltaTracker::default();
        tracker.record_add("file1.txt".to_string());
        tracker.record_add("file2.txt".to_string());

        assert_eq!(tracker.added.len(), 2);
        assert!(tracker.has_changes());
    }

    #[test]
    fn test_record_remove() {
        let mut tracker = DeltaTracker::default();
        tracker.record_remove("file1.txt".to_string());

        assert_eq!(tracker.removed.len(), 1);
        assert!(tracker.has_changes());
    }

    #[test]
    fn test_save_load() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("delta.json");

        let mut tracker = DeltaTracker::default();
        tracker.record_add("file1.txt".to_string());

        tracker.save(&path).unwrap();

        let loaded = DeltaTracker::load(&path).unwrap();
        assert_eq!(loaded.added.len(), 1);
    }

    #[test]
    fn test_add_then_remove_cancels_out() {
        let mut tracker = DeltaTracker::default();
        tracker.record_add("file1.txt".to_string());
        tracker.record_remove("file1.txt".to_string());

        // Should have nothing since add then remove cancels
        assert!(!tracker.has_changes());
    }
}
