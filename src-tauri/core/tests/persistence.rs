mod common;

use common::{edit_input, new_input, Fixture};
use dumbmanager_core::{
    ItchInput, ItchKind, ItchStatus, ListFilter, Motion, SettingsInput, StatusCounts, Store, Theme,
};

#[test]
fn fresh_store_is_genuinely_empty() {
    let fx = Fixture::new();
    assert!(fx.store.list_itches(&ListFilter::all()).unwrap().is_empty());
    assert_eq!(fx.store.counts().unwrap(), StatusCounts::default());
    let settings = fx.store.settings().unwrap();
    assert_eq!(settings.motion, Motion::Full);
    assert_eq!(settings.theme, Theme::Midnight);
    assert_eq!(settings.default_project, "");
    assert_eq!(settings.updated_at, None);
}

#[test]
fn itches_survive_reopen_exactly() {
    let mut fx = Fixture::new();
    let mut input = new_input("Login button ignores Enter", "bug", "dumbmanager-rust");
    input.description = Some("Steps:\r\n1. Type password\r\n2. Press Enter".to_owned());
    let created = fx.store.save_itch(&input).unwrap();
    assert_eq!(created.status, ItchStatus::Open);
    assert_eq!(created.kind, ItchKind::Bug);
    assert_eq!(
        created.description,
        "Steps:\n1. Type password\n2. Press Enter"
    );
    assert_eq!(created.created_at, created.updated_at);
    let second = fx.add("Dark mode for cards", "idea", "dumbmanager-rust");

    let fx = fx.reopen();
    let listed = fx.store.list_itches(&ListFilter::all()).unwrap();
    assert_eq!(listed, vec![second.clone(), created.clone()]);
    assert_eq!(fx.store.get_itch(&created.id).unwrap(), created);
}

#[test]
fn explicit_headless_delete_is_atomic_cascades_history_and_survives_restart() {
    let mut fx = Fixture::new();
    let keep = fx.add("keep", "idea", "core");
    let remove = fx.add("remove explicitly", "bug", "core");
    let ready = fx.store.transition_itch(&remove.id, "ready").unwrap();
    assert_eq!(fx.store.itch_history(&remove.id).unwrap().len(), 2);
    let removed = fx.store.delete_itch(&remove.id).unwrap();
    assert_eq!(removed, ready);
    assert!(fx.store.itch_history(&remove.id).unwrap().is_empty());

    let mut fx = fx.reopen();
    assert_eq!(
        fx.store.get_itch(&remove.id).unwrap_err().code(),
        "not_found"
    );
    assert_eq!(
        fx.store.list_itches(&ListFilter::all()).unwrap(),
        vec![keep.clone()]
    );
    assert_eq!(fx.store.counts().unwrap().total, 1);
    let later = fx.add("newest after deletion", "idea", "core");
    assert_eq!(
        fx.store.list_itches(&ListFilter::all()).unwrap(),
        vec![later, keep]
    );
}

#[test]
fn invalid_or_missing_headless_delete_preserves_existing_records() {
    let mut fx = Fixture::new();
    let keep = fx.add("keep", "bug", "p");
    assert_eq!(
        fx.store.delete_itch("not-an-id").unwrap_err().code(),
        "invalid"
    );
    assert_eq!(
        fx.store
            .delete_itch("0f8fad5b-d9cb-469f-a165-70867728950e")
            .unwrap_err()
            .code(),
        "not_found"
    );
    let fx = fx.reopen();
    assert_eq!(fx.store.get_itch(&keep.id).unwrap(), keep);
    assert_eq!(fx.store.itch_history(&keep.id).unwrap().len(), 1);
}

#[test]
fn edits_and_transitions_survive_reopen() {
    let mut fx = Fixture::new();
    let itch = fx.add("Slow search", "bug", "core");
    let mut edit = edit_input(&itch);
    edit.title = Some("Search is slow with 5k itches".to_owned());
    edit.kind = Some("improvement".to_owned());
    let edited = fx.store.save_itch(&edit).unwrap();
    let readied = fx.store.transition_itch(&itch.id, "ready").unwrap();

    let fx = fx.reopen();
    let loaded = fx.store.get_itch(&itch.id).unwrap();
    assert_eq!(loaded.title, "Search is slow with 5k itches");
    assert_eq!(loaded.kind, ItchKind::Improvement);
    assert_eq!(loaded.status, ItchStatus::Ready);
    assert_eq!(loaded.created_at, itch.created_at);
    assert_eq!(loaded.updated_at, readied.updated_at);
    assert!(edited.updated_at >= itch.updated_at);
}

#[test]
fn settings_survive_reopen_and_feed_default_project() {
    let mut fx = Fixture::new();
    let saved = fx
        .store
        .save_settings(&SettingsInput {
            motion: Some("reduced".to_owned()),
            theme: Some("midnight".to_owned()),
            default_project: Some("  Dome X3 ".to_owned()),
        })
        .unwrap();
    assert_eq!(saved.motion, Motion::Reduced);
    assert_eq!(saved.default_project, "Dome X3");
    assert!(saved.updated_at.is_some());

    let mut fx = fx.reopen();
    assert_eq!(fx.store.settings().unwrap(), saved);

    // A new itch without a project uses the default project...
    let itch = fx
        .store
        .save_itch(&ItchInput {
            title: Some("Uses default".to_owned()),
            kind: Some("idea".to_owned()),
            ..ItchInput::default()
        })
        .unwrap();
    assert_eq!(itch.project, "Dome X3");

    // ...and falls back to `unsorted` once the default is cleared.
    fx.store
        .save_settings(&SettingsInput {
            default_project: Some(String::new()),
            ..SettingsInput::default()
        })
        .unwrap();
    let itch = fx
        .store
        .save_itch(&ItchInput {
            title: Some("No default".to_owned()),
            kind: Some("idea".to_owned()),
            project: Some("   ".to_owned()),
            ..ItchInput::default()
        })
        .unwrap();
    assert_eq!(itch.project, "unsorted");
}

#[test]
fn partial_settings_save_keeps_other_fields_and_noop_does_not_write() {
    let mut fx = Fixture::new();
    let first = fx
        .store
        .save_settings(&SettingsInput {
            motion: Some("off".to_owned()),
            default_project: Some("core".to_owned()),
            ..SettingsInput::default()
        })
        .unwrap();
    let second = fx
        .store
        .save_settings(&SettingsInput {
            theme: Some("midnight".to_owned()),
            ..SettingsInput::default()
        })
        .unwrap();
    assert_eq!(
        second, first,
        "identical values must not rewrite or re-stamp settings"
    );
    assert_eq!(second.motion, Motion::Off);
    assert_eq!(second.default_project, "core");
}

#[test]
fn counts_track_statuses() {
    let mut fx = Fixture::new();
    let a = fx.add("a", "bug", "p");
    let b = fx.add("b", "bug", "p");
    let c = fx.add("c", "idea", "p");
    fx.add("d", "idea", "q");
    fx.store.transition_itch(&a.id, "stashed").unwrap();
    fx.store.transition_itch(&b.id, "ready").unwrap();
    fx.store.transition_itch(&c.id, "ready").unwrap();
    fx.store.transition_itch(&c.id, "resolved").unwrap();
    let counts = fx.store.counts().unwrap();
    assert_eq!(
        counts,
        StatusCounts {
            open: 1,
            stashed: 1,
            ready: 1,
            resolved: 1,
            total: 4
        }
    );
    let fx = fx.reopen();
    assert_eq!(fx.store.counts().unwrap(), counts);
}

#[test]
fn second_connection_sees_committed_writes() {
    // Simulates a second process on the same file (WAL + busy timeout).
    let mut fx = Fixture::new();
    let mut other = Store::open(&fx.paths.database_path()).unwrap();
    let from_first = fx.add("from first", "bug", "p");
    let from_other = other
        .save_itch(&new_input("from other", "idea", "p"))
        .unwrap();
    let seen_by_first = fx.store.list_itches(&ListFilter::all()).unwrap();
    let seen_by_other = other.list_itches(&ListFilter::all()).unwrap();
    assert_eq!(seen_by_first, vec![from_other.clone(), from_first.clone()]);
    assert_eq!(seen_by_other, seen_by_first);
}

#[test]
fn failed_validation_writes_nothing() {
    let mut fx = Fixture::new();
    let itch = fx.add("keep me", "bug", "p");
    let mut bad = edit_input(&itch);
    bad.title = Some("renamed".to_owned());
    bad.kind = Some("feature".to_owned()); // invalid kind: whole save must fail
    assert!(fx.store.save_itch(&bad).is_err());
    let fx = fx.reopen();
    assert_eq!(fx.store.get_itch(&itch.id).unwrap(), itch);
}

#[test]
fn database_failure_rolls_back_all_settings_fields() {
    let mut fx = Fixture::new();
    let original = fx
        .store
        .save_settings(&SettingsInput {
            motion: Some("reduced".to_owned()),
            default_project: Some("keep-original".to_owned()),
            ..SettingsInput::default()
        })
        .unwrap();
    let connection = rusqlite::Connection::open(fx.paths.database_path()).unwrap();
    // Fail after the earlier motion update has already executed in the same
    // transaction. This tests rollback on an actual SQLite failure, beyond
    // rejecting bad input before the transaction starts.
    connection
        .execute_batch(
            "CREATE TRIGGER reject_qa_theme BEFORE UPDATE ON settings
         WHEN NEW.key = 'theme'
         BEGIN SELECT RAISE(ABORT, 'injected settings failure'); END;",
        )
        .unwrap();
    let error = fx
        .store
        .save_settings(&SettingsInput {
            motion: Some("off".to_owned()),
            default_project: Some("must-not-persist".to_owned()),
            ..SettingsInput::default()
        })
        .unwrap_err();
    assert_eq!(error.code(), "storage");
    assert_eq!(fx.store.settings().unwrap(), original);
    drop(connection);
    let fx = fx.reopen();
    assert_eq!(fx.store.settings().unwrap(), original);
}

#[test]
fn checkpoint_makes_the_main_file_a_complete_copy() {
    let mut fx = Fixture::new();
    let kept = fx.add("Survives a file-only backup", "idea", "p");
    fx.store.checkpoint().unwrap();

    let db = fx.paths.database_path();
    let wal = std::path::PathBuf::from(format!("{}-wal", db.display()));
    if wal.exists() {
        assert_eq!(
            std::fs::metadata(&wal).unwrap().len(),
            0,
            "log folded into the main file"
        );
    }
    // Copy only the .sqlite3 file, as a user making a manual backup would.
    let backup_dir = fx.dir.path().join("backup");
    std::fs::create_dir_all(&backup_dir).unwrap();
    let copy = backup_dir.join("copy.sqlite3");
    std::fs::copy(&db, &copy).unwrap();
    let restored = Store::open(&copy).unwrap();
    assert_eq!(restored.get_itch(&kept.id).unwrap(), kept);
}
