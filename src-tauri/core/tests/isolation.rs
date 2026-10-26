//! The Rust app must never adopt, modify or share another installation's data.

mod common;

use std::fs;
use std::path::Path;

use common::{new_input, Fixture};
use dumbmanager_core::paths::DATABASE_FILE_NAME;
use dumbmanager_core::store::{APPLICATION_ID, SCHEMA_VERSION};
use dumbmanager_core::{AppPaths, CoreError, ListFilter, Store};

fn file_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).expect("read file")
}

fn sidecars_absent(db: &Path) -> bool {
    let wal = db.with_extension("sqlite-wal");
    let shm = db.with_extension("sqlite-shm");
    let wal2 = Path::new(&format!("{}-wal", db.display())).to_path_buf();
    let shm2 = Path::new(&format!("{}-shm", db.display())).to_path_buf();
    !wal.exists() && !shm.exists() && !wal2.exists() && !shm2.exists()
}

#[test]
fn separate_data_dirs_never_share_state() {
    let mut a = Fixture::new();
    let b = Fixture::new();
    a.add("only in A", "bug", "p");
    assert!(b.store.list_itches(&ListFilter::all()).unwrap().is_empty());
    assert_ne!(a.paths.database_path(), b.paths.database_path());
}

#[test]
fn database_is_stamped_and_named_for_this_app() {
    let fx = Fixture::new();
    let path = fx.paths.database_path();
    assert_eq!(path.file_name().unwrap(), DATABASE_FILE_NAME);
    assert_ne!(DATABASE_FILE_NAME, "manager.sqlite");
    drop(fx.store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let app_id: i32 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app_id, APPLICATION_ID);
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn legacy_style_database_is_refused_untouched() {
    let dir = common::TestDir::new();
    let legacy = dir.path().join("legacy.sqlite");
    {
        // Same table layout the original DumbAPI manager uses (rollback journal).
        let conn = rusqlite::Connection::open(&legacy).unwrap();
        conn.execute_batch(
            "CREATE TABLE records(id TEXT PRIMARY KEY,kind TEXT NOT NULL,project TEXT NOT NULL,updated TEXT NOT NULL,data TEXT NOT NULL);
             CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
             INSERT INTO records VALUES ('x','itch','p','2026-01-01','{}');",
        )
        .unwrap();
    }
    let before = file_bytes(&legacy);
    let err = Store::open(&legacy).unwrap_err();
    assert!(matches!(err, CoreError::ForeignDatabase(_)), "{err:?}");
    assert_eq!(
        file_bytes(&legacy),
        before,
        "foreign database bytes must not change"
    );
    assert!(
        sidecars_absent(&legacy),
        "no WAL/SHM files may be created next to it"
    );
}

#[test]
fn database_owned_by_another_application_id_is_refused() {
    let dir = common::TestDir::new();
    let other = dir.path().join("other.sqlite3");
    {
        let conn = rusqlite::Connection::open(&other).unwrap();
        conn.pragma_update(None, "application_id", 0x1234_5678)
            .unwrap();
    }
    let before = file_bytes(&other);
    assert!(matches!(
        Store::open(&other).unwrap_err(),
        CoreError::ForeignDatabase(_)
    ));
    assert_eq!(file_bytes(&other), before);
}

#[test]
fn non_database_file_is_refused_untouched() {
    let dir = common::TestDir::new();
    let path = dir.path().join(DATABASE_FILE_NAME);
    fs::write(
        &path,
        b"this is not a sqlite database, it is someone's notes\n",
    )
    .unwrap();
    let before = file_bytes(&path);
    let err = Store::open(&path).unwrap_err();
    assert!(matches!(err, CoreError::Storage(_)), "{err:?}");
    assert_eq!(file_bytes(&path), before);
}

#[test]
fn newer_schema_is_refused() {
    let fx = Fixture::new();
    let path = fx.paths.database_path();
    drop(fx.store);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        conn.execute_batch("PRAGMA journal_mode = DELETE").unwrap();
    }
    let before = file_bytes(&path);
    let err = Store::open(&path).unwrap_err();
    assert!(
        err.to_string().contains("newer than this build supports"),
        "{err}"
    );
    assert_eq!(
        file_bytes(&path),
        before,
        "unsupported schema must not change journal mode or bytes"
    );
    assert!(sidecars_absent(&path));
}

#[test]
fn legacy_data_locations_are_refused() {
    let dir = common::TestDir::new();
    for name in ["DumbManager", "dumbmanager", "DumbAPI"] {
        let err = AppPaths::new(dir.path().join(name)).unwrap_err();
        assert!(matches!(err, CoreError::Invalid(_)), "{name}");
        assert!(
            !dir.path().join(name).exists(),
            "{name} must not be created"
        );
    }
    for marker in ["manager.sqlite", "desktop.token", "t3-connection.json"] {
        let legacy = dir
            .path()
            .join(format!("looks-legacy-{}", marker.replace('.', "-")));
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join(marker), b"original app data").unwrap();
        assert!(AppPaths::new(&legacy).is_err(), "{marker}");
        assert_eq!(
            fs::read_dir(&legacy).unwrap().count(),
            1,
            "nothing added next to {marker}"
        );
    }
    assert!(AppPaths::new("relative/data").is_err());
    // The real app directory name is fine.
    assert!(AppPaths::new(dir.path().join("io.dumbmanager.rust")).is_ok());
}

#[test]
fn store_lives_only_under_its_data_dir() {
    let mut fx = Fixture::new();
    fx.add("x", "bug", "p");
    let root = fx.dir.path();
    let data = fx.paths.data_dir().to_path_buf();
    // Only the app directory exists in the temp root; nothing leaked beside it.
    let top: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(top, vec![data.file_name().unwrap().to_owned()]);
    for entry in fs::read_dir(&data).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            name.starts_with(DATABASE_FILE_NAME),
            "unexpected file {name}"
        );
    }
}

#[test]
fn saving_with_other_store_open_on_other_dir_does_not_cross() {
    let mut a = Fixture::new();
    let mut b = Fixture::new();
    let in_a = a.add("A", "bug", "shared-name");
    let in_b = b
        .store
        .save_itch(&new_input("B", "bug", "shared-name"))
        .unwrap();
    assert!(matches!(
        b.store.get_itch(&in_a.id),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        a.store.get_itch(&in_b.id),
        Err(CoreError::NotFound(_))
    ));
}
