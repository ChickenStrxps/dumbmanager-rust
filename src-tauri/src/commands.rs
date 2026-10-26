//! IPC commands from `docs/ipc-contract.md`.
//!
//! Each command validates its input in `dumbmanager-core` and runs on the
//! blocking thread pool, so SQLite and file work never stall the window or
//! the async runtime. Failures reject the JavaScript promise with a readable
//! message string. Only bundled local pages in the main window can call these
//! (see `capabilities/main.json` and `build.rs`).

use serde::{Serialize, Serializer};
use tauri::{AppHandle, Manager};

use dumbmanager_core::{
    AppSnapshot, CardExport, CoreError, CoreResult, Itch, ItchInput, SettingsInput, SettingsView,
};

use crate::platform;
use crate::state::AppState;
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::Value;

/// The value a rejected `invoke(...)` promise carries: a plain string.
#[derive(Debug)]
pub struct CommandError(String);

impl Serialize for CommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl From<CoreError> for CommandError {
    fn from(error: CoreError) -> Self {
        Self(error.to_string())
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Runs `work` with the managed state on the blocking thread pool.
async fn blocking<T, F>(app: AppHandle, work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> CoreResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|_| {
            CommandError(
                "The operation stopped unexpectedly. Unfinished database changes were rolled back; please try again."
                    .to_owned(),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn app_snapshot(app: AppHandle) -> CommandResult<AppSnapshot> {
    let version = app.package_info().version.to_string();
    blocking(app, move |state| state.app_snapshot(&version)).await
}

/// All arguments are optional; missing ones mean "everything".
#[tauri::command]
pub async fn itch_list(
    app: AppHandle,
    query: Option<String>,
    status: Option<String>,
    project: Option<String>,
) -> CommandResult<Vec<Itch>> {
    blocking(app, move |state| {
        state.itch_list(query.as_deref(), status.as_deref(), project.as_deref())
    })
    .await
}

/// Creates (no `id`) or edits (with `id`) an itch. Never changes its status.
#[tauri::command]
pub async fn itch_save(app: AppHandle, input: ItchInput) -> CommandResult<Itch> {
    blocking(app, move |state| state.itch_save(&input)).await
}

/// Moves an itch between statuses. `resolved` is only accepted from `ready`;
/// the UI sends it from the user's explicit acceptance button and nothing in
/// the backend ever resolves an itch on its own.
#[tauri::command]
pub async fn itch_transition(app: AppHandle, id: String, status: String) -> CommandResult<Itch> {
    blocking(app, move |state| state.itch_transition(&id, &status)).await
}

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> CommandResult<SettingsView> {
    blocking(app, AppState::settings_get).await
}

#[tauri::command]
pub async fn settings_save(app: AppHandle, input: SettingsInput) -> CommandResult<SettingsView> {
    blocking(app, move |state| state.settings_save(&input)).await
}

/// Renders a card into the app's exports folder. The database lock is held
/// only while the itch is read, not while pixels are drawn.
#[tauri::command]
pub async fn card_export(app: AppHandle, id: String, format: String) -> CommandResult<CardExport> {
    blocking(app, move |state| state.card_export(&id, &format)).await
}

/// Opens this app's own exports folder in Explorer. It never opens a path
/// supplied by the page.
#[tauri::command]
pub async fn open_export_folder(app: AppHandle) -> CommandResult<()> {
    blocking(app, |state| {
        let dir = state.paths.ensure_exports_dir()?;
        platform::open_folder(&dir)
    })
    .await
}

/// Same-origin `/api/*` compatibility for the original, bundled frontend.
/// Domain failures retain the original `{error, code, status}` shape.
#[tauri::command]
pub async fn legacy_api(app: AppHandle, path: String, payload: Value) -> LegacyResult<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().legacy_api(&path, &payload)
    })
    .await
    .map_err(|_| LegacyError {
        error: "The native operation stopped. Uncommitted changes were rolled back; retry with the same request key.".into(),
        code: "internal".into(),
        status: 500,
    })?
}
