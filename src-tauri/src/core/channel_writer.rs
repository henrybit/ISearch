//! Channel-based async batch writer using crossbeam-channel.
//! Replaces synchronous SQLite INSERT transactions.

use crossbeam_channel::{bounded, Sender, Receiver};
use std::sync::Arc;
use parking_lot::Mutex;
use anyhow::Result;
use tracing::{info, error, warn};
use std::thread;
use std::time::Duration;

use crate::core::lmdb_store::{LmdbStore, FileMetadata};

pub struct ChannelWriter {
    sender: Option<Sender<Vec<FileMetadata>>>,
    handle: Option<thread::JoinHandle<()>>,
    is_running: Arc<Mutex<bool>>,
}

impl ChannelWriter {
    pub fn new(store: Arc<LmdbStore>, channel_capacity: usize, batch_size: usize) -> Self {
        let (sender, receiver) = bounded(channel_capacity);
        let is_running = Arc::new(Mutex::new(true));
        let is_running_clone = is_running.clone();

        let handle = thread::spawn(move || {
            Self::writer_loop(receiver, store, batch_size, is_running_clone);
        });

        info!("Channel writer started with capacity {} and batch size {}", channel_capacity, batch_size);

        Self {
            sender: Some(sender),
            handle: Some(handle),
            is_running,
        }
    }

    fn writer_loop(
        receiver: Receiver<Vec<FileMetadata>>,
        store: Arc<LmdbStore>,
        batch_size: usize,
        is_running: Arc<Mutex<bool>>,
    ) {
        let mut pending: Vec<FileMetadata> = Vec::with_capacity(batch_size);
        let mut total_written = 0;
        let mut channel_closed = false;

        loop {
            // Try to receive with a timeout
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(batch) => {
                    pending.extend(batch);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Timeout - continue to flush if needed
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    // Channel closed, write remaining and exit
                    info!("Channel closed, flushing remaining items");
                    channel_closed = true;
                }
            }

            // Flush if batch is full or channel is closed
            if pending.len() >= batch_size || (pending.len() > 0 && channel_closed) {
                let batch_to_write: Vec<FileMetadata> = pending.drain(..).collect();

                if let Err(e) = store.insert_batch(&batch_to_write) {
                    error!("Failed to write batch: {}", e);
                    // Put items back on failure
                    pending.extend(batch_to_write);
                } else {
                    total_written += batch_to_write.len();
                    if total_written % 10000 == 0 {
                        info!("Channel writer: {} items written", total_written);
                    }
                }
            }

            // Check if writer should stop
            if channel_closed && pending.is_empty() {
                break;
            }

            // Also check the is_running flag to allow graceful shutdown
            if !*is_running.lock() && channel_closed && pending.is_empty() {
                break;
            }
        }

        info!("Channel writer loop ended, total written: {}", total_written);
    }

    pub fn write(&self, items: Vec<FileMetadata>) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        let len = items.len();
        if let Some(ref sender) = self.sender {
            sender.send(items)
                .map_err(|e| anyhow::anyhow!("Channel send failed: {}", e))?;
            info!("ChannelWriter: sent batch of {} items", len);
        } else {
            warn!("ChannelWriter: sender is None, dropping {} items", len);
        }

        Ok(())
    }

    pub fn shutdown(&mut self) {
        info!("Shutting down channel writer...");
        let mut running = self.is_running.lock();
        *running = false;
        self.sender = None;

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ChannelWriter {
    fn drop(&mut self) {
        self.shutdown();
    }
}
