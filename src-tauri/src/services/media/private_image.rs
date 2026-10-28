//! Separate flattened image copies for social cards; original evidence remains untouched.
use super::*;
use dumbmanager_card_renderer::sanitize::{self, Rect};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const CAP: u64 = 64 * 1024 * 1024;
fn rect(v: &Value, min: f64) -> LegacyResult<Rect> {
    let obj = v
        .as_object()
        .filter(|o| {
            o.len() == 4
                && ["x", "y", "width", "height"]
                    .iter()
                    .all(|k| o.contains_key(*k))
        })
        .ok_or_else(|| LegacyError::invalid("Use normalized image rectangles."))?;
    let n = |k| {
        obj.get(k)
            .and_then(Value::as_f64)
            .ok_or_else(|| LegacyError::invalid("Use normalized image rectangles."))
    };
    let r = Rect {
        x: n("x")?,
        y: n("y")?,
        width: n("width")?,
        height: n("height")?,
    };
    if !r.valid(min) {
        return Err(LegacyError::invalid(
            "Keep image rectangles within the picture.",
        ));
    }
    Ok(r)
}
fn rectangle(r: Rect) -> Value {
    json!({"x":r.x,"y":r.y,"width":r.width,"height":r.height})
}
fn options(args: &Value) -> LegacyResult<(Rect, Vec<Rect>)> {
    let crop = args
        .get("crop")
        .and_then(Value::as_str)
        .unwrap_or("{\"x\":0,\"y\":0,\"width\":1,\"height\":1}");
    let masks = args.get("masks").and_then(Value::as_str).unwrap_or("[]");
    if crop.len() > 1024 || masks.len() > 8192 {
        return Err(LegacyError::invalid("Image rectangles are too large."));
    }
    let crop: Value = serde_json::from_str(crop)
        .map_err(|_| LegacyError::invalid("Invalid image rectangles."))?;
    let masks: Value = serde_json::from_str(masks)
        .map_err(|_| LegacyError::invalid("Invalid image rectangles."))?;
    let array = masks
        .as_array()
        .filter(|a| a.len() <= 32)
        .ok_or_else(|| LegacyError::invalid("Use up to 32 privacy masks."))?;
    Ok((
        rect(&crop, 0.05)?,
        array
            .iter()
            .map(|r| rect(r, 0.001))
            .collect::<LegacyResult<Vec<_>>>()?,
    ))
}
fn folder(root: &Path) -> LegacyResult<PathBuf> {
    let path = root.join("social-images");
    fs::create_dir_all(&path)
        .map_err(|_| LegacyError::unavailable("Cannot create private image folder."))?;
    let m = fs::symlink_metadata(&path)
        .map_err(|_| LegacyError::unavailable("Cannot inspect private image folder."))?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || fs::canonicalize(&path)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            != fs::canonicalize(root).ok()
    {
        return Err(LegacyError::invalid("Invalid private image folder."));
    }
    Ok(path)
}
fn filename(v: &Value) -> LegacyResult<&str> {
    let s = v["file"].as_str().unwrap_or("");
    if s.len() != 36
        || !s.ends_with(".png")
        || !s.as_bytes()[..32]
            .iter()
            .copied()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LegacyError::invalid("Invalid private image."));
    }
    Ok(s)
}
pub(super) fn uri(state: &AppState, image: &Value) -> LegacyResult<String> {
    if image.is_null() {
        return Ok(String::new());
    }
    let path = folder(state.runtime.root())?.join(filename(image)?);
    let meta = fs::symlink_metadata(&path).map_err(|_| {
        LegacyError::not_found("The private preview is missing. Add the image again.")
    })?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 2_000_000 {
        return Err(LegacyError::invalid("Invalid private preview."));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| LegacyError::unavailable("Cannot read private preview."))?
        .take(2_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| LegacyError::unavailable("Cannot read private preview."))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || format!("{:x}", Sha256::digest(&bytes)) != image["digest"].as_str().unwrap_or("")
    {
        return Err(LegacyError::conflict(
            "The private preview changed. Create it again.",
        ));
    }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}
pub(super) fn public(image: &Value) -> Value {
    let mut image = image.clone();
    if let Some(o) = image.as_object_mut() {
        o.remove("file");
    }
    image
}
pub(super) fn set(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let expected = version(args)?;
    let (row, source) = get(state, id)?;
    eligible(&source)?;
    if row["version"].as_u64() != Some(expected) {
        return Err(LegacyError::conflict(
            "This draft changed. Reload it before editing the picture.",
        ));
    }
    let (crop, masks) = options(args)?;
    let selected = args
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("receipt");
    if !matches!(selected, "none" | "receipt" | "browser") {
        return Err(LegacyError::invalid("Unknown image source."));
    }
    let captured =
        if selected == "browser" {
            let frame = crate::services::browser::handle(
                state,
                "/api/call",
                &json!({"name":"browser.snapshot","args":{}}),
            )
            .ok_or_else(|| LegacyError::unavailable("Browse capture is unavailable."))??;
            let upload = crate::services::media::handle(state,"/api/itch-upload",&json!({
            "base64":frame["image"],"filename":"work-screenshot.jpg","mime_type":"image/jpeg"
        })).ok_or_else(||LegacyError::unavailable("Image storage is unavailable."))??;
            Some(
                upload["attachment"]["id"]
                    .as_str()
                    .ok_or_else(|| LegacyError::invalid("Browse did not return an image."))?
                    .to_owned(),
            )
        } else {
            None
        };
    let mut created = None;
    let image = if selected == "none" {
        Value::Null
    } else {
        let attachment_id = captured
            .as_deref()
            .or_else(|| {
                args.get("attachment_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                source["attachments"]
                    .as_array()
                    .and_then(|a| a.iter().rev().find(|v| v["type"] == "image"))
                    .and_then(|v| v["id"].as_str())
            })
            .ok_or_else(|| LegacyError::invalid("Add a work screenshot first."))?;
        let attachment =
            crate::services::media::references(state, &json!([attachment_id]))?.remove(0);
        if attachment["type"] != "image"
            || attachment["sizeBytes"].as_u64().unwrap_or(u64::MAX) > 10 * 1024 * 1024
        {
            return Err(LegacyError::invalid("Choose a saved image up to 10 MiB."));
        }
        let data = crate::services::media::read_attachment(state, attachment_id)?;
        let bytes = STANDARD
            .decode(data["base64"].as_str().unwrap_or(""))
            .map_err(|_| LegacyError::invalid("Invalid saved image."))?;
        let processed =
            sanitize::sanitize(&bytes, crop, &masks).map_err(|e| LegacyError::unavailable(&e))?;
        if processed.png.len() > 2_000_000 {
            return Err(LegacyError::invalid("The masked picture exceeds 2 MB."));
        }
        let dir = folder(state.runtime.root())?;
        let total = fs::read_dir(&dir)
            .map_err(|_| LegacyError::unavailable("Cannot inspect private image storage."))?
            .try_fold(0u64, |n, e| -> std::io::Result<u64> {
                let e = e?;
                let m = e.metadata()?;
                Ok(n.saturating_add(if m.is_file() { m.len() } else { 0 }))
            })
            .map_err(|_| LegacyError::unavailable("Cannot inspect private image storage."))?;
        if total.saturating_add(processed.png.len() as u64) > CAP {
            return Err(LegacyError::unavailable(
                "Private preview storage reached 64 MiB.",
            ));
        }
        let name = format!("{}.png", uuid::Uuid::new_v4().simple());
        let path = dir.join(&name);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| LegacyError::unavailable("Cannot save private preview."))?;
        if f.write_all(&processed.png)
            .and_then(|_| f.sync_all())
            .is_err()
        {
            let _ = fs::remove_file(path);
            return Err(LegacyError::unavailable("Cannot save private preview."));
        }
        created = Some(path);
        json!({"file":name,"digest":format!("{:x}",Sha256::digest(&processed.png)),"source":attachment,"crop":rectangle(crop),"masks":masks.iter().copied().map(rectangle).collect::<Vec<_>>(),"boxes":processed.boxes.into_iter().map(rectangle).collect::<Vec<_>>(),"automatic_regions":processed.automatic_regions,"width":processed.width,"height":processed.height,"reviewed":false,"created_at":time::now_iso()})
    };
    let latest = itch(state, row["itch_id"].as_str().unwrap_or(""))?;
    if let Err(e) = eligible(&latest) {
        if let Some(p) = created {
            let _ = fs::remove_file(p);
        }
        return Err(e);
    }
    let result = state.runtime.update_document(DRAFTS, |doc| {
        let current = doc
            .get_mut(id)
            .ok_or_else(|| LegacyError::not_found("Draft not found."))?;
        if current["version"].as_u64() != Some(expected) {
            return Err(LegacyError::conflict(
                "This draft changed while the image was processing.",
            ));
        }
        current["image"] = image;
        current["image_opt_out"] = json!(selected == "none");
        current["version"] = json!(expected + 1);
        current["updated_at"] = json!(time::now_iso());
        Ok(())
    });
    match result {
        Ok(doc) => refresh(state, &doc[id], &latest),
        Err(e) => {
            if let Some(p) = created {
                let _ = fs::remove_file(p);
            }
            Err(e)
        }
    }
}
pub(super) fn review(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let expected = version(args)?;
    let digest = text(args, "digest", 64)?;
    let (_, source) = get(state, id)?;
    eligible(&source)?;
    let doc = state.runtime.update_document(DRAFTS, |doc| {
        let row = doc
            .get_mut(id)
            .ok_or_else(|| LegacyError::not_found("Draft not found."))?;
        if row["version"].as_u64() != Some(expected) || row["image"]["digest"] != digest {
            return Err(LegacyError::conflict(
                "The image changed. Review the latest preview.",
            ));
        }
        row["image"]["reviewed"] = json!(true);
        row["version"] = json!(expected + 1);
        row["updated_at"] = json!(time::now_iso());
        Ok(())
    })?;
    refresh(state, &doc[id], &source)
}
