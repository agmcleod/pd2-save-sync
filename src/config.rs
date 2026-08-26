use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_VERSIONS: usize = 10;
pub const CONFIG_FILE_NAME: &str = "pd2-sync.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Local Diablo II / Project Diablo 2 Save directory (e.g., C:\Program Files (x86)\Diablo II\Save)
    pub local_dir: PathBuf,
    /// OneDrive synchronization folder (e.g., C:\Users\<user>\OneDrive\pd2save)
    pub remote_dir: PathBuf,
    /// Number of historic versions to keep in OneDrive (e.g., 7-10)
    #[serde(default = "default_max_versions")]
    pub max_versions: usize,
    /// List of file names or patterns to ignore
    #[serde(default = "default_ignored_files")]
    pub ignored_files: Vec<String>,
}

fn default_max_versions() -> usize {
    DEFAULT_MAX_VERSIONS
}

fn default_ignored_files() -> Vec<String> {
    vec![
        "desktop.ini".to_string(),
        "thumbs.db".to_string(),
        ".DS_Store".to_string(),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            local_dir: detect_default_local_dir(),
            remote_dir: detect_default_remote_dir(),
            max_versions: DEFAULT_MAX_VERSIONS,
            ignored_files: default_ignored_files(),
        }
    }
}

impl Config {
    /// Load config from an explicit path, or look in the current directory / user home directory,
    /// falling back to defaults if not found.
    pub fn load_or_default(custom_path: Option<&Path>) -> Result<(Self, Option<PathBuf>)> {
        if let Some(path) = custom_path {
            if path.exists() {
                let content = std::fs::read_to_string(path)
                    .with_context(|| format!("Failed to read config file at {:?}", path))?;
                let config: Config = toml::from_str(&content)
                    .with_context(|| format!("Failed to parse config file at {:?}", path))?;
                return Ok((config, Some(path.to_path_buf())));
            } else {
                anyhow::bail!("Specified config file does not exist: {:?}", path);
            }
        }

        // Search candidate paths
        let candidates = [
            PathBuf::from(CONFIG_FILE_NAME),
            dirs::config_dir()
                .map(|p| p.join("pd2-save-sync").join(CONFIG_FILE_NAME))
                .unwrap_or_default(),
            dirs::home_dir()
                .map(|p| p.join(".pd2-sync.toml"))
                .unwrap_or_default(),
        ];

        for candidate in &candidates {
            if candidate.as_os_str().is_empty() {
                continue;
            }
            if candidate.exists() {
                let content = std::fs::read_to_string(candidate)
                    .with_context(|| format!("Failed to read config file at {:?}", candidate))?;
                let config: Config = toml::from_str(&content)
                    .with_context(|| format!("Failed to parse config file at {:?}", candidate))?;
                return Ok((config, Some(candidate.clone())));
            }
        }

        Ok((Config::default(), None))
    }

    /// Save current config to a TOML file
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directories for {:?}", path))?;
        }
        let serialized =
            toml::to_string_pretty(self).context("Failed to serialize config to TOML format")?;
        std::fs::write(path, serialized)
            .with_context(|| format!("Failed to write config file to {:?}", path))?;
        Ok(())
    }
}

/// Detects the default local Diablo II Save directory
pub fn detect_default_local_dir() -> PathBuf {
    let standard_x86 = PathBuf::from(r"C:\Program Files (x86)\Diablo II\Save");
    if standard_x86.exists() {
        return standard_x86;
    }

    let standard_c = PathBuf::from(r"C:\Diablo II\Save");
    if standard_c.exists() {
        return standard_c;
    }

    // Default to the standard x86 path even if it doesn't exist yet on other machines
    standard_x86
}

/// Detects the default OneDrive synchronization folder
pub fn detect_default_remote_dir() -> PathBuf {
    // 1. Check OneDrive environment variables on Windows
    for env_var in &["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(val) = std::env::var(env_var) {
            let p = PathBuf::from(val).join("pd2save");
            return p;
        }
    }

    // 2. Check user home folder / OneDrive
    if let Some(home) = dirs::home_dir() {
        let p = home.join("OneDrive").join("pd2save");
        return p;
    }

    PathBuf::from(r"C:\Users\aaron\OneDrive\pd2save")
}
