//! Browser renderer broker: short-lived one-shot permits and validated app-owned GIFs.
use super::*;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
const DOCUMENT: &str = "social-browser-gifs.json";
const LIMIT: usize = 15 * 1024 * 1024;
const CACHE_LIMIT: u64 = 96 * 1024 * 1024;
const THEMES: &[(&str, &str)] = &[
    ("dumb", "DumbManager"),
    ("windows", "Windows"),
    ("mac", "Mac"),
    ("nvidia", "NVIDIA"),
    ("t3", "T3 Code"),
    ("dog", "Dog park"),
    ("cat", "Cat night"),
    ("fish", "Aquarium"),
    ("shark", "Shark week"),
    ("penguin", "Ice shelf"),
    ("ohmypi", "Oh My Pi"),
    ("chill", "Lofi chill"),
    ("nature", "Forest"),
    ("beach", "Beach day"),
    ("desert", "Desert"),
    ("flow", "Water flow"),
    ("smoke", "Dithered smoke"),
];
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn directory(root: &Path) -> LegacyResult<PathBuf> {
    fs::create_dir_all(root)
        .map_err(|_| LegacyError::unavailable("Cannot create card storage."))?;
    let dir = root.join("social-render");
    fs::create_dir_all(&dir)
        .map_err(|_| LegacyError::unavailable("Cannot create card storage."))?;
    let meta = fs::symlink_metadata(&dir)
        .map_err(|_| LegacyError::unavailable("Cannot inspect card storage."))?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || fs::canonicalize(&dir)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            != fs::canonicalize(root).ok()
    {
        return Err(LegacyError::invalid("Invalid card storage folder."));
    }
    Ok(dir)
}
fn remove_evicted(state: &AppState, ids: &[String]) {
    if let Ok(dir) = directory(state.runtime.root()) {
        for id in ids {
            if id.len() != 32
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                continue;
            }
            let path = dir.join(format!("browser-{id}.gif"));
            if fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}
fn variant(args: &Value) -> LegacyResult<(u64, String, String)> {
    let vibe = args.get("vibe").and_then(Value::as_u64).unwrap_or(0);
    if vibe > 1_000_000 {
        return Err(LegacyError::invalid("Invalid card vibe."));
    }
    let theme = args.get("theme").and_then(Value::as_str).unwrap_or("");
    if !theme.is_empty() && !THEMES.iter().any(|(key, _)| *key == theme) {
        return Err(LegacyError::invalid("Unknown card theme."));
    }
    let events = args.get("events").and_then(Value::as_str).unwrap_or("");
    let mut seen = std::collections::HashSet::new();
    if events.len() > 100
        || events
            .split(',')
            .filter(|x| !x.is_empty())
            .any(|x| !matches!(x, "greg" | "flicker" | "lightning") || !seen.insert(x))
    {
        return Err(LegacyError::invalid("Unknown card event."));
    }
    Ok((vibe, theme.to_owned(), events.to_owned()))
}
pub(super) fn prepare(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    if row["source_version"] != source["version"] {
        return Err(LegacyError::conflict(
            "The itch changed. Update the preview before exporting.",
        ));
    }
    let (vibe, theme, events) = variant(args)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let expires = now() + 600;
    let payload = json!({"id":id,"headline":row["headline"],"summary":row["summary"],"stage":stage(&source),"project":row["project"],"image":private_image::uri(state,&row["image"])?});
    let mut force = json!({});
    if !theme.is_empty() {
        force["theme"] = json!(theme);
    }
    if !events.is_empty() {
        force["events"] = json!(events
            .split(',')
            .filter(|e| !e.is_empty())
            .collect::<Vec<_>>());
    }
    let mut evicted = Vec::new();
    state.runtime.update_document(DOCUMENT,|doc|{
        let map=doc.as_object_mut().ok_or_else(||LegacyError::invalid("Unreadable card storage."))?;
        map.retain(|_,v| v["state"]=="ready" || v["expires"].as_u64().unwrap_or(0)>now());
        while map.len()>=1024 {
            let oldest=map.iter().filter(|(_,v)|v["state"]=="ready").min_by_key(|(_,v)|v["stored_at"].as_u64().unwrap_or(0)).map(|(k,_)|k.clone()).ok_or_else(||LegacyError::unavailable("Too many card renders are pending."))?;
            map.remove(&oldest);evicted.push(oldest);
        }
        map.insert(nonce.clone(),json!({"state":"prepared","expires":expires,"id":id,"version":row["version"],"source_version":row["source_version"],"vibe":vibe,"requested_theme":theme,"requested_events":events}));Ok(())
    })?;
    remove_evicted(state, &evicted);
    Ok(json!({"nonce":nonce,"expires_at":expires,"payload":payload,"vibe":vibe,"force":force}))
}
fn metadata(
    meta: &Value,
    permit: &Value,
    info: &dumbmanager_card_renderer::GifInfo,
) -> LegacyResult<()> {
    let key = meta["key"].as_str().unwrap_or("");
    let name = THEMES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .ok_or_else(|| LegacyError::invalid("Invalid card theme metadata."))?;
    let selected = permit["requested_theme"].as_str().unwrap_or("");
    if (!selected.is_empty() && selected != key)
        || meta["theme"] != name
        || meta["width"] != 1200
        || meta["height"] != 675
    {
        return Err(LegacyError::invalid(
            "Card metadata does not match its render permit.",
        ));
    }
    let frames = if matches!(key, "flow" | "smoke" | "chill") {
        96
    } else {
        72
    };
    let delay = if frames == 96 { 6 } else { 5 };
    if meta["frames"].as_u64() != Some(frames)
        || info.centiseconds != frames * delay
        || info.frames > frames as usize
        || meta["seconds"]
            .as_f64()
            .is_none_or(|s| (s - (frames * delay) as f64 / 100.0).abs() > 0.001)
    {
        return Err(LegacyError::invalid("Invalid card animation timing."));
    }
    let events = meta["events"]
        .as_array()
        .ok_or_else(|| LegacyError::invalid("Invalid card events."))?;
    let mut seen = std::collections::HashSet::new();
    for event in events {
        let e = event
            .as_str()
            .ok_or_else(|| LegacyError::invalid("Invalid card events."))?;
        if !matches!(e, "greg" | "flicker" | "lightning") || !seen.insert(e) {
            return Err(LegacyError::invalid("Invalid card events."));
        }
    }
    let wanted = permit["requested_events"].as_str().unwrap_or("");
    if !wanted.is_empty()
        && events.iter().filter_map(Value::as_str).collect::<Vec<_>>()
            != wanted
                .split(',')
                .filter(|e| !e.is_empty())
                .collect::<Vec<_>>()
    {
        return Err(LegacyError::invalid("Card events do not match the permit."));
    }
    Ok(())
}
pub(super) fn store(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let nonce = text(args, "nonce", 32)?;
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LegacyError::invalid("Invalid render permit."));
    }
    let permit = state
        .runtime
        .read_document(DOCUMENT)?
        .get(nonce)
        .cloned()
        .ok_or_else(|| LegacyError::not_found("Render permit expired."))?;
    if permit["state"] != "prepared" || permit["expires"].as_u64().unwrap_or(0) <= now() {
        return Err(LegacyError::conflict(
            "Render permit expired or was already used.",
        ));
    }
    let encoded = text(args, "gif", LIMIT.div_ceil(3) * 4)?;
    if encoded.len() > LIMIT.div_ceil(3) * 4 {
        return Err(LegacyError::invalid("Animated card is too large."));
    }
    let data = STANDARD
        .decode(encoded)
        .map_err(|_| LegacyError::invalid("Invalid GIF bytes."))?;
    let info = dumbmanager_card_renderer::inspect_gif(&data)
        .map_err(|_| LegacyError::invalid("Invalid animated card."))?;
    let meta = &args["metadata"];
    metadata(meta, &permit, &info)?;
    let id = permit["id"].as_str().unwrap_or("");
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    if row["version"] != permit["version"]
        || row["source_version"] != source["version"]
        || row["source_version"] != permit["source_version"]
    {
        return Err(LegacyError::conflict(
            "Draft or itch changed during rendering.",
        ));
    }
    let sha = format!("{:x}", Sha256::digest(&data));
    let filename = format!(
        "dumbmanager-{}.gif",
        source["id"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(8)
            .collect::<String>()
    );
    let result = json!({"draft_id":id,"version":row["version"],"source_version":row["source_version"],"stage":stage(&source),"digest":null,"export_ready":true,"vibe":permit["vibe"],"requested_theme":permit["requested_theme"],"requested_events":permit["requested_events"],"theme":meta["theme"],"key":meta["key"],"events":meta["events"],"frames":meta["frames"],"seconds":meta["seconds"],"width":1200,"height":675,"bytes":data.len(),"sha256":sha,"filename":filename});
    let path = directory(state.runtime.root())?.join(format!("browser-{nonce}.gif"));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| LegacyError::conflict("This render permit is already being stored."))?;
    if f.write_all(&data).and_then(|_| f.sync_all()).is_err() {
        let _ = fs::remove_file(&path);
        return Err(LegacyError::unavailable("Could not save animated card."));
    }
    drop(f);
    let mut evicted = Vec::new();
    let update = state.runtime.update_document(DOCUMENT, |doc| {
        let map = doc
            .as_object_mut()
            .ok_or_else(|| LegacyError::invalid("Unreadable card storage."))?;
        let mut total: u64 = map
            .values()
            .filter(|v| v["state"] == "ready")
            .map(|v| v["result"]["bytes"].as_u64().unwrap_or(0))
            .sum();
        while total.saturating_add(data.len() as u64) > CACHE_LIMIT {
            let oldest = map
                .iter()
                .filter(|(_, v)| v["state"] == "ready")
                .min_by_key(|(_, v)| v["stored_at"].as_u64().unwrap_or(0))
                .map(|(k, _)| k.clone())
                .ok_or_else(|| LegacyError::unavailable("Card cache is full."))?;
            if let Some(old) = map.remove(&oldest) {
                total = total.saturating_sub(old["result"]["bytes"].as_u64().unwrap_or(0));
            }
            evicted.push(oldest);
        }
        let item = map
            .get_mut(nonce)
            .ok_or_else(|| LegacyError::conflict("Render permit expired."))?;
        if item["state"] != "prepared" || item["expires"].as_u64().unwrap_or(0) <= now() {
            return Err(LegacyError::conflict(
                "Render permit expired or was already used.",
            ));
        }
        item["state"] = json!("ready");
        item["result"] = result.clone();
        item["stored_at"] = json!(now());
        Ok(())
    });
    if let Err(e) = update {
        let _ = fs::remove_file(path);
        return Err(e);
    }
    remove_evicted(state, &evicted);
    let mut response = result;
    response["gif"] = json!(STANDARD.encode(data));
    Ok(response)
}
pub(super) fn cached(state: &AppState, args: &Value, sha: Option<&str>) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let (vibe, theme, events) = variant(args)?;
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    if row["source_version"] != source["version"] {
        return Err(LegacyError::conflict(
            "The itch changed. Render the card again.",
        ));
    }
    let doc = state.runtime.read_document(DOCUMENT)?;
    let selected = doc.as_object().and_then(|m| {
        m.iter()
            .filter(|(_, v)| {
                v["state"] == "ready"
                    && v["id"] == id
                    && v["version"] == row["version"]
                    && v["source_version"] == source["version"]
                    && v["vibe"] == vibe
                    && v["requested_theme"] == theme
                    && v["requested_events"] == events
                    && sha.is_none_or(|s| v["result"]["sha256"] == s)
            })
            .max_by_key(|(_, v)| v["stored_at"].as_u64().unwrap_or(0))
    });
    let (nonce, item) = selected.ok_or_else(|| {
        LegacyError::conflict("Render this exact card before copying or sending.")
    })?;
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LegacyError::invalid("Invalid cached card."));
    }
    let path = directory(state.runtime.root())?.join(format!("browser-{nonce}.gif"));
    let m = fs::symlink_metadata(&path)
        .map_err(|_| LegacyError::not_found("Cached card is missing."))?;
    if !m.is_file()
        || m.file_type().is_symlink()
        || m.len() > LIMIT as u64
        || m.len() != item["result"]["bytes"].as_u64().unwrap_or(0)
    {
        return Err(LegacyError::invalid("Cached card is invalid."));
    }
    let bytes = fs::read(path).map_err(|_| LegacyError::unavailable("Cannot read cached card."))?;
    if format!("{:x}", Sha256::digest(&bytes)) != item["result"]["sha256"].as_str().unwrap_or("") {
        return Err(LegacyError::invalid("Cached card changed."));
    }
    let mut result = item["result"].clone();
    result["gif"] = json!(STANDARD.encode(bytes));
    Ok(result)
}
