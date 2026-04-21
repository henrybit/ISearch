pub mod database;
pub mod indexer;
pub mod search;
pub mod watcher;

// Storage modules
pub mod lmdb_store;
pub mod fst_index;

// Sharded FST + Inverted Index
pub mod sharded_fst;
pub mod inverted_index;
pub mod delta_tracker;

// Cross-platform file indexer
pub mod fs_indexer;

// Native speed indexer
pub mod native_indexer;

// ClamAV virus scanner
pub mod clamav_scanner;
