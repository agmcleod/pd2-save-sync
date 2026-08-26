use pd2_save_sync::config::Config;
use pd2_save_sync::history::{archive_version, list_versions, restore_version};
use pd2_save_sync::state::SyncState;
use pd2_save_sync::sync::{execute_sync, plan_sync, scan_level_one_files, SyncAction, SyncMode};
use std::fs;
use std::thread::sleep;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_level_one_only_and_ignore_subfolders() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let save_dir = root.join("Save");
    fs::create_dir_all(&save_dir).unwrap();

    // Direct files
    let char_file = save_dir.join("Paladin.d2s");
    fs::write(&char_file, b"paladin save data").unwrap();

    let map_file = save_dir.join("Paladin.map");
    fs::write(&map_file, b"paladin map data").unwrap();

    // Subdirectory and nested files (should be ignored)
    let subfolder = save_dir.join("BackupSubfolder");
    fs::create_dir_all(&subfolder).unwrap();
    fs::write(subfolder.join("Ignored.d2s"), b"should be ignored").unwrap();

    // Ignored file
    fs::write(save_dir.join("desktop.ini"), b"ignore me").unwrap();

    let ignored = vec!["desktop.ini".to_string()];
    let scanned = scan_level_one_files(&save_dir, &ignored).unwrap();

    let names: Vec<String> = scanned
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Paladin.d2s".to_string()));
    assert!(names.contains(&"Paladin.map".to_string()));
    assert!(!names.contains(&"Ignored.d2s".to_string()));
    assert!(!names.contains(&"desktop.ini".to_string()));
}

#[test]
fn test_sync_push_local_to_onedrive_and_history() {
    let temp = TempDir::new().unwrap();
    let local_dir = temp.path().join("LocalSave");
    let remote_dir = temp.path().join("OneDriveSave");

    fs::create_dir_all(&local_dir).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let char_path = local_dir.join("Sorceress.d2s");
    fs::write(&char_path, b"sorceress level 99").unwrap();

    let config = Config {
        local_dir: local_dir.clone(),
        remote_dir: remote_dir.clone(),
        max_versions: 10,
        ignored_files: vec![],
    };

    let items = plan_sync(&config, SyncMode::TwoWay).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, SyncAction::PushToRemote);

    let summary = execute_sync(&items, &config, false).unwrap();
    assert_eq!(summary.pushed, 1);
    assert_eq!(summary.pulled, 0);

    // Verify remote file exists with same content
    let remote_char = remote_dir.join("Sorceress.d2s");
    assert!(remote_char.exists());
    assert_eq!(fs::read(&remote_char).unwrap(), b"sorceress level 99");

    // Verify history recorded
    let history = list_versions(&remote_dir, "Sorceress.d2s").unwrap();
    assert_eq!(history.len(), 1);

    // Verify state was saved
    let state = SyncState::load(&local_dir);
    assert!(state.get_file("Sorceress.d2s").is_some());
}

#[test]
fn test_sync_pull_when_onedrive_newer() {
    let temp = TempDir::new().unwrap();
    let local_dir = temp.path().join("LocalSave");
    let remote_dir = temp.path().join("OneDriveSave");

    fs::create_dir_all(&local_dir).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    // Local file created first
    let local_file = local_dir.join("Barbarian.d2s");
    fs::write(&local_file, b"barbarian level 10").unwrap();

    sleep(Duration::from_millis(50));

    // Remote file created later (newer from another PC)
    let remote_file = remote_dir.join("Barbarian.d2s");
    fs::write(&remote_file, b"barbarian level 50 (from PC 2)").unwrap();

    let config = Config {
        local_dir: local_dir.clone(),
        remote_dir: remote_dir.clone(),
        max_versions: 10,
        ignored_files: vec![],
    };

    let items = plan_sync(&config, SyncMode::TwoWay).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, SyncAction::PullToLocal);

    let summary = execute_sync(&items, &config, false).unwrap();
    assert_eq!(summary.pulled, 1);
    assert_eq!(fs::read(&local_file).unwrap(), b"barbarian level 50 (from PC 2)");
}

#[test]
fn test_conflict_protection_when_destination_is_newer() {
    let temp = TempDir::new().unwrap();
    let local_dir = temp.path().join("LocalSave");
    let remote_dir = temp.path().join("OneDriveSave");

    fs::create_dir_all(&local_dir).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    // Remote has an old update
    let remote_file = remote_dir.join("Druid.d2s");
    fs::write(&remote_file, b"druid old remote").unwrap();

    sleep(Duration::from_millis(50));

    // Local destination was played more recently (newer timestamp)
    let local_file = local_dir.join("Druid.d2s");
    fs::write(&local_file, b"druid newest local").unwrap();

    let config = Config {
        local_dir: local_dir.clone(),
        remote_dir: remote_dir.clone(),
        max_versions: 10,
        ignored_files: vec![],
    };

    // If pulling, local file MUST NOT be overwritten because local timestamp is newer
    let items = plan_sync(&config, SyncMode::PullOnly).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, SyncAction::Conflict);

    let summary = execute_sync(&items, &config, false).unwrap();
    assert_eq!(summary.pulled, 0);
    assert_eq!(summary.conflicts, 1);
    // Local file was untouched
    assert_eq!(fs::read(&local_file).unwrap(), b"druid newest local");
}

#[test]
fn test_three_way_conflict_detection_when_both_sides_diverged() {
    let temp = TempDir::new().unwrap();
    let local_dir = temp.path().join("LocalSave");
    let remote_dir = temp.path().join("OneDriveSave");

    fs::create_dir_all(&local_dir).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let config = Config {
        local_dir: local_dir.clone(),
        remote_dir: remote_dir.clone(),
        max_versions: 10,
        ignored_files: vec![],
    };

    // Step 1: Initial synchronized state (e.g. baseline save)
    let base_content = b"druid base level 1";
    fs::write(local_dir.join("Druid.d2s"), base_content).unwrap();
    let items = plan_sync(&config, SyncMode::TwoWay).unwrap();
    execute_sync(&items, &config, false).unwrap();

    // Step 2: Simulate both sides modifying independently while offline
    // Local was played to Level 20
    fs::write(local_dir.join("Druid.d2s"), b"druid local level 20").unwrap();
    // Remote was played on another PC to Level 30
    fs::write(remote_dir.join("Druid.d2s"), b"druid remote PC2 level 30").unwrap();

    // Step 3: Plan sync - should detect 3-way conflict!
    let items = plan_sync(&config, SyncMode::TwoWay).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, SyncAction::Conflict);
    assert!(items[0].reason.contains("True Conflict"));

    // Step 4: Execute sync - protects local file and creates conflict copy
    let summary = execute_sync(&items, &config, false).unwrap();
    assert_eq!(summary.conflicts, 1);
    assert_eq!(fs::read(local_dir.join("Druid.d2s")).unwrap(), b"druid local level 20");

    // Verify a .conflict copy was preserved in local directory
    let files = fs::read_dir(&local_dir).unwrap();
    let conflict_file_exists = files.filter_map(|e| e.ok()).any(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        name.starts_with("Druid.conflict_")
    });
    assert!(conflict_file_exists, "A .conflict backup copy must be preserved");
}

#[test]
fn test_identical_hashes_skipped() {
    let temp = TempDir::new().unwrap();
    let local_dir = temp.path().join("LocalSave");
    let remote_dir = temp.path().join("OneDriveSave");

    fs::create_dir_all(&local_dir).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let content = b"necromancer identical content";
    fs::write(local_dir.join("Necro.d2s"), content).unwrap();
    fs::write(remote_dir.join("Necro.d2s"), content).unwrap();

    let config = Config {
        local_dir,
        remote_dir,
        max_versions: 10,
        ignored_files: vec![],
    };

    let items = plan_sync(&config, SyncMode::TwoWay).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, SyncAction::UpToDate);

    let summary = execute_sync(&items, &config, false).unwrap();
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.pushed, 0);
    assert_eq!(summary.pulled, 0);
}

#[test]
fn test_version_retention_and_pruning() {
    let temp = TempDir::new().unwrap();
    let remote_dir = temp.path().join("OneDriveSave");
    fs::create_dir_all(&remote_dir).unwrap();

    let file_path = temp.path().join("Assassin.d2s");

    // Create 12 distinct versions with max_versions = 7
    let max_versions = 7;
    for i in 1..=12 {
        fs::write(&file_path, format!("assassin version {}", i)).unwrap();
        archive_version(&remote_dir, &file_path, max_versions).unwrap();
        sleep(Duration::from_millis(15));
    }

    let versions = list_versions(&remote_dir, "Assassin.d2s").unwrap();
    assert_eq!(
        versions.len(),
        max_versions,
        "Should retain exactly the max_versions (7) newest snapshots"
    );

    // Verify newest version is version 12
    let newest = versions.last().unwrap();
    let restored_file = temp.path().join("Restored.d2s");
    restore_version(&newest.version_path, &restored_file).unwrap();
    assert_eq!(
        fs::read_to_string(&restored_file).unwrap(),
        "assassin version 12"
    );
}

#[test]
fn test_dry_run_mode_does_not_modify_files() {
    let temp = TempDir::new().unwrap();
    let local_dir = temp.path().join("LocalSave");
    let remote_dir = temp.path().join("OneDriveSave");

    fs::create_dir_all(&local_dir).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    fs::write(local_dir.join("Amazon.d2s"), b"amazon dry run").unwrap();

    let config = Config {
        local_dir,
        remote_dir: remote_dir.clone(),
        max_versions: 10,
        ignored_files: vec![],
    };

    let items = plan_sync(&config, SyncMode::TwoWay).unwrap();
    let summary = execute_sync(&items, &config, true).unwrap();

    assert_eq!(summary.pushed, 1);
    // Remote should NOT have been created in dry run mode
    assert!(!remote_dir.join("Amazon.d2s").exists());
}

#[test]
fn test_config_save_and_load() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("test_config.toml");

    let original = Config {
        local_dir: temp.path().join("DiabloSave"),
        remote_dir: temp.path().join("CloudSave"),
        max_versions: 8,
        ignored_files: vec!["custom.tmp".to_string()],
    };

    original.save_to_file(&config_path).unwrap();
    assert!(config_path.exists());

    let (loaded, found_path) = Config::load_or_default(Some(&config_path)).unwrap();
    assert_eq!(found_path, Some(config_path));
    assert_eq!(loaded.max_versions, 8);
    assert_eq!(loaded.ignored_files, vec!["custom.tmp".to_string()]);
    assert_eq!(loaded.local_dir, original.local_dir);
    assert_eq!(loaded.remote_dir, original.remote_dir);
}
