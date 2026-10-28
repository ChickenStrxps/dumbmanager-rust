//! Isolated local itch evidence. No caller-supplied paths are accepted.
use crate::state::AppState;
use base64::{engine::general_purpose::STANDARD, Engine};
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[path = "media/frameboard.rs"]
mod frameboard;

const DOC: &str = "media.json";
const MAX_FILE: usize = 50 * 1024 * 1024;
const MAX_IMAGE: usize = 10 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;

fn storage() -> LegacyError {
    LegacyError {
        error: "Could not access local evidence. Your draft was kept.".into(),
        code: "service_storage".into(),
        status: 500,
    }
}
fn too_large() -> LegacyError {
    LegacyError {
        error: "Evidence exceeds the local size limit.".into(),
        code: "too_large".into(),
        status: 413,
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn folder(root: &Path) -> LegacyResult<PathBuf> {
    fs::create_dir_all(root).map_err(|_| storage())?;
    let dir = root.join("itch-media");
    match fs::create_dir(&dir) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(storage()),
    }
    let meta = fs::symlink_metadata(&dir).map_err(|_| storage())?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || fs::canonicalize(&dir).map_err(|_| storage())?.parent()
            != Some(fs::canonicalize(root).map_err(|_| storage())?.as_path())
    {
        return Err(LegacyError::invalid("Invalid evidence storage directory."));
    }
    Ok(dir)
}
fn content(root: &Path, id: &str) -> LegacyResult<PathBuf> {
    if !valid_id(id) {
        return Err(LegacyError::invalid("Invalid evidence ID."));
    }
    let path = folder(root)?.join(id);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink() || !m.is_file()) {
        return Err(LegacyError::invalid("Invalid evidence file."));
    }
    Ok(path)
}
fn safe_name(raw: &str) -> String {
    let name: String = raw
        .chars()
        .take(220)
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    if name.trim().is_empty() {
        "evidence".into()
    } else {
        name.trim().into()
    }
}
fn upload(state: &AppState, p: &Value) -> LegacyResult<Value> {
    let encoded = p
        .get("base64")
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyError::invalid("Missing evidence bytes."))?;
    if encoded.len() > MAX_FILE.div_ceil(3) * 4 {
        return Err(too_large());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| LegacyError::invalid("Invalid evidence bytes."))?;
    if bytes.is_empty() || bytes.len() > MAX_FILE {
        return Err(too_large());
    }
    let name = safe_name(p["filename"].as_str().unwrap_or(""));
    let mime = p["mime_type"]
        .as_str()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let image = matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    );
    let video = mime.strip_prefix("video/").is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-_".contains(&b))
    });
    if !image && !video {
        return Err(LegacyError::invalid(
            "Stash supports PNG, JPEG, WebP, GIF and videos.",
        ));
    }
    if image && bytes.len() > MAX_IMAGE {
        return Err(too_large());
    }
    let wants_board = match p["video_mode"].as_str().unwrap_or("") {
        "" | "original" => false,
        "frameboard" => video,
        _ => return Err(LegacyError::invalid("Choose original video or frameboard.")),
    };
    let converted = if wants_board {
        Some(frameboard::convert(state, &bytes)?)
    } else {
        None
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = content(state.runtime.root(), &id)?;
    let original_path = folder(state.runtime.root())?.join(format!("{id}-original"));
    let (saved, output_name, output_mime, note, thumbnail, duration) = if let Some((
        board,
        duration,
    )) = converted
    {
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video");
        let output_name = format!(
            "{}-frameboard-{duration:.1}s-no-audio.jpg",
            stem.chars().take(150).collect::<String>()
        );
        let thumbnail = Some(STANDARD.encode(&board));
        (board,output_name,"image/jpeg".to_string(),format!("9 sampled frames across {duration:.1}s ? timestamps ? no audio. Original video remains local."),thumbnail,Some(duration))
    } else {
        (
            bytes.clone(),
            name.clone(),
            mime.clone(),
            if video {
                "Original video saved locally; playback depends on your browser's codecs"
            } else {
                "Saved locally with this itch"
            }
            .to_string(),
            None,
            None,
        )
    };
    let row = json!({"id":id,"type":if image || wants_board {"image"} else {"video"},"name":output_name,"mimeType":output_mime,"sizeBytes":saved.len(),"note":note,"frameboard":if wants_board {json!({"duration":duration,"frames":9,"original_name":name,"original_mime":mime,"original_size":bytes.len()})} else {Value::Null}});
    let update = state.runtime.update_document(DOC, |doc| {
        let rows = doc.as_object_mut().ok_or_else(storage)?;
        let total: u64 = rows
            .values()
            .map(|v| {
                v["sizeBytes"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(v["frameboard"]["original_size"].as_u64().unwrap_or(0))
            })
            .sum();
        let extra = saved.len() as u64 + if wants_board { bytes.len() as u64 } else { 0 };
        if total.saturating_add(extra) > MAX_TOTAL {
            return Err(too_large());
        }
        let write = |path: &Path, data: &[u8]| -> LegacyResult<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| storage())?;
            f.write_all(data)
                .and_then(|()| f.sync_all())
                .map_err(|_| storage())
        };
        if wants_board {
            write(&original_path, &bytes)?;
        }
        write(&path, &saved)?;
        rows.insert(id.clone(), row.clone());
        Ok(())
    });
    if let Err(e) = update {
        let _ = fs::remove_file(path);
        if wants_board {
            let _ = fs::remove_file(original_path);
        }
        return Err(e);
    }

    Ok(json!({"attachment":row,"note":note,"thumbnail":thumbnail}))
}
/// Convert an uploaded video using only explicitly configured local tools.
pub fn video_frameboard(state: &AppState, bytes: &[u8]) -> LegacyResult<(Vec<u8>, f64)> {
    if bytes.is_empty() || bytes.len() > MAX_FILE {
        return Err(too_large());
    }
    frameboard::convert(state, bytes)
}
/// Validate evidence metadata and file presence before the core persists it.
pub fn references(state: &AppState, attachments: &Value) -> LegacyResult<Vec<Value>> {
    let input = attachments
        .as_array()
        .ok_or_else(|| LegacyError::invalid("Expected evidence attachments."))?;
    if input.len() > 8 {
        return Err(LegacyError::invalid(
            "Attach up to eight different evidence IDs.",
        ));
    }
    let doc = state.runtime.read_document(DOC)?;
    let mut ids = std::collections::HashSet::new();
    let mut rows = Vec::new();
    let mut total = 0u64;
    for item in input {
        let id = item
            .as_str()
            .or_else(|| item.get("id").and_then(Value::as_str))
            .ok_or_else(|| LegacyError::invalid("Invalid evidence ID."))?;
        if !valid_id(id) || !ids.insert(id.to_owned()) {
            return Err(LegacyError::invalid(
                "Attach up to eight different evidence IDs.",
            ));
        }
        let row = doc
            .get(id)
            .ok_or_else(|| LegacyError::not_found("Evidence is missing. Attach it again."))?;
        let path = content(state.runtime.root(), id)?;
        let size = row["sizeBytes"].as_u64().ok_or_else(storage)?;
        if fs::metadata(path)
            .map_err(|_| LegacyError::not_found("Evidence is missing. Attach it again."))?
            .len()
            != size
        {
            return Err(LegacyError::not_found(
                "Evidence is missing or incomplete. Attach it again.",
            ));
        }
        total = total.saturating_add(size);
        rows.push(row.clone());
    }
    if total > 80 * 1024 * 1024 {
        return Err(too_large());
    }
    Ok(rows)
}
pub fn read_attachment(state: &AppState, id: &str) -> LegacyResult<Value> {
    let rows = references(state, &json!([id]))?;
    let row = &rows[0];
    let mut data = Vec::new();
    fs::File::open(content(state.runtime.root(), id)?)
        .map_err(|_| storage())?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| storage())?;
    if data.len() as u64 != row["sizeBytes"].as_u64().unwrap_or(u64::MAX) {
        return Err(storage());
    }
    Ok(
        json!({"__native_binary":true,"base64":STANDARD.encode(data),"mime_type":row["mimeType"],"status":200}),
    )
}
pub fn read_original(state: &AppState, id: &str) -> LegacyResult<Value> {
    let rows = references(state, &json!([id]))?;
    let row = &rows[0];
    let expected = row["frameboard"]["original_size"]
        .as_u64()
        .ok_or_else(|| LegacyError::not_found("This evidence has no stored original video."))?;
    let path = folder(state.runtime.root())?.join(format!("{id}-original"));
    let meta = fs::symlink_metadata(&path)
        .map_err(|_| LegacyError::not_found("The original video is missing."))?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() != expected
        || expected > MAX_FILE as u64
    {
        return Err(storage());
    }
    let data = fs::read(path).map_err(|_| storage())?;
    Ok(
        json!({"__native_binary":true,"base64":STANDARD.encode(data),"mime_type":row["frameboard"]["original_mime"],"status":200}),
    )
}
pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    match path {
        "/api/call" => {
            let (name, args) = match dumbmanager_core::legacy::call_envelope(payload) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            match name {
                "manager.media.frameboard_settings" => Some(frameboard::settings(state)),
                "manager.media.frameboard_configure" => Some(frameboard::configure(state, &args)),
                _ => None,
            }
        }
        "/api/itch-upload" => Some(upload(state, payload)),
        "/api/itch-media" => Some(
            payload
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| LegacyError::invalid("Missing evidence ID."))
                .and_then(|id| {
                    if payload["original"] == true {
                        read_original(state, id)
                    } else {
                        read_attachment(state, id)
                    }
                }),
        ),
        _ => None,
    }
}
