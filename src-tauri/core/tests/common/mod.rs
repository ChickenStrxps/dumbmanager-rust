//! Shared helpers. Every test gets its own temporary data directory.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use dumbmanager_core::{AppPaths, Itch, ItchInput, Store};

/// Unique folder under the system temp directory, removed on drop.
/// (std-only on purpose: the core crate has no dev-dependencies.)
pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dumbmanager-rust-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // remove_dir_all removes links themselves, never their targets.
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct Fixture {
    // Field order matters: the store must close before the directory is removed.
    pub store: Store,
    pub paths: AppPaths,
    pub dir: TestDir,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = TestDir::new();
        let paths = AppPaths::new(dir.path().join("io.dumbmanager.rust")).expect("paths");
        let store = Store::open(&paths.database_path()).expect("open store");
        Self { store, paths, dir }
    }

    /// Closes and reopens the database from disk.
    pub fn reopen(self) -> Self {
        let Fixture { store, paths, dir } = self;
        drop(store);
        let store = Store::open(&paths.database_path()).expect("reopen store");
        Self { store, paths, dir }
    }

    pub fn add(&mut self, title: &str, kind: &str, project: &str) -> Itch {
        self.store
            .save_itch(&new_input(title, kind, project))
            .expect("save itch")
    }
}

pub fn new_input(title: &str, kind: &str, project: &str) -> ItchInput {
    ItchInput {
        id: None,
        title: Some(title.to_owned()),
        description: None,
        project: Some(project.to_owned()),
        kind: Some(kind.to_owned()),
    }
}

pub fn edit_input(itch: &Itch) -> ItchInput {
    ItchInput {
        id: Some(itch.id.clone()),
        title: Some(itch.title.clone()),
        description: Some(itch.description.clone()),
        project: Some(itch.project.clone()),
        kind: Some(itch.kind.as_str().to_owned()),
    }
}
