//! Bounded, app-owned replay frameboards. Callers only address opaque itch IDs.
use base64::{engine::general_purpose::STANDARD, Engine};
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
const MIB: u64 = 1024 * 1024;
const TOTAL: u64 = 256 * MIB;
const KEPT: u64 = 128 * MIB;
const EVENT: u64 = 20 * MIB;
const OWNER: &str = "io.dumbmanager.rust.replay.v1";
static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
fn storage() -> LegacyError {
    LegacyError {
        error: "Could not access this app's replay storage.".into(),
        code: "service_storage".into(),
        status: 500,
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn id_ok(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn ordinary(path: &Path, dir: bool) -> bool {
    let Ok(m) = fs::symlink_metadata(path) else {
        return false;
    };
    if m.file_type().is_symlink() || if dir { !m.is_dir() } else { !m.is_file() } {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}
fn root(base: &Path) -> LegacyResult<PathBuf> {
    fs::create_dir_all(base).map_err(|_| storage())?;
    if !ordinary(base, true) {
        return Err(storage());
    }
    let dir = base.join("replay");
    match fs::create_dir(&dir) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(storage()),
    };
    if !ordinary(&dir, true)
        || fs::canonicalize(&dir).map_err(|_| storage())?.parent()
            != Some(fs::canonicalize(base).map_err(|_| storage())?.as_path())
    {
        return Err(storage());
    }
    Ok(dir)
}
fn folder(base: &Path, id: &str) -> LegacyResult<PathBuf> {
    if !id_ok(id) {
        return Err(LegacyError::invalid("Invalid replay id."));
    }
    let root = root(base)?;
    let path = root.join(id);
    if path.exists()
        && (!ordinary(&path, true)
            || fs::canonicalize(&path).map_err(|_| storage())?.parent()
                != Some(fs::canonicalize(&root).map_err(|_| storage())?.as_path()))
    {
        return Err(storage());
    }
    Ok(path)
}
fn read_json(path: &Path) -> LegacyResult<Value> {
    if !ordinary(path, false) {
        return Err(storage());
    }
    let mut b = Vec::new();
    fs::File::open(path)
        .map_err(|_| storage())?
        .take(128 * 1024)
        .read_to_end(&mut b)
        .map_err(|_| storage())?;
    serde_json::from_slice(&b).map_err(|_| storage())
}
fn manifest(base: &Path, id: &str) -> LegacyResult<Option<Value>> {
    let p = folder(base, id)?;
    if !p.exists() {
        return Ok(None);
    }
    let v = read_json(&p.join("index.json"))?;
    if v["owner"] != OWNER
        || v["id"] != id
        || v["frames"]
            .as_array()
            .is_none_or(|a| a.is_empty() || a.len() > 300)
        || v["bytes"].as_u64().is_none_or(|n| n > EVENT)
        || v["created"].as_u64().is_none()
    {
        return Err(storage());
    }
    Ok(Some(v))
}
fn discard_incomplete(path: &Path) -> LegacyResult<()> {
    if !ordinary(path, true) {
        return Err(storage());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(path).map_err(|_| storage())? {
        let entry = entry.map_err(|_| storage())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let known = name == "index.json"
            || (name.len() == 8
                && name.ends_with(".jpg")
                && name[..4].bytes().all(|b| b.is_ascii_digit()))
            || name
                .strip_prefix("manifest-")
                .and_then(|n| n.strip_suffix(".tmp"))
                .is_some_and(id_ok);
        if !known || !ordinary(&entry.path(), false) || files.len() >= 602 {
            return Err(storage());
        }
        files.push(entry.path());
    }
    for file in files {
        fs::remove_file(file).map_err(|_| storage())?;
    }
    fs::remove_dir(path).map_err(|_| storage())
}
fn purge_manifest_temps(path: &Path) -> LegacyResult<()> {
    let mut pending = Vec::new();
    for (count, entry) in fs::read_dir(path).map_err(|_| storage())?.enumerate() {
        if count >= 602 {
            return Err(storage());
        }
        let entry = entry.map_err(|_| storage())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name
            .strip_prefix("manifest-")
            .and_then(|n| n.strip_suffix(".tmp"))
            .is_some_and(id_ok)
        {
            if !ordinary(&entry.path(), false) {
                return Err(storage());
            }
            pending.push(entry.path());
        }
    }
    for file in pending {
        fs::remove_file(file).map_err(|_| storage())?;
    }
    Ok(())
}
fn rows(base: &Path) -> LegacyResult<Vec<Value>> {
    let root = root(base)?;
    let mut out = Vec::new();
    for (count, entry) in fs::read_dir(&root).map_err(|_| storage())?.enumerate() {
        if count >= 4096 {
            return Err(storage());
        }
        let entry = entry.map_err(|_| storage())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.strip_prefix("stage-").is_some_and(id_ok) {
            discard_incomplete(&entry.path())?;
        } else if id_ok(&name) {
            match manifest(base, &name) {
                Ok(Some(v)) => {
                    purge_manifest_temps(&folder(base, &name)?)?;
                    out.push(v)
                }
                _ => discard_incomplete(&folder(base, &name)?)?,
            }
        } else {
            return Err(storage());
        }
    }
    Ok(out)
}
fn remove(base: &Path, id: &str) -> LegacyResult<bool> {
    let path = folder(base, id)?;
    if !path.exists() {
        return Ok(false);
    }
    let row = manifest(base, id)?.ok_or_else(storage)?;
    purge_manifest_temps(&path)?;
    let count = row["frames"].as_array().ok_or_else(storage)?.len();
    let mut names = Vec::new();
    for e in fs::read_dir(&path).map_err(|_| storage())? {
        let e = e.map_err(|_| storage())?;
        let name = e.file_name().to_string_lossy().into_owned();
        let valid = name == "index.json"
            || (name.len() == 8
                && name.ends_with(".jpg")
                && name[..4].bytes().all(|b| b.is_ascii_digit())
                && name[..4].parse::<usize>().is_ok_and(|i| i < count));
        if !valid || !ordinary(&e.path(), false) {
            return Err(storage());
        }
        names.push(e.path())
    }
    for p in names {
        fs::remove_file(p).map_err(|_| storage())?
    }
    fs::remove_dir(path).map_err(|_| storage())?;
    Ok(true)
}
fn cleanup(base: &Path, reserve: u64) -> LegacyResult<()> {
    let mut all = rows(base)?;
    all.sort_by_key(|r| r["created"].as_u64().unwrap_or(0));
    let mut usage: u64 = all.iter().map(|r| r["bytes"].as_u64().unwrap_or(0)).sum();
    for r in all {
        let size = r["bytes"].as_u64().unwrap_or(0);
        if r["keep"] != true
            && (now().saturating_sub(r["created"].as_u64().unwrap_or(0)) > 86400
                || usage.saturating_add(reserve) > TOTAL)
            && remove(base, r["id"].as_str().unwrap_or(""))?
        {
            usage = usage.saturating_sub(size)
        }
    }
    if usage.saturating_add(reserve) > TOTAL {
        return Err(LegacyError::conflict(
            "Replay storage is full. Delete or unpin a board.",
        ));
    }
    Ok(())
}
fn write_new(path: &Path, data: &[u8]) -> LegacyResult<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| storage())?;
    f.write_all(data)
        .and_then(|_| f.sync_all())
        .map_err(|_| storage())
}
fn atomic_replace(path: &Path, data: &[u8]) -> LegacyResult<()> {
    let tmp = path.with_file_name(format!("manifest-{}.tmp", uuid::Uuid::new_v4().simple()));
    if let Err(error) = write_new(&tmp, data) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    #[cfg(windows)]
    let ok = {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
        }
        let a: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0x1 | 0x8) != 0 }
    };
    #[cfg(not(windows))]
    let ok = fs::rename(&tmp, path).is_ok();
    if !ok {
        let _ = fs::remove_file(tmp);
        return Err(storage());
    }
    Ok(())
}
/// Save completed frames obtained by a trusted selected-window capture session.
pub fn save(
    base: &Path,
    id: &str,
    frames: &[(f64, Vec<u8>)],
    truncated: bool,
    window: &Value,
) -> LegacyResult<()> {
    let _g = lock();
    if !id_ok(id) || frames.is_empty() || frames.len() > 300 {
        return Err(LegacyError::invalid("Invalid replay frames."));
    }
    let bytes: u64 = frames.iter().map(|(_, b)| b.len() as u64).sum();
    if bytes > EVENT
        || frames.iter().any(|(t, b)| {
            !t.is_finite()
                || *t < 0.0
                || b.len() > 2 * 1024 * 1024
                || !b.starts_with(&[0xff, 0xd8])
                || !b.ends_with(&[0xff, 0xd9])
        })
        || frames.windows(2).any(|w| w[0].0 > w[1].0)
    {
        return Err(LegacyError::invalid("Invalid or oversized replay frames."));
    }
    cleanup(base, bytes)?;
    let path = folder(base, id)?;
    if path.exists() {
        return Err(LegacyError::conflict(
            "A replay already exists for this itch.",
        ));
    }
    let staged = root(base)?.join(format!("stage-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&staged).map_err(|_| storage())?;
    let result = (|| {
        for (i, (_, b)) in frames.iter().enumerate() {
            write_new(&staged.join(format!("{i:04}.jpg")), b)?
        }
        let stamps: Vec<f64> = frames.iter().map(|(t, _)| *t).collect();
        let row = json!({"owner":OWNER,"id":id,"created":now(),"keep":false,"frames":stamps,"bytes":bytes,"truncated":truncated,"window":window,"duration":frames.last().unwrap().0,"format":"jpeg-frameboard","audio":false});
        write_new(
            &staged.join("index.json"),
            &serde_json::to_vec(&row).map_err(|_| storage())?,
        )?;
        fs::rename(&staged, &path).map_err(|_| storage())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = discard_incomplete(&staged);
    }
    result
}
pub fn item(base: &Path, id: &str) -> LegacyResult<Value> {
    let _g = lock();
    cleanup(base, 0)?;
    Ok(match manifest(base, id)? {
        Some(mut v) => {
            v["available"] = json!(true);
            v
        }
        None => json!({"available":false,"pending":false}),
    })
}
pub fn frame(base: &Path, id: &str, index: u64) -> LegacyResult<Value> {
    let _g = lock();
    let row = manifest(base, id)?.ok_or_else(|| LegacyError::not_found("Frame unavailable."))?;
    let count = row["frames"].as_array().ok_or_else(storage)?.len();
    if index as usize >= count {
        return Err(LegacyError::not_found("Frame unavailable."));
    }
    let file = folder(base, id)?.join(format!("{index:04}.jpg"));
    if !ordinary(&file, false) {
        return Err(storage());
    }
    let mut bytes = Vec::new();
    fs::File::open(file)
        .map_err(|_| storage())?
        .take(2 * MIB + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| storage())?;
    if bytes.len() as u64 > 2 * MIB {
        return Err(storage());
    }
    Ok(
        json!({"__native_binary":true,"base64":STANDARD.encode(bytes),"mime_type":"image/jpeg","status":200}),
    )
}
pub fn keep(base: &Path, id: &str, keep: bool) -> LegacyResult<Value> {
    let _g = lock();
    let mut row =
        manifest(base, id)?.ok_or_else(|| LegacyError::not_found("Replay unavailable."))?;
    if keep && row["keep"] != true {
        let usage: u64 = rows(base)?
            .iter()
            .filter(|r| r["keep"] == true)
            .map(|r| r["bytes"].as_u64().unwrap_or(0))
            .sum();
        if usage.saturating_add(row["bytes"].as_u64().unwrap_or(0)) > KEPT {
            return Err(LegacyError::conflict("Kept replay budget is full."));
        }
    }
    row["keep"] = json!(keep);
    atomic_replace(
        &folder(base, id)?.join("index.json"),
        &serde_json::to_vec(&row).map_err(|_| storage())?,
    )?;
    Ok(row)
}
pub fn delete(base: &Path, id: &str) -> LegacyResult<Value> {
    let _g = lock();
    Ok(json!({"deleted":remove(base,id)?}))
}
