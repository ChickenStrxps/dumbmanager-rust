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
fn get(state: &AppState, id: &str) -> LegacyResult<Value> {
    let row = state
        .store()
        .legacy_call("manager.get", &json!({"id":id}))?;
    let ready = ["ready", "resolved"].contains(&row["state"].as_str().unwrap_or(""))
        || row["run_state"] == "agent_reported_done";
    let eligible =
        ready && !["archived", "needs_description"].contains(&row["state"].as_str().unwrap_or(""));
    let mut sources = vec![
        json!({"id":"observation","label":"Original request","text":safe(row["original"].as_str().filter(|s|!s.is_empty()).or_else(||row["title"].as_str()).unwrap_or("")),"kind":"request"}),
    ];
    for p in row["plan"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["done"] == true)
        .take(16)
    {
        sources.push(json!({"id":format!("plan:{}",p["id"].as_str().unwrap_or("")),"label":"Reported completed step","text":safe(p["text"].as_str().unwrap_or("")),"kind":"report"}));
    }
    for (i, e) in row["evidence"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .take(16)
    {
        if let Some(summary) = e["summary"].as_str() {
            sources.push(json!({"id":format!("evidence:{i}:summary"),"label":"Saved result summary","text":safe(summary),"kind":"report"}));
        }
        for (j, cmd) in e["commands"]
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
            .enumerate()
        {
            if let Some(s) = cmd.as_str() {
                sources.push(json!({"id":format!("evidence:{i}:command:{j}"),"label":"Reported command","text":safe(s),"kind":"command"}));
            }
        }
    }
    // Research excerpts are reported evidence, not independently verified source reads.
    let research = state.runtime.read_document("ai-research.json")?;
    for run in research["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["itch_id"] == id)
        .rev()
        .take(4)
    {
        for report in run["reports"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .take(3)
        {
            for source in report["sources"].as_array().into_iter().flatten().take(8) {
                let excerpt = safe(source["excerpt"].as_str().unwrap_or(""));
                if !excerpt.is_empty() && excerpt != "[Credential-bearing text omitted]" {
                    sources.push(json!({"id":format!("research:{}:{}",report["id"].as_str().unwrap_or(""),source["id"].as_str().unwrap_or("")),"label":format!("Research report source: {}",source["title"].as_str().unwrap_or("Untitled")),"text":excerpt,"kind":"research","url":source["url"],"path":source["path"],"provenance":"Reported excerpt; not independently verified."}));
                }
            }
        }
    }
    let basis = digest(&json!([id, row["state"], sources]));
    let doc = state.runtime.read_document(DOC)?;
    let saved = &doc["notes"][id];
    let stale = saved.is_object() && saved["basis"] != basis;
    let notes = if stale {
        Value::Null
    } else {
        saved["notes"].clone()
    };
    let mut lessons = Vec::new();
    let mut commands = Vec::new();
    let mut packages = Vec::new();
    if eligible {
        let work = sources.iter().find(|s| s["kind"] == "report");
        let (explanation, ids) = if let Some(s) = work {
            (format!("The saved work reports: {}\n\nThis is reported history, not a new verification.",s["text"].as_str().unwrap_or("")),json!(["observation",s["id"]]))
        } else {
            ("This itch is ready or accepted, but no completed step or result summary was saved. The implementation cannot be inferred from its title.".to_string(),json!(["observation"]))
        };
        lessons.push(json!({"id":format!("{}-review-{}",id,basis),"title":"Review the saved work","explanation":explanation,"try_this":"Compare the request with the saved evidence and identify what still needs checking.","question":"Does opening this trail rerun the work or tests?","answer":"No. This trail only reads saved records.","source_ids":ids}));
        for s in sources
            .iter()
            .filter(|s| s["kind"] == "command" && s["text"] != "[Credential-bearing text omitted]")
            .take(8)
        {
            commands.push(json!({"command":s["text"],"explanation":"Saved in the work report. Review arguments and workspace before reuse.","caution":"Reported history; this view does not run or verify it.","source_ids":[s["id"]],"kind":"Recorded command"}));
        }
        for entry in notes["lessons"].as_array().into_iter().flatten().take(6) {
            let mut l = entry.clone();
            l["id"] = json!(format!("{}-note-{}", id, digest(entry)));
            l["source_ids"] = json!([entry["source_id"]]);
            lessons.push(l);
        }
        for entry in notes["commands"].as_array().into_iter().flatten().take(6) {
            let mut c = entry.clone();
            c["source_ids"] = json!([entry["source_id"]]);
            c["kind"] = json!("Recorded command");
            commands.push(c);
        }
        for entry in notes["packages"].as_array().into_iter().flatten().take(6) {
            let mut p = entry.clone();
            p["source_ids"] = json!([entry["source_id"]]);
            p["kind"] = json!("Agent-written package tip");
            p["docs"] = json!("");
            packages.push(p);
        }
    }
    let revision = digest(&json!([basis, notes]));
    let progress = doc["progress"][id].clone();
    let valid: Vec<_> = progress["read"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| lessons.iter().any(|l| l["id"] == **v))
        .cloned()
        .collect();
    Ok(
        json!({"id":id,"title":safe(row["title"].as_str().unwrap_or("")),"project":row["project"],"eligible":eligible,"stage":if row["state"]=="resolved"{"Accepted by you"}else if eligible{"Ready to test"}else{"Work not finished"},"revision":revision,"source_revision":basis,"work_revision":basis,"itch_version":row["version"],"workspace":"","environment":"","workspace_note":"No workspace was independently verified.","lessons":lessons,"commands":commands,"packages":packages,"sources":sources,"notes_stale":stale,"notes_version":saved["version"].as_u64().unwrap_or(0),"progress":{"version":progress["version"].as_u64().unwrap_or(0),"read":valid,"total":lessons.len()},"coverage":"Built locally from this itch's saved work. Reports are not rerun and dependencies are not inferred."}),
    )
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
