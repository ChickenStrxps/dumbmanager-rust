//! Explicitly configured services belonging to this installation only.
pub mod ai;
pub mod browser;
pub mod companion;
pub mod desktop;
pub mod git;
pub mod media;
pub mod process;
pub mod runtime;
pub mod social;
pub mod t3;
pub mod voice;

use dumbmanager_core::legacy::LegacyResult;
use serde_json::Value;

pub fn dispatch(
    state: &crate::state::AppState,
    path: &str,
    payload: &Value,
) -> Option<LegacyResult<Value>> {
    // Pick the owner before parsing/cloning arguments. In particular, a large
    // rendered GIF must not be copied by every unrelated service in the chain.
    match path {
        "/api/cook" | "/api/upload" => t3::handle(state, path, payload),
        "/api/itch-upload" | "/api/itch-media" => media::handle(state, path, payload),
        "/api/native" | "/api/replay" => desktop::handle(state, path, payload),
        "/api/preview" => {
            if payload.as_object().is_none_or(|o| {
                o.keys()
                    .any(|k| !matches!(k.as_str(), "thread_id" | "attachment_id" | "t3_target"))
            }) {
                return Some(Err(dumbmanager_core::legacy::LegacyError::invalid(
                    "Unsupported preview field.",
                )));
            }
            t3::handle(
                state,
                "/api/call",
                &serde_json::json!({"name":"t3.preview","args":{
                "thread_id":payload["thread_id"],"attachment_id":payload["attachment_id"]},
                "t3_target":payload["t3_target"].as_str().unwrap_or("")}),
            )
        }
        "/api/call" => match payload.get("name").and_then(Value::as_str).unwrap_or("") {
            name if name.starts_with("t3.")
                || name.starts_with("projects.")
                || name == "manager.send" =>
            {
                t3::handle(state, path, payload)
            }
            name if name.starts_with("browser.") => browser::handle(state, path, payload),
            name if name.starts_with("ai.") => voice::handle(state, path, payload),
            name if name.starts_with("manager.media.") => media::handle(state, path, payload),
            name if name.starts_with("manager.social.") => social::handle(state, path, payload),
            name if name.starts_with("git.") => git::handle(state, path, payload),
            name if name.starts_with("companion.") || name.starts_with("bridge.") => {
                companion::handle(state, path, payload)
            }
            _ => ai::handle(state, path, payload),
        },
        _ => None,
    }
}
