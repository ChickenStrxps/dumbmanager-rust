//! Where DumbManager Rust keeps its files.
//!
//! Everything lives under one data directory (in the desktop app: the Tauri
//! app data directory for `io.dumbmanager.rust`). This module refuses
//! directories that look like the original DumbManager/DumbAPI data, and keeps
//! exports in a fixed child folder that must not be a link to somewhere else.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, CoreResult};

/// SQLite file name. Deliberately different from the original app's `manager.sqlite`.
pub const DATABASE_FILE_NAME: &str = "dumbmanager-rust.sqlite3";
/// The only folder exports are written to.
pub const EXPORTS_DIR_NAME: &str = "exports";

/// Final path components used by the original installation's data folders.
const LEGACY_DIR_NAMES: [&str; 2] = ["dumbmanager", "dumbapi"];
/// Files that only exist in the original installation's data folders.
const LEGACY_MARKERS: [&str; 4] = [
    "manager.sqlite",
    "desktop.token",
    "t3-connection.json",
    "phone-share.json",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
}

impl AppPaths {
    /// Validates and creates the data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> CoreResult<Self> {
        let data_dir = data_dir.into();
        if !data_dir.is_absolute() {
            return Err(CoreError::invalid(
                "The app data directory must be an absolute path.",
            ));
        }
        refuse_legacy_location(&data_dir)?;
        if fs::symlink_metadata(&data_dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(CoreError::invalid(
                "The app data directory must be a real folder, not a link to another location.",
            ));
        }
        fs::create_dir_all(&data_dir)?;
        refuse_legacy_location(&fs::canonicalize(&data_dir)?)?;
        Ok(Self { data_dir })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE_NAME)
    }

    /// The exports folder path (may not exist yet).
    pub fn exports_dir(&self) -> PathBuf {
        self.data_dir.join(EXPORTS_DIR_NAME)
    }

    /// Creates the exports folder if needed and verifies it is a real folder
    /// directly inside the data directory (not a symlink or junction).
    /// Returns the plain (non-`\\?\`) path, suitable for display and the shell.
    pub fn ensure_exports_dir(&self) -> CoreResult<PathBuf> {
        let dir = self.exports_dir();
        // Concurrent first exports may both try to create this directory.
        // AlreadyExists is fine only after inspecting the actual entry below.
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let meta = fs::symlink_metadata(&dir)?;
        // On Windows `is_symlink` is also true for junctions/mount points.
        if meta.file_type().is_symlink() {
            return Err(CoreError::invalid(
                "The exports folder is a link to another location. Remove the link; DumbManager Rust only writes to its own folder.",
            ));
        }
        if !meta.is_dir() {
            return Err(CoreError::invalid(
                "The exports path exists but is not a folder. Move that file away and try again.",
            ));
        }
        // Belt and braces: after resolving every link in the path, the exports
        // folder must still be a direct child of the resolved data directory.
        let real_dir = fs::canonicalize(&dir)?;
        let real_root = fs::canonicalize(&self.data_dir)?;
        if real_dir.parent() != Some(real_root.as_path()) {
            return Err(CoreError::invalid(
                "The exports folder resolves outside the app data directory. Nothing was written.",
            ));
        }
        Ok(dir)
    }
}

fn refuse_legacy_location(dir: &Path) -> CoreResult<()> {
    let final_name = dir
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if LEGACY_DIR_NAMES.contains(&final_name.as_str()) {
        return Err(CoreError::invalid(
            "Refusing to use a folder named like the original DumbManager/DumbAPI data. DumbManager Rust keeps its own separate data directory.",
        ));
    }
    if LEGACY_MARKERS
        .iter()
        .any(|marker| dir.join(marker).exists())
    {
        return Err(CoreError::invalid(
            "Refusing to use a folder that contains the original DumbManager's data. DumbManager Rust keeps its own separate data directory.",
        ));
    }
    Ok(())
}
