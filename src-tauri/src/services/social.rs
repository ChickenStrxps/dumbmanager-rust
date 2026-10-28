//! Local completion drafts and actual native card rendering. No automatic posts.
use crate::state::AppState;
use base64::{engine::general_purpose::STANDARD, Engine};
use dumbmanager_core::{
    legacy::{self, LegacyError, LegacyResult},
    time,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[path = "media/browser_gif.rs"]
mod browser_gif;
#[path = "media/posts.rs"]
mod posts;
#[path = "media/private_image.rs"]
mod private_image;
#[path = "media/static_card.rs"]
mod static_card;
#[path = "media/webhook.rs"]
mod webhook;

const DRAFTS: &str = "social-drafts.json";
fn text<'a>(v: &'a Value, k: &str, max: usize) -> LegacyResult<&'a str> {
    let s = v
        .get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| LegacyError::invalid("Missing social field."))?;
    if s.chars().count() > max
        || s.chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(LegacyError::invalid("Social field is too long or invalid."));
    }
    Ok(s)
}
fn version(v: &Value) -> LegacyResult<u64> {
    v.get("expected_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| LegacyError::invalid("Expected a draft version."))
}
fn itch(state: &AppState, id: &str) -> LegacyResult<Value> {
    state.store().legacy_call("manager.get", &json!({"id":id}))
}
fn stage(v: &Value) -> &'static str {
    if v["state"] == "ready" {
        "review"
    } else if v["state"] == "resolved"
        && v["outcomes"]
            .as_array()
            .is_some_and(|a| a.iter().any(|o| o["acceptance"] == "user_confirmed"))
    {
        "accepted"
    } else {
        "changed"
    }
}
fn refresh(state: &AppState, row: &Value, source: &Value) -> LegacyResult<Value> {
    let mut row = row.clone();
    row["stage"] = json!(stage(source));
    row["source_changed"] = json!(row["source_version"] != source["version"]);
    row["posting"] = posts::for_itch(state, source["id"].as_str().unwrap_or(""))?;
    row["image"] = private_image::public(&row["image"]);
    Ok(row)
}
fn get(state: &AppState, id: &str) -> LegacyResult<(Value, Value)> {
    if id.len() > 100 {
        return Err(LegacyError::invalid("Invalid draft ID."));
    }
    let all = state.runtime.read_document(DRAFTS)?;
    let row = all
        .get(id)
        .cloned()
        .ok_or_else(|| LegacyError::not_found("Draft not found."))?;
    let source = itch(state, row["itch_id"].as_str().unwrap_or(""))?;
    Ok((row, source))
}
fn eligible(source: &Value) -> LegacyResult<()> {
    if stage(source) == "changed" {
        Err(LegacyError::invalid(
            "Choose an accepted W or an itch ready for review.",
        ))
    } else {
        Ok(())
    }
}
fn list(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let project = args.get("project").and_then(Value::as_str).unwrap_or("");
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(12);
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0);
    if project.chars().count() > 100
        || query.chars().count() > 500
        || !(1..=50).contains(&limit)
        || offset > 100000
    {
        return Err(LegacyError::invalid("Invalid social list filter."));
    }
    let mut items = Vec::new();
    let mut at = 0;
    loop {
        let page = state.store().legacy_call(
            "manager.list",
            &json!({"project":project,"limit":100,"offset":at}),
        )?;
        let total = page["total"].as_u64().unwrap_or(0);
        if total > 2000 {
            return Err(LegacyError::unavailable(
                "Social list cannot yet enumerate more than 2,000 local itches.",
            ));
        }
        for row in page["items"].as_array().into_iter().flatten() {
            let s = stage(row);
            let hay = format!(
                "{} {}",
                row["title"].as_str().unwrap_or(""),
                row["id"].as_str().unwrap_or("")
            )
            .to_lowercase();
            if s != "changed" && query.split_whitespace().all(|w| hay.contains(w)) {
                items.push(json!({"id":row["id"],"title":row["title"],"project":row["project"],"stage":s,"updated_at":row["updated_at"],"posting":posts::for_itch(state, row["id"].as_str().unwrap_or(""))?}));
            }
        }
        at += 100;
        if at >= total {
            break;
        }
    }
    let total = items.len();
    let page: Vec<_> = items
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    Ok(
        json!({"items":page,"total":total,"next_offset":if offset+limit<total as u64 {Some(offset+limit)} else {None}}),
    )
}
fn draft(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "itch_id", 100)?;
    let source = itch(state, id)?;
    eligible(&source)?;
    let digest = format!("{:x}", Sha256::digest(id.as_bytes()));
    let key = format!("post-{}", &digest[..28]);
    let now = time::now_iso();
    let headline: String = source["title"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(120)
        .collect();
    let summary = if stage(&source) == "accepted" {
        source["outcomes"]
            .as_array()
            .and_then(|a| a.iter().rev().find(|o| o["acceptance"] == "user_confirmed"))
            .and_then(|o| o["summary"].as_str())
            .unwrap_or("")
    } else {
        ""
    };
    let summary: String = if summary.is_empty() {
        "Ready for review. Testing before calling it a W.".into()
    } else {
        summary.chars().take(600).collect()
    };
    let prefix = if stage(&source) == "accepted" {
        "One less dumb thing."
    } else {
        "Ready to test."
    };
    let row = json!({"id":key,"itch_id":id,"project":source["project"],"source_version":source["version"],"headline":headline,"summary":summary,"caption":format!("{prefix}\n\n{headline}\n\n{summary}"),"version":1,"created_at":now,"updated_at":now});
    let doc = state.runtime.update_document(DRAFTS, |doc| {
        let map = doc
            .as_object_mut()
            .ok_or_else(|| LegacyError::invalid("Unreadable draft storage."))?;
        map.entry(key.clone()).or_insert(row);
        Ok(())
    })?;
    refresh(state, &doc[&key], &source)
}
fn save(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let expected = version(args)?;
    let headline = text(args, "headline", 120)?.trim();
    let summary = text(args, "summary", 600)?.trim();
    let caption = text(args, "caption", 2000)?.trim();
    if headline.is_empty() || caption.is_empty() {
        return Err(LegacyError::invalid("Add a headline and caption."));
    }
    let (_, source) = get(state, id)?;
    eligible(&source)?;
    let doc = state.runtime.update_document(DRAFTS, |doc| {
        let row = doc
            .get_mut(id)
            .ok_or_else(|| LegacyError::not_found("Draft not found."))?;
        let changed =
            row["headline"] != headline || row["summary"] != summary || row["caption"] != caption;
        if row["version"].as_u64() != Some(expected) && changed {
            return Err(LegacyError::conflict(
                "This draft changed in another window. Reload before saving.",
            ));
        }
        if changed || row["source_version"] != source["version"] {
            row["headline"] = json!(headline);
            row["summary"] = json!(summary);
            row["caption"] = json!(caption);
            row["copy_edited"] = json!(changed || row["copy_edited"] == true);
            row["source_version"] = source["version"].clone();
            row["version"] = json!(row["version"].as_u64().unwrap_or(0) + 1);
            row["updated_at"] = json!(time::now_iso());
        }
        Ok(())
    })?;
    refresh(state, &doc[id], &source)
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn svg(row: &Value, image: &str) -> String {
    static_card::card(row, image)
}
fn preview(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    let image_uri = private_image::uri(state, &row["image"])?;
    let row = refresh(state, &row, &source)?;
    let caption = row["caption"].as_str().unwrap_or("");
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("text", caption)
        .finish();
    Ok(
        json!({"draft":row,"svg":svg(&row, &image_uri),"export_ready":true,"filename":format!("dumbmanager-{}.png",source["id"].as_str().unwrap_or("").chars().take(8).collect::<String>()),"composers":{"x":format!("https://twitter.com/intent/tweet?{query}"),"bluesky":format!("https://bsky.app/intent/compose?{query}")},"note":"Local draft only. Nothing is marked posted."}),
    )
}

fn gif(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    if row["source_version"] != source["version"] {
        return Err(LegacyError::conflict(
            "The itch changed. Update the preview before exporting.",
        ));
    }
    let theme = args.get("theme").and_then(Value::as_str).unwrap_or("");
    let events = args.get("events").and_then(Value::as_str).unwrap_or("");
    if !matches!(theme, "" | "flow") || !events.is_empty() {
        return Err(LegacyError::unavailable(
            "That animation style or event is not available in the native renderer.",
        ));
    }
    let vibe = args.get("vibe").and_then(Value::as_u64).unwrap_or(0);
    if vibe > 1_000_000 {
        return Err(LegacyError::invalid("Invalid card vibe."));
    }
    let seed = format!("{id}:{vibe}");
    let input = dumbmanager_card_renderer::CardInput {
        title: row["headline"].as_str().unwrap_or(""),
        description: row["summary"].as_str().unwrap_or(""),
        project: row["project"].as_str().unwrap_or(""),
        status: source["state"].as_str().unwrap_or(""),
        seed: &seed,
    };
    let _permit = state.exports.try_enter().map_err(LegacyError::from)?;
    let dir = state.runtime.root().join("social-render");
    std::fs::create_dir_all(&dir)
        .map_err(|_| LegacyError::unavailable("Cannot create local card render."))?;
    let meta = std::fs::symlink_metadata(&dir)
        .map_err(|_| LegacyError::unavailable("Cannot inspect local card render folder."))?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || std::fs::canonicalize(&dir)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            != std::fs::canonicalize(state.runtime.root()).ok()
    {
        return Err(LegacyError::invalid("Invalid local card render folder."));
    }
    let result = dumbmanager_card_renderer::export_card(&dir, &input, "gif")
        .map_err(|_| LegacyError::unavailable("Native card render failed."))?;
    let data = std::fs::read(&result.path);
    let _ = std::fs::remove_file(&result.path);
    let data = data.map_err(|_| LegacyError::unavailable("Cannot read local card render."))?;
    if data.len() > 15 * 1024 * 1024 {
        return Err(LegacyError::invalid("Animated card is larger than 15 MiB."));
    }
    let digest = format!("{:x}", Sha256::digest(&data));
    Ok(
        json!({"draft_id":id,"version":row["version"],"source_version":row["source_version"],"stage":stage(&source),"digest":null,"export_ready":true,"vibe":vibe,"requested_theme":theme,"theme":"Native water flow","key":"flow","events":[],"frames":result.frames,"seconds":result.frames as f64 * 0.05,"width":result.width,"height":result.height,"bytes":data.len(),"sha256":digest,"filename":format!("dumbmanager-{}.gif",source["id"].as_str().unwrap_or("").chars().take(8).collect::<String>()),"gif":STANDARD.encode(data)}),
    )
}
// The file-drop clipboard references the file after this function returns.
// Serialize copies so an older request cannot remove a newer clipboard file.
static COPY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn write_clipboard_gif(
    root: &std::path::Path,
    data: &[u8],
    digest: &str,
) -> LegacyResult<std::path::PathBuf> {
    use std::io::Write;
    if data.len() > 15 * 1024 * 1024
        || !data.starts_with(b"GIF89a")
        || !data.ends_with(b";")
        || format!("{:x}", Sha256::digest(data)) != digest
    {
        return Err(LegacyError::invalid("The rendered GIF is invalid."));
    }
    let dir = root.join("social-render");
    std::fs::create_dir_all(&dir)
        .map_err(|_| LegacyError::unavailable("Cannot create local card render."))?;
    let meta = std::fs::symlink_metadata(&dir)
        .map_err(|_| LegacyError::unavailable("Cannot inspect card folder."))?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || std::fs::canonicalize(&dir)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            != std::fs::canonicalize(root).ok()
    {
        return Err(LegacyError::invalid("Invalid local card render folder."));
    }
    let path = dir.join(format!("clipboard-{}.gif", uuid::Uuid::new_v4().simple()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| LegacyError::unavailable("Cannot save clipboard GIF."))?;
    if file.write_all(data).and_then(|_| file.sync_all()).is_err() {
        let _ = std::fs::remove_file(&path);
        return Err(LegacyError::unavailable("Cannot save clipboard GIF."));
    }
    drop(file);
    let actual = std::fs::read(&path)
        .map_err(|_| LegacyError::unavailable("Cannot verify clipboard GIF."))?;
    if actual != data {
        let _ = std::fs::remove_file(&path);
        return Err(LegacyError::unavailable("Cannot verify clipboard GIF."));
    }
    std::fs::canonicalize(&path)
        .map_err(|_| LegacyError::unavailable("Cannot resolve clipboard GIF."))
}
fn gif_copy(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let _guard = COPY_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let expected = version(args)?;
    let id = text(args, "id", 100)?;
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    if row["version"].as_u64() != Some(expected) || row["source_version"] != source["version"] {
        return Err(LegacyError::conflict(
            "This draft or itch changed. Reopen the preview.",
        ));
    }
    let sha = text(args, "sha256", 64)?;
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LegacyError::invalid("Use the exact preview GIF digest."));
    }
    let rendered = browser_gif::cached(state, args, Some(sha))?;
    let (latest, source) = get(state, id)?;
    if latest["version"].as_u64() != Some(expected) || latest["source_version"] != source["version"]
    {
        return Err(LegacyError::conflict(
            "This draft or itch changed. Reopen the preview.",
        ));
    }
    let data = STANDARD
        .decode(rendered["gif"].as_str().unwrap_or(""))
        .map_err(|_| LegacyError::unavailable("Cannot decode rendered GIF."))?;
    let path = write_clipboard_gif(
        state.runtime.root(),
        &data,
        rendered["sha256"].as_str().unwrap_or(""),
    )?;
    if let Err(err) = crate::platform::copy_export_file(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(err.into());
    }
    // Remove only earlier files created by this operation, after a successful handoff.
    if let Ok(entries) = std::fs::read_dir(path.parent().unwrap()) {
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if p != path
                && name.starts_with("clipboard-")
                && name.ends_with(".gif")
                && entry
                    .file_type()
                    .is_ok_and(|t| t.is_file() && !t.is_symlink())
            {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(
        json!({"copied":true,"format":"image/gif file","filename":rendered["filename"],"bytes":rendered["bytes"],"sha256":rendered["sha256"],"version":rendered["version"],"theme":rendered["theme"],"events":rendered["events"]}),
    )
}
pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    if path != "/api/call" {
        return None;
    }
    let (name, args) = match legacy::call_envelope(payload) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    match name {
        "manager.social.list" => Some(list(state, &args)),
        "manager.social.draft" => Some(draft(state, &args)),
        "manager.social.save" => Some(save(state, &args)),
        "manager.social.preview" => Some(preview(state, &args)),
        "manager.social.gif" => Some(gif(state, &args)),
        "manager.social.gif_prepare" => Some(browser_gif::prepare(state, &args)),
        "manager.social.gif_store" => Some(browser_gif::store(state, &args)),
        "manager.social.gif_copy" => Some(gif_copy(state, &args)),
        "manager.social.image" => Some(private_image::set(state, &args)),
        "manager.social.image_review" => Some(private_image::review(state, &args)),
        "manager.social.check" => Some(posts::check(state, &args)),
        "manager.social.link" => Some(posts::link(state, &args)),
        "manager.social.unlink" => Some(posts::unlink(state, &args)),
        "manager.social.webhook_settings"
        | "manager.social.webhook_configure"
        | "manager.social.webhook_preview"
        | "manager.social.webhook_send"
        | "manager.social.webhook_status" => Some(webhook::call(state, name, &args)),
        _ => None,
    }
}
