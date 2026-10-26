//! Real concurrent connections/files, not a shared test connection mutex.
mod common;

use std::collections::HashSet;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;

use common::{new_input, Fixture, TestDir};
use dumbmanager_core::export::write_new_file;
use dumbmanager_core::{AppPaths, ListFilter, Motion, SettingsInput, Store};

#[test]
fn concurrent_writers_preserve_every_record_and_revision() {
    let fx = Fixture::new();
    let barrier = Arc::new(Barrier::new(4));
    let writers: Vec<_> = (0..4)
        .map(|writer| {
            let path = fx.paths.database_path();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                barrier.wait();
                for index in 0..24 {
                    store
                        .save_itch(&new_input(
                            &format!("writer-{writer}-{index}"),
                            "bug",
                            "concurrent",
                        ))
                        .unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let records = fx.store.list_itches(&ListFilter::all()).unwrap();
    assert_eq!(records.len(), 96);
    assert_eq!(
        records
            .iter()
            .map(|itch| &itch.id)
            .collect::<HashSet<_>>()
            .len(),
        96
    );
    assert_eq!(fx.store.counts().unwrap().open, 96);
    for itch in records {
        assert_eq!(fx.store.itch_history(&itch.id).unwrap().len(), 1);
    }
}

#[test]
fn simultaneous_first_open_initializes_one_database() {
    let dir = TestDir::new();
    let paths = AppPaths::new(dir.path().join("io.dumbmanager.rust")).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let writers: Vec<_> = (0..4)
        .map(|writer| {
            let path = paths.database_path();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let mut store = Store::open(&path).unwrap();
                store
                    .save_itch(&new_input(&format!("first-open-{writer}"), "idea", "p"))
                    .unwrap();
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let store = Store::open(&paths.database_path()).unwrap();
    assert_eq!(store.counts().unwrap().total, 4);
}

#[test]
fn concurrent_partial_settings_saves_do_not_lose_other_fields() {
    let fx = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let writers: Vec<_> = (0..2)
        .map(|writer| {
            let path = fx.paths.database_path();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                barrier.wait();
                for index in 0..24 {
                    let input = if writer == 0 {
                        SettingsInput {
                            motion: Some("off".to_owned()),
                            ..SettingsInput::default()
                        }
                    } else {
                        SettingsInput {
                            default_project: Some(format!("project-{index}")),
                            ..SettingsInput::default()
                        }
                    };
                    store.save_settings(&input).unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let saved = fx.store.settings().unwrap();
    assert_eq!(saved.motion, Motion::Off);
    assert_eq!(saved.default_project, "project-23");
}

#[test]
fn concurrent_first_export_directory_creation_is_safe() {
    let dir = TestDir::new();
    let paths = AppPaths::new(dir.path().join("io.dumbmanager.rust")).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let creators: Vec<_> = (0..8)
        .map(|_| {
            let paths = paths.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                paths.ensure_exports_dir().unwrap()
            })
        })
        .collect();
    for creator in creators {
        assert_eq!(creator.join().unwrap(), paths.exports_dir());
    }
}

#[test]
fn concurrent_file_publication_never_replaces_another_export() {
    let dir = TestDir::new();
    let barrier = Arc::new(Barrier::new(8));
    let writers: Vec<_> = (0u8..8)
        .map(|writer| {
            let path = dir.path().to_path_buf();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let output = write_new_file(&path, "card", "png", &[writer; 128]).unwrap();
                (output, writer)
            })
        })
        .collect();
    let mut names = HashSet::new();
    for writer in writers {
        let (path, byte) = writer.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), [byte; 128]);
        assert!(names.insert(path));
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 8);
}
