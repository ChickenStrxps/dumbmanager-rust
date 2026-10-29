//! Manual, serialized updater operations. The frontend never supplies a URL or signature.
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::Duration;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use uuid::Uuid;

const PLACEHOLDER: &str = "REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY_BEFORE_RELEASE";

#[derive(Default)]
pub struct UpdateState {
    busy: AtomicBool,
    candidate: Mutex<Option<(String, Update)>>,
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl UpdateState {
    fn enter(&self) -> Result<Busy<'_>, String> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| "An update operation is already in progress. Please wait.".to_string())?;
        Ok(Busy(&self.busy))
    }
}
#[derive(Serialize)]
pub struct Candidate {
    id: String,
    version: String,
    current_version: String,
}

#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<Candidate>, String> {
    let _busy = state.enter()?;
    *state
        .candidate
        .lock()
        .map_err(|_| "Updater state is unavailable.")? = None;
    let key = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if key.is_empty() || key == PLACEHOLDER {
        return Err("Updates are not configured in this build. Contact the publisher or install a configured release.".into());
    }
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Could not prepare update check: {e}"))?;
    let found = updater
        .check()
        .await
        .map_err(|e| format!("Update check failed. Check your connection and try again: {e}"))?;
    if let Some(update) = found {
        let id = Uuid::new_v4().to_string();
        let result = Candidate {
            id: id.clone(),
            version: update.version.clone(),
            current_version: update.current_version.clone(),
        };
        *state
            .candidate
            .lock()
            .map_err(|_| "Updater state is unavailable.")? = Some((id, update));
        Ok(Some(result))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn update_install(
    state: State<'_, UpdateState>,
    candidate_id: String,
) -> Result<(), String> {
    let _busy = state.enter()?;
    let update = state
        .candidate
        .lock()
        .map_err(|_| "Updater state is unavailable.")?
        .as_ref()
        .filter(|(id, _)| id == &candidate_id)
        .map(|(_, update)| update.clone())
        .ok_or("This update is no longer selected. Check for updates again.")?;
    // The official plugin verifies both the signature and signed version before
    // launching the installer. On failure the same candidate can be retried.
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            format!("Update was not installed: {e}. Your work is still open; retry or check again.")
        })
}
