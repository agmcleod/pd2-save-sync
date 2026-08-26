# pd2-save-sync

A lightweight, conflict-safe save synchronization and version backup tool for **Diablo II / Project Diablo 2**, designed to keep your characters synchronized across multiple PCs via **OneDrive** without risking data loss.

---

## Features

- **Level-One Filtering**: Only synchronizes save files directly in the `Save/` folder (`*.d2s`, `*.key`, `*.map`, `*.stash`, etc.); subfolders and system files are ignored.
- **3-Way Conflict Safety**: Tracks the last known sync state per file. If both computers modified a save independently while offline, it flags a conflict, preserves your active local save, and writes a `.conflict_<timestamp>` copy so no progress is ever overwritten.
- **Automatic Version Retention (7–10 Versions)**: Archives timestamped snapshots in `OneDrive/pd2save/.history/` and automatically prunes excess backups beyond your configured limit (default: 10).
- **Rollback / Restore**: Inspect past save backups and restore any previous version with a single command.
- **Live Watcher**: Background file watcher with debounce that automatically syncs whenever Diablo II saves or exits.
- **Dry-Run Mode**: Preview what files will be copied before touching anything.

---

## Quick Start

### 1. Build & Install

```bash
cargo build --release
```

The compiled binary will be in `target/release/pd2-save-sync.exe`.

---

## Common Usage

### Check Differences (Safe / Read-Only)

See the status of all local vs. OneDrive saves without modifying anything:

```bash
pd2-save-sync status
```

### Synchronize (Default Two-Way Sync)

Performs smart 3-way sync with safety checks and backup rotation:

```bash
pd2-save-sync sync
```

To test without making changes:

```bash
pd2-save-sync sync --dry-run
```

### Continuous Background Sync (Watch Mode)

Run this while playing; it detects save changes and auto-syncs when you exit games:

```bash
pd2-save-sync watch
```

### View Version History

List all recorded historical versions for a character:

```bash
pd2-save-sync history Sorceress.d2s
```

Or view all saves:

```bash
pd2-save-sync history
```

### Restore a Previous Backup

Roll back a save file from OneDrive's history:

```bash
# Restore latest backup
pd2-save-sync restore Sorceress.d2s

# Restore a specific version index from the history list
pd2-save-sync restore Sorceress.d2s --version 2
```

---

## Configuration (`pd2-sync.toml`)

By default, `pd2-save-sync` automatically detects your Windows OneDrive folder and standard Diablo II paths.

To create a custom configuration file in your directory:

```bash
pd2-save-sync init
```

Example `pd2-sync.toml`:

```toml
local_dir = "C:\\Program Files (x86)\\Diablo II\\Save"
remote_dir = "C:\\Users\\aaron\\OneDrive\\pd2save"
max_versions = 10
ignored_files = ["desktop.ini", "thumbs.db", ".DS_Store"]
```

---

## CLI Command Reference

| Command                                | Description                                                                        |
| :------------------------------------- | :--------------------------------------------------------------------------------- |
| `pd2-save-sync` / `pd2-save-sync sync` | Two-way sync with 3-way conflict checks & version history                          |
| `pd2-save-sync sync --dry-run`         | Preview sync actions without writing to disk                                       |
| `pd2-save-sync status`                 | Display comparison table between local and OneDrive                                |
| `pd2-save-sync push`                   | Force upload local save files to OneDrive                                          |
| `pd2-save-sync pull`                   | Download OneDrive saves to local (protected against overwriting newer local saves) |
| `pd2-save-sync watch`                  | Continuously watch local folder and sync on changes                                |
| `pd2-save-sync history [file]`         | List version history snapshots in OneDrive                                         |
| `pd2-save-sync restore <file>`         | Restore a historical snapshot to your save folder                                  |
| `pd2-save-sync init`                   | Generate a `pd2-sync.toml` configuration file                                      |
