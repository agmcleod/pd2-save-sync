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
        // 1. Try local directory
        let local_state_path = local_dir.join(STATE_FILE_NAME);
        if local_state_path.exists() {
            if let Ok(content) = fs::read_to_string(&local_state_path) {
                if let Ok(state) = serde_json::from_str::<SyncState>(&content) {
                    return state;
                }
            }
        }

        // 2. Try AppData config directory
        if let Some(appdata_state) = get_appdata_state_path() {
            if appdata_state.exists() {
                if let Ok(content) = fs::read_to_string(&appdata_state) {
                    if let Ok(state) = serde_json::from_str::<SyncState>(&content) {
                        return state;
                    }
                }
            }
        }

        SyncState::default()
    }

    /// Saves the sync state to disk (tries local directory first, falls back to AppData if permission denied)
    pub fn save(&self, local_dir: &Path) -> Result<()> {
        let serialized = serde_json::to_string_pretty(self)
            .context("Failed to serialize sync state to JSON")?;

        let local_state_path = local_dir.join(STATE_FILE_NAME);
        let save_local_result = fs::write(&local_state_path, &serialized);

        if save_local_result.is_ok() {
            return Ok(());
        }

        // Fallback to AppData if local save directory is write-protected (e.g. Program Files)
        if let Some(appdata_path) = get_appdata_state_path() {
            if let Some(parent) = appdata_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&appdata_path, &serialized).with_context(|| {
                format!("Failed to write sync state to fallback {:?}", appdata_path)
            })?;
            return Ok(());
        }

        save_local_result.map(|_| ()).map_err(Into::into)
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

/// Computes fallback path in AppData/pd2-save-sync/state.json
fn get_appdata_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("pd2-save-sync").join("sync-state.json"))
}
