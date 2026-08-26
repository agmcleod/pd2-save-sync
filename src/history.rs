use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const HISTORY_DIR_NAME: &str = ".history";

#[derive(Debug, Clone)]
pub struct VersionEntry {
    pub file_name: String,
    pub version_path: PathBuf,
    pub created_at: DateTime<Local>,
    pub file_size: u64,
    pub hash: String,
}

/// Computes the SHA-256 hash of a file's content
pub fn compute_file_hash(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("Failed to read file for hashing: {:?}", path))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Returns the history folder for a given base remote directory
pub fn get_history_dir(remote_dir: &Path) -> PathBuf {
    remote_dir.join(HISTORY_DIR_NAME)
}

/// Returns the specific history folder for a given file name
pub fn get_file_history_dir(remote_dir: &Path, file_name: &str) -> PathBuf {
    get_history_dir(remote_dir).join(file_name)
}

/// Archives a copy of the specified file to the remote history directory,
/// and then prunes old versions beyond `max_versions`.
pub fn archive_version(
    remote_dir: &Path,
    source_file: &Path,
    max_versions: usize,
) -> Result<Option<PathBuf>> {
    if !source_file.exists() || !source_file.is_file() {
        return Ok(None);
    }

    let file_name = source_file
        .file_name()
        .context("Invalid file name")?
        .to_string_lossy()
        .to_string();

    let history_dir = get_file_history_dir(remote_dir, &file_name);
    fs::create_dir_all(&history_dir)
        .with_context(|| format!("Failed to create history dir: {:?}", history_dir))?;

    let file_hash = compute_file_hash(source_file)?;
    let short_hash = &file_hash[..8.min(file_hash.len())];

    // Check if the most recent version has the same content hash, to avoid duplicate archives
    let existing_versions = list_versions(remote_dir, &file_name)?;
    if let Some(latest) = existing_versions.last() {
        if latest.hash.starts_with(short_hash) {
            // Already archived this exact version
            return Ok(None);
        }
    }

    let now: DateTime<Local> = Local::now();
    let timestamp_str = now.format("%Y%m%d_%H%M%S_%6f").to_string();
    let archive_file_name = format!("{}_{}_{}", timestamp_str, short_hash, file_name);
    let target_path = history_dir.join(archive_file_name);

    fs::copy(source_file, &target_path)
        .with_context(|| format!("Failed to archive version to {:?}", target_path))?;

    // Prune versions if exceeding max_versions
    prune_versions(remote_dir, &file_name, max_versions)?;

    Ok(Some(target_path))
}

/// Prunes old versions in history for `file_name`, keeping only the newest `max_versions`.
pub fn prune_versions(remote_dir: &Path, file_name: &str, max_versions: usize) -> Result<usize> {
    if max_versions == 0 {
        return Ok(0);
    }

    let versions = list_versions(remote_dir, file_name)?;
    if versions.len() <= max_versions {
        return Ok(0);
    }

    let to_remove_count = versions.len() - max_versions;
    let mut removed = 0;

    for version in versions.iter().take(to_remove_count) {
        if let Err(e) = fs::remove_file(&version.version_path) {
            eprintln!(
                "Warning: Failed to remove old version {:?}: {}",
                version.version_path, e
            );
        } else {
            removed += 1;
        }
    }

    Ok(removed)
}

/// Lists all historical versions for a file, sorted chronologically from oldest to newest.
pub fn list_versions(remote_dir: &Path, file_name: &str) -> Result<Vec<VersionEntry>> {
    let history_dir = get_file_history_dir(remote_dir, file_name);
    if !history_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(&history_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let meta = entry.metadata()?;
            let modified: DateTime<Local> = meta.modified().unwrap_or(SystemTime::now()).into();

            let name = entry.file_name().to_string_lossy().to_string();
            // Extract hash if encoded in filename (e.g. YYYYMMDD_HHMMSS_ffffff_<hash>_<original_name>)
            let parts: Vec<&str> = name.splitn(4, '_').collect();
            let hash = if parts.len() >= 4 {
                parts[2].to_string()
            } else if parts.len() >= 3 {
                parts[1].to_string()
            } else {
                compute_file_hash(&path).unwrap_or_default()
            };

            entries.push(VersionEntry {
                file_name: name,
                version_path: path,
                created_at: modified,
                file_size: meta.len(),
                hash,
            });
        }
    }

    // Sort oldest first by chronological filename (starts with ISO-style YYYYMMDD_HHMMSS_ffffff)
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(entries)
}

/// Restores a specific version to the destination path
pub fn restore_version(version_file: &Path, destination_file: &Path) -> Result<()> {
    if !version_file.exists() {
        anyhow::bail!("Backup version file does not exist: {:?}", version_file);
    }

    if let Some(parent) = destination_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {:?}", parent))?;
    }

    fs::copy(version_file, destination_file).with_context(|| {
        format!(
            "Failed to restore {:?} to {:?}",
            version_file, destination_file
        )
    })?;

    Ok(())
}
