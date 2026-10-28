//! Opt-in desktop actions. Nothing registers a key or captures on startup.
use crate::state::AppState;
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use tauri::AppHandle;
#[path = "desktop/capture.rs"]
mod capture;
#[path = "desktop/replay.rs"]
mod replay;
#[path = "desktop/shortcuts.rs"]
mod shortcuts;
#[path = "desktop/snip.rs"]
mod snip;
#[path = "desktop/windows.rs"]
mod windows;
pub fn install(app: &AppHandle) {
    shortcuts::install(app)
}
/// Call only after manager.capture has successfully returned its itch ID.
pub fn finish_replay(id: &str, token: &str, request_key: &str) {
    capture::finish(id, token, request_key);
}
fn action(p: &Value) -> LegacyResult<&str> {
    p.get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyError::invalid("Choose an action."))
}
fn id(p: &Value) -> LegacyResult<String> {
    p.get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyError::invalid("Missing replay id."))
        .and_then(|id| {
            uuid::Uuid::parse_str(id)
                .map(|u| u.simple().to_string())
                .map_err(|_| LegacyError::invalid("Invalid replay id."))
        })
}
fn native(state: &AppState, p: &Value) -> LegacyResult<Value> {
    match action(p)? {
        "status" => Ok(shortcuts::status()),
        "start" => shortcuts::start(),
        "test" => shortcuts::test(),
        "snip" => capture_region(state, p),
        "capture" | "moment" => Err(LegacyError::unavailable(
            "Use the capture controls in the app window.",
        )),
        _ => Err(LegacyError::invalid("Unknown shortcut action.")),
    }
}
fn capture_region(state: &AppState, p: &Value) -> LegacyResult<Value> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let id = p.get("itch_id").and_then(Value::as_str).unwrap_or("");
    if !id.is_empty() {
        let current =
            state.legacy_api("/api/call", &json!({"name":"manager.get","args":{"id":id}}))?;
        if current["attachments"]
            .as_array()
            .is_some_and(|a| a.len() >= 8)
        {
            return Err(LegacyError::invalid(
                "This itch already has eight receipts. Remove one before snipping.",
            ));
        }
    }
    let Some(bytes) = snip::select()? else {
        let mut result = shortcuts::status();
        result["snip"] = json!({"cancelled":true});
        return Ok(result);
    };
    let name = format!("snip-{}.png", uuid::Uuid::new_v4().simple());
    let saved = super::media::handle(
        state,
        "/api/itch-upload",
        &json!({"filename":name,"mime_type":"image/png","base64":STANDARD.encode(bytes)}),
    )
    .ok_or_else(|| LegacyError::unavailable("Evidence storage is unavailable."))??;
    let attachment = saved["attachment"].clone();
    let mut result = shortcuts::status();
    let mut snip = json!({"cancelled":false,"attachment":attachment,"itch_id":id,"attached":false});
    if !id.is_empty() {
        match state.legacy_api(
            "/api/call",
            &json!({"name":"manager.attach","args":{"id":id,"attachments":[attachment["id"]]}}),
        ) {
            Ok(_) => snip["attached"] = json!(true),
            Err(e) => snip["error"] = json!(e.error),
        }
    }
    result["snip"] = snip;
    Ok(result)
}
fn replay_api(state: &AppState, p: &Value) -> LegacyResult<Value> {
    let root = state.runtime.root();
    match action(p)? {
        "status" => Ok(capture::status()),
        "begin" => Ok(capture::begin(p["request_key"].as_str().unwrap_or(""))),
        "cancel" => Ok(capture::cancel(p["request_key"].as_str().unwrap_or(""))),
        "disarm" => Ok(capture::disarm()),
        "windows" => Ok(json!({"items":windows::windows(),"note":"Select a visible test window."})),
        "arm" => capture::arm(
            root.to_path_buf(),
            p.get("hwnd").and_then(Value::as_str).unwrap_or(""),
        ),
        "item" => {
            let id = id(p)?;
            if capture::pending(&id) {
                Ok(json!({"available":false,"pending":true}))
            } else {
                replay::item(root, &id)
            }
        }
        "frame" => {
            let n = p
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| LegacyError::invalid("Choose a frame index."))?;
            replay::frame(root, &id(p)?, n)
        }
        "keep" => {
            let keep = p
                .get("keep")
                .and_then(Value::as_bool)
                .ok_or_else(|| LegacyError::invalid("Choose whether to keep this replay."))?;
            replay::keep(root, &id(p)?, keep)
        }
        "delete" => replay::delete(root, &id(p)?),
        _ => Err(LegacyError::invalid("Unknown replay action.")),
    }
}
pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    match path {
        "/api/native" => Some(native(state, payload)),
        "/api/replay" => Some(replay_api(state, payload)),
        _ => None,
    }
}
