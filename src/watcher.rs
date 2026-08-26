use anyhow::{Context, Result};
use colored::Colorize;
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::sync::{SyncMode, execute_sync, plan_sync};

/// Starts watching the local directory for save changes and triggers sync with debouncing
pub fn watch_and_sync(config: &Config) -> Result<()> {
    if !config.local_dir.exists() {
        std::fs::create_dir_all(&config.local_dir)
            .with_context(|| format!("Failed to create local directory {:?}", config.local_dir))?;
    }

    println!(
        "{} Watching {} for Diablo II save changes...",
        "[WATCH]".cyan().bold(),
        config.local_dir.display().to_string().yellow()
    );
    println!("Press Ctrl+C to stop.\n");

    let (tx, rx) = channel();

    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        NotifyConfig::default(),
    )?;

    // Only watch non-recursively (level-1)
    watcher.watch(&config.local_dir, RecursiveMode::NonRecursive)?;

    let debounce_duration = Duration::from_millis(1500);
    let mut last_sync = Instant::now();

    loop {
        match rx.recv() {
            Ok(event) => {
                // Check if any path in event is a file in the local root (not a subdirectory)
                let relevant = event.paths.iter().any(|p| {
                    p.parent() == Some(&config.local_dir)
                        && p.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .map(|n| {
                                !config
                                    .ignored_files
                                    .iter()
                                    .any(|ig| ig.eq_ignore_ascii_case(&n))
                            })
                            .unwrap_or(false)
                });

                if !relevant {
                    continue;
                }

                // Debounce to allow multi-file write bursts from game exit/save
                std::thread::sleep(Duration::from_millis(300));
                // Drain any additional events queued during sleep
                while rx.try_recv().is_ok() {}

                if last_sync.elapsed() > debounce_duration {
                    println!(
                        "\n{} Change detected! Running automatic sync...",
                        "[SYNC]".green().bold()
                    );
                    match plan_sync(config, SyncMode::TwoWay) {
                        Ok(items) => {
                            if let Err(e) = execute_sync(&items, config, false) {
                                eprintln!("{} Sync error: {}", "[ERROR]".red().bold(), e);
                            }
                        }
                        Err(e) => {
                            eprintln!("{} Failed to plan sync: {}", "[ERROR]".red().bold(), e);
                        }
                    }
                    last_sync = Instant::now();
                }
            }
            Err(e) => {
                eprintln!("Watch channel error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
