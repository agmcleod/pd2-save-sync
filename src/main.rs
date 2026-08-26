use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::{Path, PathBuf};

use pd2_save_sync::config::{Config, CONFIG_FILE_NAME};
use pd2_save_sync::history::{get_history_dir, list_versions, restore_version};
use pd2_save_sync::sync::{execute_sync, plan_sync, print_status_table, scan_level_one_files, SyncMode};
use pd2_save_sync::watcher::watch_and_sync;

#[derive(Parser, Debug)]
#[command(name = "pd2-save-sync")]
#[command(about = "Synchronize Diablo II / Project Diablo 2 saves with OneDrive and manage version backups", version)]
struct Cli {
    /// Path to a custom config TOML file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Override local Diablo II save directory
    #[arg(long, global = true)]
    local_dir: Option<PathBuf>,

    /// Override remote OneDrive directory
    #[arg(long, global = true)]
    remote_dir: Option<PathBuf>,

    /// Override maximum versions retained in history
    #[arg(long, global = true)]
    max_versions: Option<usize>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Perform two-way sync between local Save and OneDrive (Default)
    Sync {
        /// Preview changes without modifying files on disk
        #[arg(long)]
        dry_run: bool,
    },
    /// Push local save files to OneDrive
    Push {
        /// Preview changes without modifying files on disk
        #[arg(long)]
        dry_run: bool,
    },
    /// Pull OneDrive save files to local folder (protects against overwriting newer local files)
    Pull {
        /// Preview changes without modifying files on disk
        #[arg(long)]
        dry_run: bool,
    },
    /// Check differences and sync status without modifying files
    Status,
    /// View backup version history for save files in OneDrive
    History {
        /// Specific file name to inspect (e.g. Sorceress.d2s). If omitted, lists history for all saves.
        file: Option<String>,
    },
    /// Restore a previous backup version of a save file
    Restore {
        /// Name of the save file to restore (e.g. Sorceress.d2s)
        file: String,
        /// Specific version number (1 = oldest, or highest number for newest)
        #[arg(short, long)]
        version: Option<usize>,
        /// Destination directory to restore file into (defaults to local save directory)
        #[arg(long)]
        dest: Option<PathBuf>,
    },
    /// Continuously watch the local save folder and sync automatically on changes
    Watch,
    /// Generate a default pd2-sync.toml configuration file
    Init {
        /// Force overwrite if file already exists
        #[arg(short, long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (mut config, loaded_path) = Config::load_or_default(cli.config.as_deref())?;

    // Apply CLI overrides if provided
    if let Some(local) = cli.local_dir {
        config.local_dir = local;
    }
    if let Some(remote) = cli.remote_dir {
        config.remote_dir = remote;
    }
    if let Some(max_v) = cli.max_versions {
        config.max_versions = max_v;
    }

    match cli.command {
        None => {
            run_sync(&config, SyncMode::TwoWay, false, loaded_path.as_deref())?;
        }
        Some(Commands::Sync { dry_run }) => {
            run_sync(&config, SyncMode::TwoWay, dry_run, loaded_path.as_deref())?;
        }
        Some(Commands::Push { dry_run }) => {
            run_sync(&config, SyncMode::PushOnly, dry_run, loaded_path.as_deref())?;
        }
        Some(Commands::Pull { dry_run }) => {
            run_sync(&config, SyncMode::PullOnly, dry_run, loaded_path.as_deref())?;
        }
        Some(Commands::Status) => {
            run_status(&config, loaded_path.as_deref())?;
        }
        Some(Commands::History { file }) => {
            run_history(&config, file.as_deref())?;
        }
        Some(Commands::Restore {
            file,
            version,
            dest,
        }) => {
            run_restore(&config, &file, version, dest)?;
        }
        Some(Commands::Watch) => {
            println!("{} Starting watch mode...", "[INFO]".cyan().bold());
            println!("Local Save Dir:   {}", config.local_dir.display());
            println!("OneDrive Save Dir: {}", config.remote_dir.display());
            watch_and_sync(&config)?;
        }
        Some(Commands::Init { force }) => {
            run_init(&config, force)?;
        }
    }

    Ok(())
}

fn run_sync(
    config: &Config,
    mode: SyncMode,
    dry_run: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    println!("{}", "=== Project Diablo 2 Save Sync ===".bold().cyan());
    if let Some(p) = config_path {
        println!("Config:       {}", p.display());
    }
    println!("Local Dir:    {}", config.local_dir.display());
    println!("OneDrive Dir: {}", config.remote_dir.display());
    println!("Max Versions: {}", config.max_versions);
    println!("Mode:         {:?}", mode);
    if dry_run {
        println!("{}", "[DRY RUN MODE - No files will be modified]".yellow().bold());
    }
    println!();

    let items = plan_sync(config, mode)?;
    if items.is_empty() {
        println!("No save files found in either directory.");
        return Ok(());
    }

    let summary = execute_sync(&items, config, dry_run)?;

    println!("\n{}", "--- Sync Summary ---".bold());
    println!("Pushed to OneDrive: {}", summary.pushed.to_string().green());
    println!("Pulled to Local:    {}", summary.pulled.to_string().blue());
    println!("Skipped (Up-to-date): {}", summary.skipped);
    if summary.conflicts > 0 {
        println!("Conflicts detected:  {}", summary.conflicts.to_string().yellow().bold());
    }
    if summary.errors > 0 {
        println!("Errors encountered:  {}", summary.errors.to_string().red().bold());
    }

    Ok(())
}

fn run_status(config: &Config, config_path: Option<&Path>) -> Result<()> {
    println!("{}", "=== Project Diablo 2 Save Status ===".bold().cyan());
    if let Some(p) = config_path {
        println!("Config:       {}", p.display());
    }
    println!("Local Dir:    {}", config.local_dir.display());
    println!("OneDrive Dir: {}", config.remote_dir.display());

    let items = plan_sync(config, SyncMode::TwoWay)?;
    if items.is_empty() {
        println!("No save files found in either directory.");
        return Ok(());
    }

    print_status_table(&items);
    Ok(())
}

fn run_history(config: &Config, target_file: Option<&str>) -> Result<()> {
    println!("{}", "=== Save Version History ===".bold().cyan());
    println!("History Location: {}\n", get_history_dir(&config.remote_dir).display());

    if let Some(file_name) = target_file {
        let versions = list_versions(&config.remote_dir, file_name)?;
        if versions.is_empty() {
            println!("No history found for '{}'", file_name);
            return Ok(());
        }

        println!("Found {} version(s) for '{}':", versions.len(), file_name.bold());
        for (i, v) in versions.iter().enumerate() {
            println!(
                "  [{:>2}] {} | Size: {:>8} bytes | Hash: {}",
                i + 1,
                v.created_at.format("%Y-%m-%d %H:%M:%S"),
                v.file_size,
                v.hash
            );
        }
    } else {
        let remote_files = scan_level_one_files(&config.remote_dir, &config.ignored_files)?;
        let history_dir = get_history_dir(&config.remote_dir);

        let mut all_files = std::collections::BTreeSet::new();
        for f in &remote_files {
            if let Some(name) = f.file_name() {
                all_files.insert(name.to_string_lossy().to_string());
            }
        }
        if history_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&history_dir) {
                for e in entries.flatten() {
                    if e.path().is_dir() {
                        all_files.insert(e.file_name().to_string_lossy().to_string());
                    }
                }
            }
        }

        if all_files.is_empty() {
            println!("No saves or version histories found.");
            return Ok(());
        }

        for file_name in all_files {
            let versions = list_versions(&config.remote_dir, &file_name)?;
            println!("* {} ({} versions recorded)", file_name.bold(), versions.len());
            for (i, v) in versions.iter().enumerate() {
                println!(
                    "    [{:>2}] {} ({} bytes, hash: {})",
                    i + 1,
                    v.created_at.format("%Y-%m-%d %H:%M:%S"),
                    v.file_size,
                    v.hash
                );
            }
        }
    }

    Ok(())
}

fn run_restore(
    config: &Config,
    file_name: &str,
    version_idx: Option<usize>,
    dest: Option<PathBuf>,
) -> Result<()> {
    let versions = list_versions(&config.remote_dir, file_name)?;
    if versions.is_empty() {
        anyhow::bail!("No backup versions found for '{}'", file_name);
    }

    let selected_version = match version_idx {
        Some(idx) => {
            if idx == 0 || idx > versions.len() {
                anyhow::bail!(
                    "Invalid version index {}. Valid range is 1 to {}",
                    idx,
                    versions.len()
                );
            }
            &versions[idx - 1]
        }
        None => {
            println!("No version index specified; defaulting to latest backup.");
            versions.last().unwrap()
        }
    };

    let target_dir = dest.unwrap_or_else(|| config.local_dir.clone());
    let destination_file = target_dir.join(file_name);

    println!(
        "Restoring version [{}] from {} to {}...",
        selected_version.file_name,
        selected_version.created_at.format("%Y-%m-%d %H:%M:%S"),
        destination_file.display()
    );

    restore_version(&selected_version.version_path, &destination_file)?;
    println!("{}", "Restore completed successfully!".green().bold());

    Ok(())
}

fn run_init(config: &Config, force: bool) -> Result<()> {
    let target = Path::new(CONFIG_FILE_NAME);
    if target.exists() && !force {
        anyhow::bail!(
            "{} already exists. Use --force to overwrite.",
            CONFIG_FILE_NAME
        );
    }

    config.save_to_file(target)?;
    println!(
        "{} Created configuration file at {}",
        "[SUCCESS]".green().bold(),
        target.display()
    );
    Ok(())
}
