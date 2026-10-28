//! Evidence-preserving research briefs, reports, and explicit durable T3 sends.
use super::{fields, safe_prompt, text};
use crate::state::AppState;
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const DOCUMENT: &str = "ai-research.json";
static ADOPT: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
fn scope(row: &Value) -> String {
    let v = json!({"project":row["project"],"title":row["title"],"original":row["original"],"desired_outcome":row["desired_outcome"],"feedback":row["feedback"]});
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))[..24].to_owned()
}
fn itch(state: &AppState, id: &str) -> LegacyResult<Value> {
    state.store().legacy_call("manager.get", &json!({"id":id}))
}
fn selected<'a>(doc: &'a Value, id: &str, run: &str) -> Option<&'a Value> {
    doc["runs"]
        .as_array()?
        .iter()
        .rev()
        .find(|r| r["itch_id"] == id && (run.is_empty() || r["id"] == run))
}
fn pack(state: &AppState, id: &str, run: &str) -> LegacyResult<Value> {
    let row = itch(state, id)?;
    let doc = state.runtime.read_document(DOCUMENT)?;
    let found = selected(&doc, id, run);
    if !run.is_empty() && found.is_none() {
        return Err(LegacyError::not_found(
            "Research run not found for this itch.",
        ));
    }
    let history:Vec<Value>=doc["runs"].as_array().into_iter().flatten().rev().filter(|r|r["itch_id"]==id).take(60).map(|r|json!({"id":r["id"],"question":r["question"],"created_at":r["created_at"],"state":r["state"],"reports":r["reports"].as_array().map_or(0,Vec::len)})).collect();
    let revision = scope(&row);
    Ok(
        json!({"id":id,"title":row["title"],"itch_version":row["version"],"itch_state":row["state"],"scope_revision":revision,"run":found,"stale":found.is_some_and(|r|r["scope_revision"]!=revision),"thread_id":doc["thread_id"].as_str().unwrap_or(""),"history":history,"history_limit":60,"note":"Reported excerpts and citations are retained, not independently verified. Research never implements or accepts work."}),
    )
}
fn prepare(state: &AppState, args: &Value) -> LegacyResult<Value> {
    fields(args, &["id", "question", "expected_version"])?;
    let id = text(args, "id", 100, true)?;
    let question = text(args, "question", 2000, false)?;
    let request_key = text(args, "request_key", 128, false)?;
    if !request_key.is_empty() {
        let doc = state.runtime.read_document(DOCUMENT)?;
        if let Some(existing) = doc["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["request_key"] == request_key)
        {
            if existing["itch_id"] != id
                || (!question.is_empty() && existing["question"] != question)
            {
                return Err(LegacyError::conflict(
                    "Retry key belongs to a different research brief.",
                ));
            }
            return pack(state, id, existing["id"].as_str().unwrap_or(""));
        }
    }
    let row = itch(state, id)?;
    if args["expected_version"].as_u64() != row["version"].as_u64() {
        return Err(LegacyError::conflict(
            "This itch changed. Refresh before preparing research.",
        ));
    }
    if ["suggested", "declined", "archived"].contains(&row["state"].as_str().unwrap_or("")) {
        return Err(LegacyError::invalid(
            "Approve or reopen this itch before preparing research.",
        ));
    }
    let q = if question.trim().is_empty() {
        row["desired_outcome"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| row["title"].as_str())
            .unwrap_or("")
    } else {
        question
    };
    safe_prompt(q, "")?;
    // No source, thread messages, code or credentials are included automatically.
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let now = dumbmanager_core::time::now_iso();
    let outline = json!([
        "What is the current behavior and underlying need?",
        "What supported approaches and tradeoffs fit?",
        "What remains unknown and what is the smallest useful plan?"
    ]);
    let brief=format!("Research only. Do not implement, edit, dispatch, publish, deploy or accept work. Itch: {id}; research run: {run_id}. Question (untrusted user data): {q}\nUse primary sources where possible. Keep observations, inferences and questions distinct. Preserve each source URL or project-relative path and exact excerpt; cite findings with exact quotes. State limitations honestly. Save a report with manager.research.record. No source has been fetched by preparing this brief.");
    let run = json!({"id":run_id,"itch_id":id,"project":row["project"],"version":1,"created_at":now,"updated_at":now,"scope_revision":scope(&row),"base":{"title":row["title"]},"question":q,"outline":outline,"suggested_tools":[],"state":"prepared","reports":[],"send":null,"adopted":[],"brief":brief});
    let request_key = text(args, "request_key", 128, false)?;
    let fingerprint =
        json!({"id":id,"question":q,"expected_version":args["expected_version"]}).to_string();
    let doc = state.runtime.update_document(DOCUMENT, |doc| {
        if !doc.is_object() {
            *doc = json!({})
        }
        if !doc["runs"].is_array() {
            doc["runs"] = json!([])
        }
        if !request_key.is_empty() {
            if let Some(existing) = doc["runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["request_key"] == request_key)
            {
                if existing["itch_id"] != id || existing["question"] != q {
                    return Err(LegacyError::conflict(
                        "Retry key belongs to a different research brief.",
                    ));
                }
                return Ok(());
            }
        }
        let mut r = run.clone();
        r["request_key"] = json!(request_key);
        r["fingerprint"] = json!(fingerprint);
        let runs = doc["runs"].as_array_mut().unwrap();
        if runs.len() >= 500 {
            return Err(LegacyError::invalid("Research storage is full."));
        }
        runs.push(r);
        Ok(())
    })?;
    let saved = if request_key.is_empty() {
        run_id.as_str()
    } else {
        doc["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["request_key"] == request_key)
            .and_then(|r| r["id"].as_str())
            .unwrap_or(&run_id)
    };
    pack(state, id, saved)
}
fn send(state: &AppState, args: &Value, expected_target: &str) -> LegacyResult<Value> {
    fields(args, &["id", "run_id", "thread_id", "expected_version"])?;
    let id = text(args, "id", 100, true)?;
    let run_id = text(args, "run_id", 100, true)?;
    let thread = text(args, "thread_id", 200, true)?;
    let key = text(args, "request_key", 100, true)?;
    let bare = thread.strip_prefix("mcp:").unwrap_or(thread);
    let parsed = uuid::Uuid::parse_str(bare)
        .map_err(|_| LegacyError::invalid("Choose an exact T3 thread ID."))?;
    if parsed.hyphenated().to_string() != bare.to_ascii_lowercase() {
        return Err(LegacyError::invalid("Choose an exact T3 thread ID."));
    }
    let version = args["expected_version"]
        .as_u64()
        .ok_or_else(|| LegacyError::invalid("Supply research version."))?;
    let distributions = crate::services::t3::handle(
        state,
        "/api/call",
        &json!({"name":"t3.distributions","args":{}}),
    )
    .ok_or_else(|| LegacyError::unavailable("T3 is unavailable."))??;
    let current = distributions["target"].as_str().unwrap_or("");
    if current.is_empty() || current == "t3@unconfigured" {
        return Err(LegacyError::unavailable(
            "Configure T3 before sending research.",
        ));
    }
    if !expected_target.is_empty() && expected_target != current {
        return Err(LegacyError::conflict(
            "The T3 installation changed. Refresh before sending.",
        ));
    }
    let row = itch(state, id)?;
    let saved = state.runtime.update_document(DOCUMENT, |doc| {
        let run = doc["runs"].as_array_mut().and_then(|a| a.iter_mut().find(|r| r["id"] == run_id && r["itch_id"] == id)).ok_or_else(|| LegacyError::not_found("Research run not found."))?;
        if run["send"].is_object() {
            if run["send"]["thread_id"] != thread || run["send"]["request_key"] != key || run["send"]["target"] != current { return Err(LegacyError::conflict("This run is frozen to a different send, thread or T3 installation.")); }
        } else {
            if run["version"].as_u64() != Some(version) || run["scope_revision"] != scope(&row) { return Err(LegacyError::conflict("Research changed. Refresh before sending.")); }
            if run["state"] != "prepared" { return Err(LegacyError::conflict("This research run is no longer ready to send.")); }
            let stable = format!("research-{}", run_id);
            run["send"] = json!({"thread_id":thread,"request_key":key,"target":current,"transport_key":stable,"receipt":null});
            run["state"] = json!("send_unconfirmed");
            run["version"] = json!(version + 1);
            doc["thread_id"] = json!(thread);
        }
        Ok(())
    })?;
    let run = selected(&saved, id, run_id).unwrap();
    if run["send"]["receipt"].is_object() {
        return pack(state, id, run_id);
    }
    let receipt = crate::services::t3::send_message(
        state,
        &json!({"thread_id":thread,"message":run["brief"],"request_key":run["send"]["transport_key"]}),
        current,
    )?;
    state.runtime.update_document(DOCUMENT, |doc| {
        let run = doc["runs"]
            .as_array_mut()
            .and_then(|a| {
                a.iter_mut()
                    .find(|r| r["id"] == run_id && r["itch_id"] == id)
            })
            .ok_or_else(|| LegacyError::not_found("Research run not found."))?;
        if run["send"]["thread_id"] != thread
            || run["send"]["target"] != current
            || run["send"]["request_key"] != key
        {
            return Err(LegacyError::conflict("Research send identity changed."));
        }
        let mut receipt = receipt.clone();
        receipt["thread_url"] = json!(format!("t3-thread://v1/{thread}"));
        run["send"]["receipt"] = receipt;
        if run["state"] == "send_unconfirmed" {
            run["state"] = json!("awaiting_report");
        }
        Ok(())
    })?;
    pack(state, id, run_id)
}
fn list<'a>(v: &'a Value, key: &str, max: usize) -> LegacyResult<&'a Vec<Value>> {
    v.get(key)
        .and_then(Value::as_array)
        .filter(|a| a.len() <= max)
        .ok_or_else(|| LegacyError::invalid("A report list is missing or too long."))
}
fn report(raw: &Value) -> LegacyResult<Value> {
    let parsed: Value = if let Some(s) = raw.as_str() {
        serde_json::from_str(s).map_err(|_| LegacyError::invalid("Report must be JSON."))?
    } else {
        raw.clone()
    };
    fields(
        &parsed,
        &[
            "summary",
            "sources",
            "findings",
            "plan",
            "tools",
            "open_questions",
            "limitations",
            "tool_ideas",
        ],
    )?;
    if parsed.to_string().len() > 24000 {
        return Err(LegacyError::invalid(
            "Keep the report under 24000 characters.",
        ));
    }
    fn safe(v: &Value) -> LegacyResult<()> {
        match v {
            Value::String(s) => safe_prompt(s, ""),
            Value::Array(a) => {
                for item in a {
                    safe(item)?;
                }
                Ok(())
            }
            Value::Object(o) => {
                for (k, v) in o {
                    if [
                        "api_key",
                        "access_token",
                        "authorization",
                        "cookie",
                        "password",
                        "secret",
                    ]
                    .contains(&k.to_ascii_lowercase().as_str())
                    {
                        return Err(LegacyError::invalid(
                            "Remove credentials from research reports.",
                        ));
                    }
                    safe(v)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    safe(&parsed)?;
    text(&parsed, "summary", 3000, true)?;
    let empty = Vec::new();
    let mut normalized = parsed.clone();
    for key in [
        "sources",
        "findings",
        "plan",
        "tools",
        "open_questions",
        "limitations",
        "tool_ideas",
    ] {
        if normalized.get(key).is_none() {
            normalized[key] = json!([])
        }
    }
    let sources = list(&normalized, "sources", 12)?;
    let mut ids = std::collections::HashSet::new();
    for s in sources {
        let id = text(s, "id", 50, true)?;
        if !ids.insert(id.to_owned()) {
            return Err(LegacyError::invalid("Use distinct source IDs."));
        }
        text(s, "title", 240, false)?;
        let url = text(s, "url", 1500, false)?;
        let path = text(s, "path", 500, false)?;
        text(s, "excerpt", 3000, true)?;
        if url.is_empty() == path.is_empty() {
            return Err(LegacyError::invalid(
                "Each source needs one URL or relative path.",
            ));
        }
        if !url.is_empty() {
            let u =
                url::Url::parse(url).map_err(|_| LegacyError::invalid("Invalid source URL."))?;
            if !["https", "http"].contains(&u.scheme())
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
            {
                return Err(LegacyError::invalid(
                    "Use HTTP(S) source URLs without credentials.",
                ));
            }
        } else if path.starts_with(['/', '\\'])
            || path.contains(':')
            || path.replace('\\', "/").split('/').any(|p| p == "..")
        {
            return Err(LegacyError::invalid("Use a project-relative source path."));
        }
    }
    for f in list(&normalized, "findings", 16)? {
        let kind = text(f, "kind", 20, true)?;
        if !["observed", "inference", "question"].contains(&kind) {
            return Err(LegacyError::invalid(
                "Use observed, inference or question findings.",
            ));
        }
        text(f, "text", 1800, true)?;
        let citations = f
            .get("citations")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        if citations.len() > 6 || (kind != "question" && citations.is_empty()) {
            return Err(LegacyError::invalid(
                "Observed findings and inferences require citations.",
            ));
        }
        for c in citations {
            let id = text(c, "source_id", 50, true)?;
            let quote = text(c, "quote", 1500, true)?;
            if !sources
                .iter()
                .any(|s| s["id"] == id && s["excerpt"].as_str().is_some_and(|e| e.contains(quote)))
            {
                return Err(LegacyError::invalid(
                    "Citations must quote an excerpt in this report exactly.",
                ));
            }
        }
    }
    let mut steps = std::collections::HashSet::new();
    for p in list(&normalized, "plan", 12)? {
        let id = text(p, "id", 40, true)?;
        text(p, "text", 1000, true)?;
        if !steps.insert(id) {
            return Err(LegacyError::invalid("Plan IDs must be unique."));
        }
    }
    // The native app cannot verify old DumbAPI capabilities; do not claim them.
    if !list(&normalized, "tools", 8)?.is_empty() {
        return Err(LegacyError::invalid(
            "Native tool recommendations are not available; use tool_ideas.",
        ));
    }
    for (key, limit) in [("open_questions", 8), ("limitations", 8), ("tool_ideas", 4)] {
        if list(&normalized, key, limit)?
            .iter()
            .any(|v| v.as_str().is_none_or(|s| s.len() > 1000))
        {
            return Err(LegacyError::invalid("Report notes must be bounded text."));
        }
    }
    Ok(normalized)
}
fn record(state: &AppState, args: &Value) -> LegacyResult<Value> {
    fields(args, &["id", "run_id", "expected_version", "report"])?;
    let id = text(args, "id", 100, true)?;
    let run = text(args, "run_id", 100, true)?;
    let version = args["expected_version"]
        .as_u64()
        .ok_or_else(|| LegacyError::invalid("Supply the research version."))?;
    let mut value = report(
        args.get("report")
            .ok_or_else(|| LegacyError::invalid("Supply a report."))?,
    )?;
    let key = text(args, "request_key", 128, false)?;
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            json!({"id":id,"run_id":run,"version":version,"report":value})
                .to_string()
                .as_bytes()
        )
    );
    value["request_key"] = json!(key);
    value["fingerprint"] = json!(fingerprint);
    value["id"] = json!(uuid::Uuid::new_v4().simple().to_string());
    value["at"] = json!(dumbmanager_core::time::now_iso());
    value["provenance"] = json!("agent_reported");
    state.runtime.update_document(DOCUMENT, |doc| {
        let row = doc["runs"]
            .as_array_mut()
            .and_then(|a| a.iter_mut().find(|r| r["id"] == run && r["itch_id"] == id))
            .ok_or_else(|| LegacyError::not_found("Research run not found."))?;
        if !key.is_empty() {
            if let Some(old) = row["reports"]
                .as_array()
                .and_then(|a| a.iter().find(|r| r["request_key"] == key))
            {
                if old["fingerprint"] != fingerprint {
                    return Err(LegacyError::conflict(
                        "Retry key belongs to a different report.",
                    ));
                }
                return Ok(());
            }
        }
        if row["version"].as_u64() != Some(version) {
            return Err(LegacyError::conflict(
                "Research changed. Refresh before recording.",
            ));
        }
        let reports = row["reports"]
            .as_array_mut()
            .ok_or_else(|| LegacyError::invalid("Invalid research record."))?;
        if reports.len() >= 12 {
            return Err(LegacyError::invalid("Prepare a new research pass."));
        }
        reports.push(value);
        row["state"] = json!("reported");
        row["version"] = json!(version + 1);
        row["updated_at"] = json!(dumbmanager_core::time::now_iso());
        Ok(())
    })?;
    pack(state, id, run)
}
fn adopt(state: &AppState, args: &Value) -> LegacyResult<Value> {
    fields(
        args,
        &[
            "id",
            "run_id",
            "report_id",
            "step_ids",
            "expected_version",
            "itch_version",
        ],
    )?;
    let id = text(args, "id", 100, true)?;
    let run_id = text(args, "run_id", 100, true)?;
    let report_id = text(args, "report_id", 100, true)?;
    let ids: Vec<String> = serde_json::from_str(text(args, "step_ids", 1000, true)?)
        .map_err(|_| LegacyError::invalid("Choose saved research steps."))?;
    if ids.is_empty()
        || ids.len() > 12
        || ids.iter().any(|s| s.len() > 40)
        || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
    {
        return Err(LegacyError::invalid("Choose unique saved research steps."));
    }
    let expected = args["expected_version"]
        .as_u64()
        .ok_or_else(|| LegacyError::invalid("Supply research version."))?;
    let itch_version = args["itch_version"]
        .as_u64()
        .ok_or_else(|| LegacyError::invalid("Supply itch version."))?;
    let _guard = ADOPT
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let doc = state.runtime.read_document(DOCUMENT)?;
    let run = selected(&doc, id, run_id)
        .ok_or_else(|| LegacyError::not_found("Research run not found."))?;
    let row = itch(state, id)?;
    if run["version"].as_u64() != Some(expected) || run["scope_revision"] != scope(&row) {
        return Err(LegacyError::conflict(
            "Research or itch changed. Refresh before adopting steps.",
        ));
    }
    if ["resolved", "archived", "suggested", "declined"]
        .contains(&row["state"].as_str().unwrap_or(""))
    {
        return Err(LegacyError::invalid(
            "Reopen or approve this itch before adopting steps.",
        ));
    }
    let report = run["reports"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["id"] == report_id))
        .ok_or_else(|| LegacyError::not_found("Research report not found."))?;
    let choices = report["plan"]
        .as_array()
        .ok_or_else(|| LegacyError::invalid("Report has no proposed plan."))?;
    let chosen: Vec<Value> = ids
        .iter()
        .map(|id| {
            choices
                .iter()
                .find(|p| p["id"] == *id)
                .cloned()
                .ok_or_else(|| LegacyError::invalid("Choose steps from this report."))
        })
        .collect::<LegacyResult<_>>()?;
    let mut plan = row["plan"].as_array().cloned().unwrap_or_default();
    let identities: Vec<String> = ids.iter().map(|s| format!("{report_id}:{s}")).collect();
    let already = identities.iter().all(|identity| {
        plan.iter()
            .any(|p| p["id"] == format!("research:{identity}"))
    });
    if row["version"].as_u64() != Some(itch_version)
        && !(already && row["version"].as_u64() == Some(itch_version + 1))
    {
        return Err(LegacyError::conflict(
            "This itch changed. Refresh before adopting steps.",
        ));
    }
    if !already {
        for p in chosen {
            let identity = format!("research:{report_id}:{}", p["id"].as_str().unwrap());
            if !plan.iter().any(|x| x["id"] == identity) {
                plan.push(json!({"id":identity,"text":p["text"],"done":false}));
            }
        }
        let mut changes = json!({"plan":plan});
        if row["state"] == "ready" || row["run_state"] == "agent_reported_done" {
            changes["state"] = json!("open");
            changes["run_state"] = json!("not_sent");
        }
        state.store().legacy_call(
            "manager.update",
            &json!({"id":id,"expected_version":itch_version,"changes":changes}),
        )?;
    }
    state.runtime.update_document(DOCUMENT, |doc| {
        let run = doc["runs"]
            .as_array_mut()
            .and_then(|a| {
                a.iter_mut()
                    .find(|r| r["id"] == run_id && r["itch_id"] == id)
            })
            .ok_or_else(|| LegacyError::not_found("Research run not found."))?;
        if run["version"].as_u64() != Some(expected) {
            return Err(LegacyError::conflict("Research changed; refresh."));
        }
        let adopted = run["adopted"]
            .as_array_mut()
            .ok_or_else(|| LegacyError::invalid("Invalid research record."))?;
        for identity in identities {
            if !adopted.iter().any(|v| v == &identity) {
                adopted.push(json!(identity));
            }
        }
        run["version"] = json!(expected + 1);
        run["updated_at"] = json!(dumbmanager_core::time::now_iso());
        Ok(())
    })?;
    pack(state, id, run_id)
}
pub(super) fn handle(
    state: &AppState,
    name: &str,
    args: &Value,
    target: &str,
) -> Option<LegacyResult<Value>> {
    Some(match name {
        "manager.research.get" => {
            let id = match text(args, "id", 100, true) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let run = match text(args, "run_id", 100, false) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let key = match text(args, "request_key", 128, false) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if run.is_empty() && !key.is_empty() {
                let doc = match state.runtime.read_document(DOCUMENT) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                let found = doc["runs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|r| r["itch_id"] == id && r["request_key"] == key);
                if let Some(found) = found {
                    pack(state, id, found["id"].as_str().unwrap_or(""))
                } else {
                    pack(state, id, "").map(|mut p| {
                        p["run"] = Value::Null;
                        p
                    })
                }
            } else {
                pack(state, id, run)
            }
        }
        "manager.research.prepare" => prepare(state, args),
        "manager.research.send" => send(state, args, target),
        "manager.research.record" => record(state, args),
        "manager.research.adopt" => adopt(state, args),
        _ => return None,
    })
}
