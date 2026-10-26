//! Explicit, isolated QA for the native command service and real card renderer.
//!
//! Invoked by `cargo run --example native-check -- <new-temporary-root>`.
//! The desktop executable has no data-directory override. This harness only
//! creates a fresh direct child of the OS temporary folder; an existing path
//! is refused. Its synthetic QA records never enter normal application data.

use std::fs;
use std::io::Write;
use std::path::Path;

use dumbmanager_core::{
    AppPaths, CardExport, CoreError, CoreResult, IntegrationState, ItchInput, ItchStatus,
    SettingsInput, Store,
};
use serde::Serialize;

use crate::state::AppState;
use crate::{card, APP_ID, APP_NAME};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCheckReport {
    pub ok: bool,
    pub name: String,
    pub version: String,
    pub root: String,
    pub data_dir: String,
    pub checks: Vec<String>,
    pub exports: Vec<CardExport>,
    pub limitations: Vec<String>,
}

fn require(condition: bool, message: &str) -> CoreResult<()> {
    if condition {
        Ok(())
    } else {
        Err(CoreError::Storage(format!(
            "Native self-check failed: {message}"
        )))
    }
}

pub(crate) fn create_temporary_root(root: &Path) -> CoreResult<()> {
    let unsafe_component = root.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    });
    if !root.is_absolute() || unsafe_component || root.file_name().is_none() {
        return Err(CoreError::invalid(
            "Native self-check needs an absolute, new direct child of the OS temporary directory.",
        ));
    }
    let temp = fs::canonicalize(std::env::temp_dir())?;
    let parent = root
        .parent()
        .ok_or_else(|| CoreError::invalid("Missing temporary parent."))?;
    if fs::canonicalize(parent)? != temp {
        return Err(CoreError::invalid(
            "Native self-check may only create a new direct child of the OS temporary directory. No application data path is accepted.",
        ));
    }
    // Atomic create: never adopt or clear an existing directory, even if empty.
    fs::create_dir(root).map_err(|error| {
        CoreError::Io(format!(
            "Could not create a new native self-check root: {error}. Choose a path that does not exist."
        ))
    })?;
    Ok(())
}

fn open(paths: &AppPaths) -> CoreResult<AppState> {
    Ok(AppState::new(
        paths.clone(),
        Store::open(&paths.database_path())?,
        card::renderer(),
    ))
}

/// Runs fixed, real native operations. Leaves the new root and an evidence
/// report for inspection. Does not open Explorer, a window, or any connection.
pub fn run(root: &Path) -> CoreResult<NativeCheckReport> {
    create_temporary_root(root)?;
    fs::write(
        root.join("NATIVE-CHECK.txt"),
        b"Isolated DumbManager Rust native QA. Synthetic test input only.\n",
    )?;
    let paths = AppPaths::new(root.join(APP_ID))?;
    let state = open(&paths)?;
    let version = env!("CARGO_PKG_VERSION");
    let mut checks = Vec::new();

    let initial = state.app_snapshot(version)?;
    require(
        initial.name == APP_NAME && initial.counts.total == 0,
        "fresh snapshot must be empty",
    )?;
    require(
        state.itch_list(None, None, None)?.is_empty(),
        "fresh list must be empty",
    )?;
    require(
        initial.integrations.iter().all(|item| {
            if item.id == "card-renderer" {
                item.state == IntegrationState::Ready
            } else {
                item.state == IntegrationState::Unavailable
            }
        }),
        "integration statuses must reflect implemented features only",
    )?;
    checks.push("fresh snapshot, empty storage, honest integrations".to_owned());

    let saved_settings = state.settings_save(&SettingsInput {
        motion: Some("reduced".to_owned()),
        theme: Some("midnight".to_owned()),
        default_project: Some("native-qa".to_owned()),
    })?;
    let invalid_settings = state.settings_save(&SettingsInput {
        motion: Some("off".to_owned()),
        theme: Some("invalid-theme".to_owned()),
        default_project: Some("must-not-persist".to_owned()),
    });
    require(
        invalid_settings.is_err() && state.settings_get()? == saved_settings,
        "invalid settings must leave the entire previous value intact",
    )?;
    checks.push("settings validation and atomic save".to_owned());

    let input = ItchInput {
        title: Some("Native QA: keyboard review".to_owned()),
        description: Some("Synthetic QA input. Real SQLite persistence and native card export.\nNo external sends.".to_owned()),
        kind: Some("bug".to_owned()),
        ..ItchInput::default()
    };
    let created = state.itch_save(&input)?;
    require(
        created.status == ItchStatus::Open && created.project == "native-qa",
        "new itch must be open and use the saved default project",
    )?;
    let invalid = state.itch_save(&ItchInput {
        title: Some("   ".to_owned()),
        kind: Some("bug".to_owned()),
        ..ItchInput::default()
    });
    require(
        invalid.is_err() && state.app_snapshot(version)?.counts.total == 1,
        "invalid itch must not be saved",
    )?;
    let illegal_acceptance = state.itch_transition(&created.id, "resolved");
    require(
        illegal_acceptance.is_err(),
        "open itch must not be accepted without review",
    )?;
    state.itch_transition(&created.id, "stashed")?;
    state.itch_transition(&created.id, "open")?;
    state.itch_transition(&created.id, "ready")?;
    let edited = state.itch_save(&ItchInput {
        id: Some(created.id.clone()),
        title: Some("Native QA: keyboard review verified".to_owned()),
        ..ItchInput::default()
    })?;
    require(
        edited.status == ItchStatus::Ready,
        "editing must preserve review status",
    )?;
    let found = state.itch_list(Some("KEYBOARD SQLite"), Some("ready"), Some("native-qa"))?;
    require(
        found == vec![edited.clone()],
        "combined case-insensitive search/filter must find the saved itch",
    )?;
    require(
        state
            .itch_list(Some("no-such-qa-term"), None, None)?
            .is_empty(),
        "unmatched search must be empty",
    )?;
    let snapshot = state.app_snapshot(version)?;
    require(
        snapshot.counts.ready == 1 && snapshot.counts.resolved == 0,
        "nothing may be automatically accepted",
    )?;
    let json =
        serde_json::to_value(&edited).map_err(|error| CoreError::Storage(error.to_string()))?;
    require(
        json["createdAt"].is_string() && json["updatedAt"].is_string() && json["status"] == "ready",
        "native command serialization must use the IPC contract",
    )?;
    checks
        .push("create, edit, review transitions, search, filters, camelCase IPC values".to_owned());

    state.store().checkpoint()?;
    drop(state);
    let state = open(&paths)?;
    require(
        state.settings_get()? == saved_settings,
        "settings must survive restart",
    )?;
    require(
        state.itch_list(None, None, None)? == vec![edited.clone()],
        "itch and status must survive restart",
    )?;
    checks.push("database restart persistence".to_owned());

    let separate_paths = AppPaths::new(root.join("isolated-second-store"))?;
    let separate = open(&separate_paths)?;
    require(
        separate.itch_list(None, None, None)?.is_empty()
            && separate.settings_get()?.default_project.is_empty(),
        "separate stores must never share state",
    )?;
    drop(separate);
    checks.push("separate data-directory isolation".to_owned());

    let first_permit = state.exports.try_enter()?;
    let second_permit = state.exports.try_enter()?;
    let saturated = state.card_export(&edited.id, "png");
    require(
        matches!(saturated, Err(CoreError::Busy(_))) && !paths.exports_dir().exists(),
        "a saturated export gate must reject without writing",
    )?;
    drop((first_permit, second_permit));
    require(
        state.card_export(&edited.id, "jpg").is_err() && !paths.exports_dir().exists(),
        "invalid export formats must not write",
    )?;
    checks.push("two-slot export bound and invalid-format rejection".to_owned());

    let mut exports = Vec::new();
    for (format, dimensions) in [("png", (2400, 1350, 1)), ("gif", (1200, 675, 72))] {
        let export = state.card_export(&edited.id, format)?;
        require(
            (export.width, export.height, export.frames) == dimensions,
            "real renderer dimensions and frame count must match the renderer contract",
        )?;
        let path = Path::new(&export.path);
        require(
            path.parent() == Some(paths.exports_dir().as_path())
                && fs::metadata(path)?.len() == export.bytes
                && export.bytes > 0,
            "real export must exist directly in the fixed export directory",
        )?;
        exports.push(export);
    }
    require(
        state.itch_list(None, None, None)? == vec![edited],
        "export must not alter or accept the itch",
    )?;
    checks.push(
        "real PNG/GIF rendering, file checks, fixed folder, unchanged review status".to_owned(),
    );
    state.store().checkpoint()?;
    drop(state);

    let report = NativeCheckReport {
        ok: true,
        name: APP_NAME.to_owned(),
        version: version.to_owned(),
        root: root.display().to_string(),
        data_dir: paths.data_dir().display().to_string(),
        checks,
        exports,
        limitations: vec![
            "Exercises the shared native command service, not WebView IPC delivery or installer behavior.".to_owned(),
            "Explorer opening and all remote integrations are intentionally not invoked.".to_owned(),
            "Renderer font coverage is ASCII/Latin, not full Unicode typography.".to_owned(),
        ],
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("native-check-report.json"))?;
    serde_json::to_writer_pretty(&mut file, &report)
        .map_err(|error| CoreError::Io(error.to_string()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_existing_roots_and_paths_outside_temporary_parent() {
        assert!(create_temporary_root(&std::env::temp_dir()).is_err());
        assert!(create_temporary_root(
            &std::env::current_dir().unwrap().join("must-not-create-qa")
        )
        .is_err());
        assert!(create_temporary_root(Path::new("relative-qa")).is_err());

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let existing = std::env::temp_dir().join(format!(
            "dumbmanager-rust-check-refusal-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&existing).unwrap();
        let sentinel = existing.join("keep.txt");
        fs::write(&sentinel, b"must stay untouched").unwrap();
        assert!(run(&existing).is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"must stay untouched");
        assert_eq!(fs::read_dir(&existing).unwrap().count(), 1);
        fs::remove_file(&sentinel).unwrap();
        fs::remove_dir(&existing).unwrap();
    }
}
