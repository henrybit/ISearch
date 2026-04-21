//! File watcher using macOS FSEvents.
//!
//! Monitors file system changes and updates the database incrementally.
//! Changes are recorded in delta_tracker and processed in batches.

use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use anyhow::Result;
use tracing::{info, warn, error};

use crate::core::database::{self, FileData};
use crate::core::delta_tracker::DeltaTracker;

pub struct FileWatcher {
    watcher: Option<RecommendedWatcher>,
    is_watching: Arc<Mutex<bool>>,
    last_events: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    delta_tracker: Arc<Mutex<DeltaTracker>>,
}

impl FileWatcher {
    pub fn new() -> Self {
        let data_dir = database::get_data_dir();
        let tracker = DeltaTracker::load(&data_dir.join("delta.json"))
            .unwrap_or_default();

        Self {
            watcher: None,
            is_watching: Arc::new(Mutex::new(false)),
            last_events: Arc::new(Mutex::new(HashMap::new())),
            delta_tracker: Arc::new(Mutex::new(tracker)),
        }
    }

    pub fn is_watching(&self) -> bool {
        *self.is_watching.lock().unwrap()
    }

    fn should_process(&self, path: &Path) -> bool {
        let mut last_events = self.last_events.lock().unwrap();
        let now = Instant::now();
        const DEBOUNCE_DURATION: Duration = Duration::from_secs(2);

        if let Some(last_time) = last_events.get(path) {
            if now.duration_since(*last_time) < DEBOUNCE_DURATION {
                return false;
            }
        }

        last_events.insert(path.to_path_buf(), now);
        true
    }

    pub fn start_watching<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();

        if self.is_watching() {
            info!("Already watching, stopping first");
            self.stop_watching();
        }

        info!("Starting file watcher for {:?}", path);

        let is_watching = self.is_watching.clone();
        let last_events = self.last_events.clone();
        let delta_tracker = self.delta_tracker.clone();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        for path in event.paths {
                            if path.is_dir() {
                                continue;
                            }

                            if !Self::should_process_event(&last_events, &path) {
                                continue;
                            }

                            let mut delta = delta_tracker.lock().unwrap();
                            match event.kind {
                                EventKind::Create(_) | EventKind::Modify(_) => {
                                    if let Err(e) = Self::index_file(&path, &mut delta) {
                                        warn!("Failed to index file {:?}: {}", path, e);
                                    }
                                }
                                EventKind::Remove(_) => {
                                    let path_str = path.to_string_lossy().to_string();
                                    delta.record_remove(path_str);
                                    if let Err(e) = database::delete_file_by_path(&path.to_string_lossy()) {
                                        warn!("Failed to delete file from index {:?}: {}", path, e);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("Watch error: {:?}", e);
                    }
                }
            },
            Config::default(),
        )?;

        let mut watcher = watcher;
        watcher.watch(&path, RecursiveMode::Recursive)?;

        let mut is_watching = is_watching.lock().unwrap();
        *is_watching = true;
        drop(is_watching);

        self.watcher = Some(watcher);

        info!("File watcher started successfully");
        Ok(())
    }

    fn should_process_event(last_events: &Arc<Mutex<HashMap<PathBuf, Instant>>>, path: &Path) -> bool {
        let mut events = last_events.lock().unwrap();
        let now = Instant::now();
        const DEBOUNCE_DURATION: Duration = Duration::from_secs(2);

        if let Some(last_time) = events.get(path) {
            if now.duration_since(*last_time) < DEBOUNCE_DURATION {
                return false;
            }
        }

        events.insert(path.to_path_buf(), now);
        true
    }

    pub fn stop_watching(&mut self) {
        if let Some(watcher) = self.watcher.take() {
            // Watcher dropped here
        }

        let mut is_watching = self.is_watching.lock().unwrap();
        *is_watching = false;

        // Clear last events
        let mut events = self.last_events.lock().unwrap();
        events.clear();

        info!("File watcher stopped");
    }

    fn index_file(path: &Path, delta: &mut DeltaTracker) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(path)?;
        let is_dir = metadata.is_dir();

        let size = metadata.len() as u64;
        let created = metadata.created()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
            .unwrap_or(0);
        let modified = metadata.modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
            .unwrap_or(0);

        let path_str = path.to_string_lossy().to_string();

        // Record in delta tracker
        delta.record_add(path_str.clone());

        // Update LMDB directly
        let file_data = FileData {
            path: path_str,
            size,
            is_directory: is_dir,
            created_at: created,
            modified_at: modified,
        };

        database::insert_files_batch(&[file_data])?;

        Ok(())
    }

    /// Process pending changes and rebuild indexes
    pub fn process_pending_changes(&self) -> anyhow::Result<usize> {
        let mut delta = self.delta_tracker.lock().unwrap();
        let pending = delta.pending_changes();

        if pending > 0 {
            info!("Processing {} pending changes, rebuilding indexes...", pending);

            // Rebuild all indexes
            database::rebuild_all_indexes()?;

            // Save delta tracker
            let data_dir = database::get_data_dir();
            delta.save(&data_dir.join("delta.json"))?;
        }

        Ok(pending)
    }

    /// Get number of pending changes
    pub fn pending_changes(&self) -> usize {
        let delta = self.delta_tracker.lock().unwrap();
        delta.pending_changes()
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop_watching();
    }
}
