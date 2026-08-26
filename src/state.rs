use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const STATE_FILE_NAME: &str = ".pd2-sync-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileState {
    pub file_name: String,
    pub last_sync_time: DateTime<Local>,
    pub synced_hash: String,
    pub synced_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    pub files: HashMap<String, FileState>,
}

impl SyncState {
    /// Loads the sync state from either the local directory or the AppData configuration directory
    pub fn load(local_dir: &Path) -> Self {
        let state_path = get_state_file_path(local_dir);
        if state_path.exists() {
            if let Ok(content) = fs::read_to_string(&state_path) {
                if let Ok(state) = serde_json::from_str::<SyncState>(&content) {
                    return state;
                }
            }
        }
        SyncState::default()
    }

    /// Saves the sync state to disk
    pub fn save(&self, local_dir: &Path) -> Result<()> {
        let state_path = get_state_file_path(local_dir);
        if let Some(parent) = state_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let serialized =
            serde_json::to_string_pretty(self).context("Failed to serialize sync state to JSON")?;
        fs::write(&state_path, serialized)
            .with_context(|| format!("Failed to write sync state file to {:?}", state_path))?;
        Ok(())
    }

    /// Updates or inserts a record for a synced file
    pub fn record_file(&mut self, file_name: &str, hash: &str, size: u64) {
        self.files.insert(
            file_name.to_string(),
            FileState {
                file_name: file_name.to_string(),
                last_sync_time: Local::now(),
                synced_hash: hash.to_string(),
                synced_size: size,
            },
        );
    }

    /// Gets the recorded state for a file, if available
    pub fn get_file(&self, file_name: &str) -> Option<&FileState> {
        self.files.get(file_name)
    }

    /// Removes a file from tracking
    pub fn remove_file(&mut self, file_name: &str) {
        self.files.remove(file_name);
    }
}

/// Computes the path to the state file (prefers local save dir, fallbacks to AppData if not writable)
pub fn get_state_file_path(local_dir: &Path) -> PathBuf {
    local_dir.join(STATE_FILE_NAME)
}
