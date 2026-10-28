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
    pub runtime: crate::services::runtime::Runtime,
    pub previews: crate::preview::PreviewCache,
}

impl AppState {
    pub fn new(paths: AppPaths, store: Store, renderer: Option<Box<dyn CardRenderer>>) -> Self {
        let runtime = crate::services::runtime::Runtime::new(paths.data_dir().join("services"));
        Self {
            store: Mutex::new(store),
            paths,
            started_at: time::now_iso(),
            renderer,
            exports: ExportGate::new(),
            runtime,
            previews: crate::preview::PreviewCache::default(),
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
    // adapters dispatch them to spawn_blocking.
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
        let mut result = self.legacy_api_raw(path, payload)?;
        if path != "/api/call" {
            return Ok(result);
        }
        let name = payload.get("name").and_then(Value::as_str).unwrap_or("");
        if !matches!(name, "manager.state" | "manager.list" | "manager.get") {
            return Ok(result);
        }
        // Posting evidence lives in a separate service document. Join the current
        // saved snapshot on reads so list/detail badges survive migration, reload,
        // linking and unlinking without copying stale statuses into itch records.
        let posting = self.runtime.read_document("social-post-links.json")?;
        let annotate = |row: &mut Value| {
            if let Some(id) = row.get("id").and_then(Value::as_str) {
                let saved = posting.get(id).cloned().unwrap_or_else(|| {
                    serde_json::json!({"itch_id":id,"state":"unknown","posts":[],
                        "candidates":[],"candidate_count":0,"coverage":null,
                        "note":"No external account check has been performed."})
                });
                row["posting"] = saved;
            }
        };
        if name == "manager.get" {
            annotate(&mut result);
        } else {
            let field = if name == "manager.state" {
                "itches"
            } else {
                "items"
            };
            if let Some(rows) = result.get_mut(field).and_then(Value::as_array_mut) {
                rows.iter_mut().for_each(annotate);
            }
        }
        Ok(result)
    }

    fn legacy_api_raw(&self, path: &str, payload: &Value) -> LegacyResult<Value> {
        if matches!(path, "/api/upload" | "/api/itch-upload") {
            // Binary transport has a separate, narrow size limit. It cannot
            // enlarge arbitrary capability inputs or smuggle a filesystem path.
            let encoded = payload
                .get("base64")
                .and_then(Value::as_str)
                .ok_or_else(|| LegacyError::invalid("Expected an encoded upload."))?;
            if encoded.len() > 69_905_068 || !payload.is_object() {
                return Err(LegacyError {
                    error: "Attach a file up to 50 MiB.".into(),
                    code: "too_large".into(),
                    status: 413,
                });
            }
        } else if path == "/api/call"
            && payload.get("name").and_then(Value::as_str) == Some("manager.social.gif_store")
        {
            // Only the frozen-card renderer may submit a larger JSON body.
            // The service independently checks the one-shot nonce and GIF bytes.
            let args = payload
                .get("args")
                .filter(|v| v.is_object())
                .ok_or_else(|| LegacyError::invalid("Expected rendered card arguments."))?;
            let gif = args
                .get("gif")
                .and_then(Value::as_str)
                .ok_or_else(|| LegacyError::invalid("Expected an encoded card."))?;
            if gif.len() > 20_971_520
                || serde_json::to_vec(payload).map_or(true, |v| v.len() > 21_037_056)
            {
                return Err(LegacyError {
                    error: "Animated card exceeds 15 MiB.".into(),
                    code: "too_large".into(),
                    status: 413,
                });
            }
        } else {
            legacy::validate_request(path, payload)?;
        }
        if let Some(result) = crate::services::dispatch(self, path, payload) {
            if path == "/api/preview" {
                let mut result = result?;
                if let Some(html) = result
                    .get("html")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                {
                    result.as_object_mut().unwrap().remove("html");
                    result["url"] = Value::String(self.previews.insert(html)?);
                }
                return Ok(result);
            }
            return result;
        }
        if path == "/api/call" {
            let (name, mut args) = legacy::call_envelope(payload)?;
            if matches!(name, "manager.capture" | "manager.attach") {
                let replay_token = args
                    .as_object_mut()
                    .and_then(|a| a.remove("native_replay_token"))
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let ids = match args.get("attachments") {
                    None => serde_json::json!([]),
                    Some(Value::String(raw)) => serde_json::from_str(raw)
                        .map_err(|_| LegacyError::invalid("Expected uploaded evidence IDs."))?,
                    Some(value) => value.clone(),
                };
                let references = crate::services::media::references(self, &ids)?;
                let result = self
                    .store()
                    .legacy_call_with_media(name, &args, Some(&references))?;
                if name == "manager.capture" {
                    if let Some(id) = result["id"].as_str() {
                        crate::services::desktop::finish_replay(
                            id,
                            &replay_token,
                            args["request_key"].as_str().unwrap_or(""),
                        );
                    }
                }
                return Ok(result);
            }
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
