//! Learning from saved work only; no commands, model, or source scans run here.
use super::{fields, text};
use crate::state::AppState;
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const DOC: &str = "ai-learning.json";
fn digest(v: &Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))[..24].to_string()
}
pub(super) fn safe(s: &str) -> String {
    let lower = s.to_lowercase();
    if [
        "bearer ",
        "api_key",
        "api-key",
        "access_token",
        "password",
        "secret",
        "authorization",
        "cookie",
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
    ]
    .iter()
    .any(|x| lower.contains(x))
    {
        return "[Credential-bearing text omitted]".into();
    }
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(2400)
        .collect()
}
#[path = "learning_content.rs"]
mod content;
fn get(state: &AppState, id: &str) -> LegacyResult<Value> {
    let row = state
        .store()
        .legacy_call("manager.get", &json!({"id":id}))?;
    let research = state.runtime.read_document("ai-research.json")?;
    let doc = state.runtime.read_document(DOC)?;
    Ok(content::build(&row, &research, &doc))
}
fn record(state: &AppState, args: &Value) -> LegacyResult<Value> {
    fields(args, &["id", "revision", "notes"])?;
    let id = text(args, "id", 100, true)?;
    let revision = text(args, "revision", 100, true)?;
    let raw = text(args, "notes", 18000, true)?;
    let notes: Value = serde_json::from_str(raw)
        .map_err(|_| LegacyError::invalid("Learning notes must be JSON."))?;
    fields(&notes, &["lessons", "commands", "packages"])?;
    let pack = get(state, id)?;
    if pack["revision"] != revision {
        return Err(LegacyError::conflict("Learning trail changed. Refresh."));
    }
    if pack["eligible"] != true {
        return Err(LegacyError::invalid(
            "Notes require ready or accepted work.",
        ));
    }
    for (kind, names) in [
        (
            "lessons",
            vec!["title", "explanation", "try_this", "question", "answer"],
        ),
        ("commands", vec!["command", "explanation", "caution"]),
        ("packages", vec!["name", "tip"]),
    ] {
        let Some(entries) = notes.get(kind) else {
            continue;
        };
        let entries = entries
            .as_array()
            .filter(|a| a.len() <= 6)
            .ok_or_else(|| LegacyError::invalid("Use up to six notes in each section."))?;
        for entry in entries {
            let mut allowed = names.clone();
            allowed.extend(["source_id", "quote"]);
            fields(entry, &allowed)?;
            let source = text(entry, "source_id", 200, true)?;
            let quote = text(entry, "quote", 900, true)?;
            let found = pack["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| {
                    s["id"] == source
                        && ["report", "command", "research"]
                            .contains(&s["kind"].as_str().unwrap_or(""))
                })
                .ok_or_else(|| {
                    LegacyError::invalid(
                        "Cite completed work, reported commands or a saved research excerpt.",
                    )
                })?;
            if !found["text"].as_str().is_some_and(|s| s.contains(quote)) || safe(quote) != quote {
                return Err(LegacyError::invalid(
                    "Quote exact non-sensitive saved work.",
                ));
            }
            for name in &names {
                let value = text(entry, name, 1400, *name != "caution")?;
                if safe(value) != value {
                    return Err(LegacyError::invalid(
                        "Remove credentials from learning notes.",
                    ));
                }
            }
            if kind == "commands" && !quote.contains(text(entry, "command", 900, true)?) {
                return Err(LegacyError::invalid(
                    "A command must appear in its cited quote.",
                ));
            }
            if kind == "packages"
                && !quote
                    .to_lowercase()
                    .contains(&text(entry, "name", 900, true)?.to_lowercase())
            {
                return Err(LegacyError::invalid(
                    "A package name must appear in its cited quote.",
                ));
            }
        }
    }
    let basis = pack["work_revision"].clone();
    let version = pack["notes_version"].as_u64().unwrap_or(0);
    state.runtime.update_document(DOC, |doc| {
        if !doc.is_object() {
            *doc = json!({})
        }
        if !doc["notes"].is_object() {
            doc["notes"] = json!({})
        }
        if doc["notes"][id]["version"].as_u64().unwrap_or(0) != version {
            return Err(LegacyError::conflict("Learning notes changed. Refresh."));
        }
        doc["notes"][id] = json!({"basis":basis,"version":version+1,"notes":notes});
        Ok(())
    })?;
    get(state, id)
}
fn progress(state: &AppState, args: &Value) -> LegacyResult<Value> {
    fields(
        args,
        &["id", "revision", "lesson_id", "done", "expected_version"],
    )?;
    let id = text(args, "id", 100, true)?;
    let revision = text(args, "revision", 100, true)?;
    let lesson = text(args, "lesson_id", 200, true)?;
    let expected = args["expected_version"]
        .as_u64()
        .ok_or_else(|| LegacyError::invalid("Supply the progress version."))?;
    let done = args.get("done").and_then(Value::as_bool).unwrap_or(true);
    let pack = get(state, id)?;
    if pack["revision"] != revision
        || !pack["lessons"]
            .as_array()
            .is_some_and(|a| a.iter().any(|l| l["id"] == lesson))
    {
        return Err(LegacyError::conflict(
            "Learning trail changed. Refresh before saving progress.",
        ));
    }
    state.runtime.update_document(DOC, |doc| {
        if !doc.is_object() {
            *doc = json!({})
        }
        if !doc["progress"].is_object() {
            doc["progress"] = json!({})
        }
        let p = &mut doc["progress"][id];
        if !p.is_object() {
            *p = json!({"version":0,"read":[]})
        }
        if p["version"].as_u64() != Some(expected) {
            return Err(LegacyError::conflict("Learning progress changed. Refresh."));
        }
        let read = p["read"]
            .as_array_mut()
            .ok_or_else(|| LegacyError::invalid("Invalid saved progress."))?;
        read.retain(|v| v != lesson);
        if done {
            read.push(json!(lesson))
        }
        p["version"] = json!(expected + 1);
        Ok(())
    })?;
    get(state, id)
}
pub(super) fn handle(state: &AppState, name: &str, args: &Value) -> Option<LegacyResult<Value>> {
    Some(match name {
        "manager.learning.get" => match text(args, "id", 100, true) {
            Ok(id) => get(state, id),
            Err(e) => Err(e),
        },
        "manager.learning.progress" => progress(state, args),
        "manager.learning.record" => record(state, args),
        _ => return None,
    })
}
