//! State shared by every command, managed by Tauri.

use dumbmanager_core::legacy::{self, LegacyError, LegacyResult};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Mutex, MutexGuard, PoisonError};

use dumbmanager_core::{
    integration_statuses, time, AppPaths, AppSnapshot, CardExport, CardRenderer, CoreResult,
    ExportGate, ExportJob, Itch, ItchInput, ListFilter, Settings, SettingsInput, SettingsView,
    Store,
};

use crate::APP_NAME;

pub struct AppState {
    store: Mutex<Store>,
    pub paths: AppPaths,
    pub started_at: String,
    /// `None` makes `card_export` answer with an explicit "unavailable" error.
    pub renderer: Option<Box<dyn CardRenderer>>,
    pub exports: ExportGate,
}

impl AppState {
    pub fn new(paths: AppPaths, store: Store, renderer: Option<Box<dyn CardRenderer>>) -> Self {
        Self {
            store: Mutex::new(store),
            paths,
            started_at: time::now_iso(),
            renderer,
            exports: ExportGate::new(),
        }
    }

    /// Locks the store. A panic while the lock was held cannot leave the
    /// database half-written (SQLite rolls back unfinished transactions) and
    /// the store keeps no other state, so a poisoned lock is safe to reuse.
    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn renderer(&self) -> Option<&dyn CardRenderer> {
        self.renderer.as_deref()
    }

    // These operations are the shared native command service. The Tauri
    // adapters dispatch them to spawn_blocking; the console QA example calls
    // the same methods against a new, strictly temporary data directory.
    pub fn app_snapshot(&self, version: &str) -> CoreResult<AppSnapshot> {
        Ok(AppSnapshot {
            name: APP_NAME.to_owned(),
            version: version.to_owned(),
            data_dir: self.paths.data_dir().display().to_string(),
            counts: self.store().counts()?,
            integrations: integration_statuses(self.renderer().is_some()),
            started_at: self.started_at.clone(),
        })
    }

    pub fn itch_list(
        &self,
        query: Option<&str>,
        status: Option<&str>,
        project: Option<&str>,
    ) -> CoreResult<Vec<Itch>> {
        let filter = ListFilter::parse(query, status, project)?;
        self.store().list_itches(&filter)
    }

    pub fn itch_save(&self, input: &ItchInput) -> CoreResult<Itch> {
        self.store().save_itch(input)
    }

    pub fn itch_transition(&self, id: &str, status: &str) -> CoreResult<Itch> {
        self.store().transition_itch(id, status)
    }

    fn settings_view(&self, settings: Settings) -> SettingsView {
        SettingsView::new(
            settings,
            integration_statuses(self.renderer().is_some()),
            &self.paths.exports_dir(),
        )
    }

    pub fn settings_get(&self) -> CoreResult<SettingsView> {
        let settings = self.store().settings()?;
        Ok(self.settings_view(settings))
    }

    pub fn settings_save(&self, input: &SettingsInput) -> CoreResult<SettingsView> {
        let saved = self.store().save_settings(input)?;
        Ok(self.settings_view(saved))
    }

    pub fn card_export(&self, id: &str, format: &str) -> CoreResult<CardExport> {
        // Drop the database lock before acquiring the rendering permit and
        // doing CPU/file work, so reads and saves remain available during export.
        let job = ExportJob::prepare(&self.store(), id, format)?;
        let _slot = self.exports.try_enter()?;
        job.run(&self.paths, self.renderer())
    }

    pub fn legacy_api(&self, path: &str, payload: &Value) -> LegacyResult<Value> {
        legacy::validate_request(path, payload)?;
        if path == "/api/call" {
            let (name, args) = legacy::call_envelope(payload)?;
            if name == "manager.card_export" {
                // This optional local extension uses the existing verified
                // renderer, never the original social draft/image service.
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct ExportArgs {
                    id: String,
                    format: String,
                }
                let args: ExportArgs = serde_json::from_value(args).map_err(|_| {
                    LegacyError::invalid("Card export requires an id and gif/png format only.")
                })?;
                return serde_json::to_value(self.card_export(&args.id, &args.format)?)
                    .map_err(|_| LegacyError::invalid("Could not describe the exported card."));
            }
        }
        self.store().legacy_api(path, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestDir(PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        state: AppState,
        // Drop the state (and its SQLite connection) before this directory.
        _dir: TestDir,
    }

    impl Fixture {
        fn new(render: bool) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "dumbmanager-rust-legacy-command-test-{}-{}-{}",
                std::process::id(),
                time::now_millis(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let paths = AppPaths::new(root.join("io.dumbmanager.rust")).unwrap();
            let store = Store::open(&paths.database_path()).unwrap();
            Self {
                state: AppState::new(
                    paths,
                    store,
                    if render {
                        crate::card::renderer()
                    } else {
                        None
                    },
                ),
                _dir: TestDir(root),
            }
        }

        fn capture(&self) -> Value {
            self.state.legacy_api("/api/call", &json!({"name":"manager.capture",
                "args":{"title":"Keep the exact original page","project":"sample","request_key":"native-adapter-test"}})).unwrap()
        }
    }

    #[test]
    fn legacy_native_service_returns_raw_shapes_and_structured_errors() {
        let fx = Fixture::new(false);
        let row = fx.capture();
        assert_eq!(row["state"], "open");
        assert_eq!(row["version"], 1);
        let read = fx
            .state
            .legacy_api(
                "/api/call",
                &json!({"name":"manager.get","args":{"id":row["id"]}}),
            )
            .unwrap();
        assert_eq!(read, row);
        let error = fx
            .state
            .legacy_api(
                "/api/call",
                &json!({"name":"manager.card_export","args":{"id":row["id"],"format":"gif"}}),
            )
            .unwrap_err();
        assert_eq!(error.code, "unavailable");
        assert_eq!(error.status, 501);
        assert!(serde_json::to_value(error).unwrap()["error"].is_string());
        assert!(!fx.state.paths.exports_dir().exists());
        assert_eq!(fx.state.legacy_api("/api/call", &json!({"name":"manager.card_export","args":{"id":row["id"],"format":"png","path":"C:\\not-allowed.png"}})).unwrap_err().status,400);
    }

    #[test]
    fn original_capture_can_export_real_native_card_without_mutating_itch() {
        let fx = Fixture::new(true);
        let row = fx.capture();
        let card = fx
            .state
            .legacy_api(
                "/api/call",
                &json!({"name":"manager.card_export","args":{"id":row["id"],"format":"png"}}),
            )
            .unwrap();
        let path = PathBuf::from(card["path"].as_str().unwrap());
        assert_eq!(path.parent(), Some(fx.state.paths.exports_dir().as_path()));
        assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        assert!(card["width"].as_u64().unwrap() >= 1920);
        assert_eq!(card["frames"], 1);
        assert!(card["bytes"].as_u64().unwrap() > 100);
        assert_eq!(
            fx.state
                .legacy_api(
                    "/api/call",
                    &json!({"name":"manager.get","args":{"id":row["id"]}})
                )
                .unwrap(),
            row
        );
    }
}
