use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use colored::Colorize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::history::{archive_version, compute_file_hash, list_versions, HISTORY_DIR_NAME};
use crate::state::{FileState, SyncState, STATE_FILE_NAME};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    UpToDate,
    PushToRemote,
    PullToLocal,
    Conflict,
    Ignored,
}

#[derive(Debug, Clone)]
pub struct SyncItem {
    pub file_name: String,
    pub action: SyncAction,
    pub local_path: PathBuf,
    pub remote_path: PathBuf,
    pub local_mtime: Option<DateTime<Local>>,
    pub remote_mtime: Option<DateTime<Local>>,
    pub local_size: Option<u64>,
    pub remote_size: Option<u64>,
    pub local_hash: Option<String>,
    pub remote_hash: Option<String>,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct SyncSummary {
    pub pushed: usize,
    pub pulled: usize,
    pub skipped: usize,
    pub conflicts: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    TwoWay,
    PushOnly,
    PullOnly,
}

/// Scans only level-1 files from a directory (subfolders and ignored files are excluded)
pub fn scan_level_one_files(dir: &Path, ignored: &[String]) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read directory {:?}", dir))? {
        let entry = entry?;
        let path = entry.path();

        // Strict level-one check: ignore subdirectories
        if path.is_file() {
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Ignore internal history folder, state tracking file, and user-configured ignored files
            if file_name == HISTORY_DIR_NAME
                || file_name == STATE_FILE_NAME
                || ignored.iter().any(|ig| ig.eq_ignore_ascii_case(&file_name))
            {
                continue;
            }
            files.push(path);
        }
    }
    Ok(files)
}

/// Analyzes local and remote folders using 3-way state comparison and builds the sync plan
pub fn plan_sync(config: &Config, mode: SyncMode) -> Result<Vec<SyncItem>> {
    let local_files = scan_level_one_files(&config.local_dir, &config.ignored_files)?;
    let remote_files = scan_level_one_files(&config.remote_dir, &config.ignored_files)?;

    let state = SyncState::load(&config.local_dir);

    let mut all_file_names = BTreeSet::new();

    for f in &local_files {
        if let Some(name) = f.file_name() {
            all_file_names.insert(name.to_string_lossy().to_string());
        }
    }

    for f in &remote_files {
        if let Some(name) = f.file_name() {
            all_file_names.insert(name.to_string_lossy().to_string());
        }
    }

    let mut items = Vec::new();

    for file_name in all_file_names {
        let local_path = config.local_dir.join(&file_name);
        let remote_path = config.remote_dir.join(&file_name);

        let local_meta = fs::metadata(&local_path).ok();
        let remote_meta = fs::metadata(&remote_path).ok();

        let local_mtime: Option<DateTime<Local>> = local_meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(Into::into);

        let remote_mtime: Option<DateTime<Local>> = remote_meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(Into::into);

        let local_size = local_meta.as_ref().map(|m| m.len());
        let remote_size = remote_meta.as_ref().map(|m| m.len());

        let local_hash = if local_path.is_file() {
            compute_file_hash(&local_path).ok()
        } else {
            None
        };

        let remote_hash = if remote_path.is_file() {
            compute_file_hash(&remote_path).ok()
        } else {
            None
        };

        let file_state = state.get_file(&file_name);

        let (action, reason) = determine_action(
            &local_path,
            &remote_path,
            &file_name,
            &config.remote_dir,
            local_hash.as_deref(),
            remote_hash.as_deref(),
            local_mtime,
            remote_mtime,
            file_state,
            mode,
        );

        items.push(SyncItem {
            file_name,
            action,
            local_path,
            remote_path,
            local_mtime,
            remote_mtime,
            local_size,
            remote_size,
            local_hash,
            remote_hash,
            reason,
        });
    }

    Ok(items)
}

fn determine_action(
    local_path: &Path,
    remote_path: &Path,
    file_name: &str,
    remote_dir: &Path,
    local_hash: Option<&str>,
    remote_hash: Option<&str>,
    local_mtime: Option<DateTime<Local>>,
    remote_mtime: Option<DateTime<Local>>,
    file_state: Option<&FileState>,
    mode: SyncMode,
) -> (SyncAction, String) {
    let local_exists = local_path.is_file();
    let remote_exists = remote_path.is_file();

    match (local_exists, remote_exists) {
        (true, false) => match mode {
            SyncMode::PullOnly => (
                SyncAction::Ignored,
                "Local file exists but Pull-Only mode requested".into(),
            ),
            _ => (
                SyncAction::PushToRemote,
                "New local save; uploading to OneDrive".into(),
            ),
        },
        (false, true) => match mode {
            SyncMode::PushOnly => (
                SyncAction::Ignored,
                "Remote file exists but Push-Only mode requested".into(),
            ),
            _ => (
                SyncAction::PullToLocal,
                "New remote save; downloading from OneDrive".into(),
            ),
        },
        (true, true) => {
            // 1. Content Hash Check: If content is already identical, nothing to do
            if let (Some(lh), Some(rh)) = (local_hash, remote_hash) {
                if lh == rh {
                    return (SyncAction::UpToDate, "Identical content hash".into());
                }
            }

            let l_time = local_mtime.unwrap_or_else(Local::now);
            let r_time = remote_mtime.unwrap_or_else(Local::now);

            match mode {
                SyncMode::PushOnly => (
                    SyncAction::PushToRemote,
                    format!(
                        "Push requested (Local: {}, Remote: {})",
                        l_time.format("%Y-%m-%d %H:%M:%S"),
                        r_time.format("%Y-%m-%d %H:%M:%S")
                    ),
                ),
                SyncMode::PullOnly => {
                    // Conflict guard in pull mode: if local timestamp is newer, do not overwrite!
                    if l_time > r_time {
                        (
                            SyncAction::Conflict,
                            format!(
                                "Conflict guard: Local timestamp ({}) is newer than Remote ({}); will not overwrite destination",
                                l_time.format("%Y-%m-%d %H:%M:%S"),
                                r_time.format("%Y-%m-%d %H:%M:%S")
                            ),
                        )
                    } else {
                        (
                            SyncAction::PullToLocal,
                            format!(
                                "Pull requested (Remote: {}, Local: {})",
                                r_time.format("%Y-%m-%d %H:%M:%S"),
                                l_time.format("%Y-%m-%d %H:%M:%S")
                            ),
                        )
                    }
                }
                SyncMode::TwoWay => {
                    // 2. 3-Way State Comparison (if previously synced state is recorded)
                    if let (Some(state), Some(lh), Some(rh)) = (file_state, local_hash, remote_hash) {
                        let local_changed = lh != state.synced_hash;
                        let remote_changed = rh != state.synced_hash;

                        if local_changed && !remote_changed {
                            return (
                                SyncAction::PushToRemote,
                                "Local save modified since last sync; safe push to OneDrive".into(),
                            );
                        } else if !local_changed && remote_changed {
                            return (
                                SyncAction::PullToLocal,
                                "Remote save updated from another PC; safe pull to local".into(),
                            );
                        } else if local_changed && remote_changed {
                            return (
                                SyncAction::Conflict,
                                format!(
                                    "True Conflict: Both local ({}) and OneDrive ({}) were modified independently since last sync",
                                    l_time.format("%Y-%m-%d %H:%M:%S"),
                                    r_time.format("%Y-%m-%d %H:%M:%S")
                                ),
                            );
                        }
                    }

                    // 3. History Check (if state file is missing on this machine, e.g. first run on laptop)
                    if let Some(lh) = local_hash {
                        if let Ok(history_versions) = list_versions(remote_dir, file_name) {
                            // If local hash matches an older snapshot in history, remote is newer
                            if history_versions.iter().any(|v| v.hash.starts_with(&lh[..8.min(lh.len())])) {
                                return (
                                    SyncAction::PullToLocal,
                                    "Local matches an earlier revision in history; pulling latest OneDrive save".into(),
                                );
                            }
                        }
                    }

                    // 4. Timestamp-based fallback
                    if l_time > r_time {
                        (
                            SyncAction::PushToRemote,
                            format!(
                                "Local is newer ({}) > Remote ({}); pushing to OneDrive",
                                l_time.format("%Y-%m-%d %H:%M:%S"),
                                r_time.format("%Y-%m-%d %H:%M:%S")
                            ),
                        )
                    } else if r_time > l_time {
                        (
                            SyncAction::PullToLocal,
                            format!(
                                "OneDrive is newer ({}) > Local ({}); pulling to local Save",
                                r_time.format("%Y-%m-%d %H:%M:%S"),
                                l_time.format("%Y-%m-%d %H:%M:%S")
                            ),
                        )
                    } else {
                        (
                            SyncAction::Conflict,
                            "Identical timestamp but different hash (conflict)".into(),
                        )
                    }
                }
            }
        }
        (false, false) => (SyncAction::Ignored, "Neither file exists".into()),
    }
}

/// Executes the sync plan with backup archiving, version rotation, and state tracking
pub fn execute_sync(items: &[SyncItem], config: &Config, dry_run: bool) -> Result<SyncSummary> {
    let mut summary = SyncSummary::default();
    let mut state = SyncState::load(&config.local_dir);

    // Ensure target directories exist if not in dry-run
    if !dry_run {
        if !config.local_dir.exists() {
            fs::create_dir_all(&config.local_dir).with_context(|| {
                format!("Failed to create local directory {:?}", config.local_dir)
            })?;
        }
        if !config.remote_dir.exists() {
            fs::create_dir_all(&config.remote_dir).with_context(|| {
                format!("Failed to create remote directory {:?}", config.remote_dir)
            })?;
        }
    }

    for item in items {
        match item.action {
            SyncAction::UpToDate => {
                summary.skipped += 1;
                if let (Some(h), Some(s)) = (&item.local_hash, item.local_size) {
                    state.record_file(&item.file_name, h, s);
                }
            }
            SyncAction::Ignored => {
                summary.skipped += 1;
            }
            SyncAction::Conflict => {
                summary.conflicts += 1;
                eprintln!(
                    "{} Conflict for {} -> {}",
                    "[CONFLICT]".yellow().bold(),
                    item.file_name.bold(),
                    item.reason
                );

                if !dry_run {
                    // 1. Archive existing local and remote files to history for safety
                    if item.remote_path.exists() {
                        let _ = archive_version(
                            &config.remote_dir,
                            &item.remote_path,
                            config.max_versions,
                        );
                    }
                    if item.local_path.exists() {
                        let _ = archive_version(
                            &config.remote_dir,
                            &item.local_path,
                            config.max_versions,
                        );
                    }

                    // 2. Save a copy of the conflicting remote file alongside local with a .conflict timestamp
                    let conflict_time = Local::now().format("%Y%m%d_%H%M%S");
                    let ext = item
                        .local_path
                        .extension()
                        .map(|e| e.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let stem = item
                        .local_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| item.file_name.clone());

                    let conflict_name = if ext.is_empty() {
                        format!("{}.conflict_{}", stem, conflict_time)
                    } else {
                        format!("{}.conflict_{}.{}", stem, conflict_time, ext)
                    };

                    let conflict_path = config.local_dir.join(&conflict_name);
                    if item.remote_path.exists() {
                        if let Ok(_) = fs::copy(&item.remote_path, &conflict_path) {
                            println!(
                                "{} Preserved conflicting remote copy at {}",
                                "[SAFETY]".yellow(),
                                conflict_name.cyan()
                            );
                        }
                    }
                }
            }
            SyncAction::PushToRemote => {
                if dry_run {
                    println!(
                        "{} [DRY-RUN] Push {} -> OneDrive ({})",
                        "[PUSH]".green().bold(),
                        item.file_name.cyan(),
                        item.reason
                    );
                    summary.pushed += 1;
                } else {
                    println!(
                        "{} Pushing {} -> OneDrive...",
                        "[PUSH]".green().bold(),
                        item.file_name.cyan()
                    );

                    // 1. Archive existing remote file (if any) to history
                    if item.remote_path.exists() {
                        if let Err(e) = archive_version(
                            &config.remote_dir,
                            &item.remote_path,
                            config.max_versions,
                        ) {
                            eprintln!("Warning: Failed to archive remote version: {}", e);
                        }
                    }

                    // 2. Copy local file to remote
                    if let Err(e) = fs::copy(&item.local_path, &item.remote_path) {
                        eprintln!(
                            "{} Failed to copy {:?} to {:?}: {}",
                            "[ERROR]".red().bold(),
                            item.local_path,
                            item.remote_path,
                            e
                        );
                        summary.errors += 1;
                    } else {
                        // 3. Archive the newly pushed version into history
                        let _ = archive_version(
                            &config.remote_dir,
                            &item.remote_path,
                            config.max_versions,
                        );

                        // 4. Update sync state
                        if let Ok(hash) = compute_file_hash(&item.local_path) {
                            let size = fs::metadata(&item.local_path).map(|m| m.len()).unwrap_or(0);
                            state.record_file(&item.file_name, &hash, size);
                        }
                        summary.pushed += 1;
                    }
                }
            }
            SyncAction::PullToLocal => {
                if dry_run {
                    println!(
                        "{} [DRY-RUN] Pull {} <- OneDrive ({})",
                        "[PULL]".blue().bold(),
                        item.file_name.cyan(),
                        item.reason
                    );
                    summary.pulled += 1;
                } else {
                    println!(
                        "{} Pulling {} <- OneDrive...",
                        "[PULL]".blue().bold(),
                        item.file_name.cyan()
                    );

                    // 1. If local exists, archive it in OneDrive history first to guarantee zero data loss
                    if item.local_path.exists() {
                        if let Err(e) = archive_version(
                            &config.remote_dir,
                            &item.local_path,
                            config.max_versions,
                        ) {
                            eprintln!("Warning: Failed to archive existing local version: {}", e);
                        }
                    }

                    // 2. Copy remote file to local
                    if let Err(e) = fs::copy(&item.remote_path, &item.local_path) {
                        eprintln!(
                            "{} Failed to copy {:?} to {:?}: {}",
                            "[ERROR]".red().bold(),
                            item.remote_path,
                            item.local_path,
                            e
                        );
                        summary.errors += 1;
                    } else {
                        // 3. Update sync state
                        if let Ok(hash) = compute_file_hash(&item.local_path) {
                            let size = fs::metadata(&item.local_path).map(|m| m.len()).unwrap_or(0);
                            state.record_file(&item.file_name, &hash, size);
                        }
                        summary.pulled += 1;
                    }
                }
            }
        }
    }

    if !dry_run {
        let _ = state.save(&config.local_dir);
    }

    Ok(summary)
}

/// Print status comparison table to console
pub fn print_status_table(items: &[SyncItem]) {
    println!(
        "\n{:<25} {:<15} {:<22} {:<22} {:<30}",
        "File", "Action", "Local Modified", "OneDrive Modified", "Reason"
    );
    println!("{:-<120}", "");

    for item in items {
        let action_str = match item.action {
            SyncAction::UpToDate => "Up to date".green(),
            SyncAction::PushToRemote => "Push (Upload)".bright_green().bold(),
            SyncAction::PullToLocal => "Pull (Download)".blue().bold(),
            SyncAction::Conflict => "Conflict".yellow().bold(),
            SyncAction::Ignored => "Ignored".dimmed(),
        };

        let local_time_str = item
            .local_mtime
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "---".to_string());

        let remote_time_str = item
            .remote_mtime
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "---".to_string());

        println!(
            "{:<25} {:<15} {:<22} {:<22} {:<30}",
            item.file_name, action_str, local_time_str, remote_time_str, item.reason
        );
    }
    println!("{:-<120}\n", "");
}
