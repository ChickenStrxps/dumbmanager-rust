//! General guides shown only beside a literal mention in saved work.
use super::{digest, safe};
use serde_json::{json, Value};

fn mentions(text: &str, key: &str) -> bool {
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .collect();
    match key {
        "node" => words.contains(&"node"),
        "gif" => words.iter().any(|w| matches!(*w, "gif" | "gifs")),
        "idempotency" => words
            .iter()
            .any(|w| matches!(*w, "idempotency" | "idempotence" | "idempotent")),
        _ => words.contains(&key),
    }
}
const TOPICS: &[(&str, &str, &str, &str, &str, &str, &str)] = &[
    ("unittest", "Python unittest", "Discovery collects test modules from a directory. A passing suite covers those checks; it does not prove every user flow works.", "Find one test for this change. Read its setup, action and assertion before changing anything.", "Why might the tests pass while a button still feels broken?", "The test may cover the domain logic without exercising the browser interaction.", "https://docs.python.org/3/library/unittest.html"),
    ("node", "Node syntax checks", "node --check parses a script without executing it. Syntax success does not exercise events, layout or network requests.", "Compare the saved syntax check with the separate browser evidence for this itch.", "Does a syntax check prove a click handler works?", "No. It checks parsing; an interaction needs its own check.", "https://nodejs.org/api/cli.html#-c---check"),
    ("playwright", "Playwright", "Locators find controls and can wait for them to become actionable. Prefer a visible role or label over a fragile screen coordinate.", "Find the control this itch changed and describe it by its visible label.", "What tends to survive a layout change: a button label or a screen coordinate?", "The label usually survives. A coordinate can point somewhere else after a resize.", "https://playwright.dev/docs/locators"),
    ("sqlite", "SQLite transactions", "A transaction groups related writes so they commit together. A saved UI response and a durable database write are different things.", "Identify which part of this result should still be true after reload.", "What does a reload check add to a successful save response?", "It checks that the application can read the persisted result again.", "https://www.sqlite.org/lang_transaction.html"),
    ("gif", "Looping GIFs", "A GIF contains timed frames. A seamless loop needs the last-to-first transition to fit the motion, as well as a smooth sequence within the file.", "Watch the recorded animation through two loops. Look at the seam and any text that should remain still.", "Where can a loop jump even when all the middle frames look smooth?", "At the transition from the final frame back to the first.", "https://www.w3.org/Graphics/GIF/spec-gif89a.txt"),
    ("idempotency", "Retry identity", "A stable request key lets a retry refer to the same action. Reusing it with different content should be rejected rather than silently doing new work.", "Trace the saved retry evidence: what stayed the same, and what happened only once?", "Should a lost response automatically mean repeating the external action?", "No. First recover the receipt for that request; the original action may already have succeeded.", ""),
 ];
fn lesson(
    key: &str,
    title: &str,
    explanation: String,
    exercise: &str,
    question: &str,
    answer: &str,
    ids: Value,
) -> Value {
    let mut value = json!({"title":title,"explanation":explanation,"try_this":exercise,"question":question,"answer":answer,"source_ids":ids});
    value["id"] = json!(format!("{key}-{}", digest(&value)));
    value
}
fn add(sources: &mut Vec<Value>, id: String, label: &str, text: &str, kind: &str) {
    let text = safe(text);
    if !text.trim().is_empty() {
        sources.push(json!({"id":id,"label":label,"text":text,"kind":kind}));
    }
}
fn sanitized(value: &Value) -> Value {
    match value {
        Value::String(s) => json!(safe(s)),
        Value::Array(a) => Value::Array(a.iter().take(32).map(sanitized).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .take(32)
                .map(|(k, v)| {
                    (
                        k.clone(),
                        if matches!(
                            k.to_lowercase().as_str(),
                            "api_key"
                                | "api-key"
                                | "token"
                                | "access_token"
                                | "authorization"
                                | "cookie"
                                | "password"
                                | "secret"
                        ) {
                            json!("[Credential omitted]")
                        } else {
                            sanitized(v)
                        },
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}
pub(super) fn build(row: &Value, research: &Value, doc: &Value) -> Value {
    let id = row["id"].as_str().unwrap_or("");
    let state = row["state"].as_str().unwrap_or("");
    let eligible = (["ready", "resolved"].contains(&state)
        || row["run_state"] == "agent_reported_done")
        && !matches!(state, "archived" | "needs_description");
    let mut sources = Vec::new();
    add(
        &mut sources,
        "observation".into(),
        "Original request",
        row["original"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| row["title"].as_str())
            .unwrap_or(""),
        "request",
    );
    add(
        &mut sources,
        "goal".into(),
        "Saved desired outcome",
        row["desired_outcome"].as_str().unwrap_or(""),
        "request",
    );
    for key in ["workspace", "task", "testing"] {
        add(
            &mut sources,
            format!("context:{key}"),
            &format!("Context at capture ? {key}"),
            row["context"][key].as_str().unwrap_or(""),
            "context",
        );
    }
    let mut work = Vec::new();
    for p in row["plan"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["done"] == true)
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        add(
            &mut work,
            format!("plan:{}", p["id"].as_str().unwrap_or("")),
            "Reported completed step",
            p["text"].as_str().unwrap_or(""),
            "report",
        );
    }
    for (i, e) in row["evidence"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let text = e
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| sanitized(e).to_string());
        add(
            &mut work,
            format!("evidence:{i}"),
            "Saved evidence",
            &text,
            "report",
        );
        for (key, label, kind) in [
            ("summary", "Saved result summary", "report"),
            (
                "workspace",
                "Workspace reported for this work",
                "environment",
            ),
            (
                "environment",
                "Environment reported for this work",
                "environment",
            ),
        ] {
            add(
                &mut work,
                format!("evidence:{i}:{key}"),
                label,
                e[key].as_str().unwrap_or(""),
                kind,
            );
        }
        for (j, c) in e["commands"]
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
            .enumerate()
        {
            if let Some(s) = c.as_str() {
                add(
                    &mut work,
                    format!("evidence:{i}:command:{j}"),
                    "Reported command",
                    s,
                    "command",
                );
            }
        }
    }
    // Research remains explicitly reported evidence, not a verified source scan.
    let mut excerpts = Vec::new();
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
                let text = safe(source["excerpt"].as_str().unwrap_or(""));
                if !text.is_empty() && text != "[Credential-bearing text omitted]" {
                    excerpts.push(json!({"id":format!("research:{}:{}",report["id"].as_str().unwrap_or(""),source["id"].as_str().unwrap_or("")),"label":format!("Research report source: {}",source["title"].as_str().unwrap_or("Untitled")),"text":text,"kind":"research","url":source["url"],"path":source["path"],"provenance":"Reported excerpt; not independently verified."}));
                }
            }
        }
    }
    // Reserve room for recent commands/environment as well as research.
    let keep = work.len().saturating_sub(24);
    sources.extend(work.into_iter().skip(keep));
    sources.extend(excerpts.into_iter().take(12));
    let recent = |key: &str| {
        row[key]
            .as_array()
            .and_then(|a| a.last())
            .cloned()
            .unwrap_or(Value::Null)
    };
    let work_basis = digest(&json!([
        1,
        id,
        sources,
        recent("occurrences"),
        recent("feedback")
    ]));
    for (i, o) in row["outcomes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        add(
            &mut sources,
            format!("outcome:{i}"),
            "Accepted outcome",
            o["summary"].as_str().unwrap_or(""),
            "report",
        );
        if !o["evidence"].is_null() {
            add(
                &mut sources,
                format!("outcome:{i}:evidence"),
                "Accepted outcome evidence",
                &sanitized(&o["evidence"]).to_string(),
                "report",
            );
        }
    }
    let basis = digest(&json!([
        1,
        id,
        sources,
        state == "resolved",
        eligible,
        recent("occurrences"),
        recent("feedback")
    ]));
    let saved = &doc["notes"][id];
    let stale = saved.is_object() && saved["basis"] != work_basis;
    let notes = if stale {
        Value::Null
    } else {
        saved["notes"].clone()
    };
    let mut lessons = Vec::new();
    let mut commands = Vec::new();
    let mut packages = Vec::new();
    if eligible {
        let reports: Vec<_> = sources.iter().filter(|s| s["kind"] == "report").collect();
        let step = reports
            .iter()
            .find(|s| {
                s["id"].as_str().is_some_and(|id| id.starts_with("plan:"))
                    && !s["text"]
                        .as_str()
                        .unwrap_or("")
                        .split_whitespace()
                        .next()
                        .is_some_and(|w| {
                            ["recheck", "verify", "test", "review", "inspect", "research"]
                                .contains(&w.to_lowercase().as_str())
                        })
            })
            .copied()
            .or_else(|| {
                reports
                    .iter()
                    .copied()
                    .find(|s| s["id"].as_str().is_some_and(|id| id.starts_with("plan:")))
            })
            .or_else(|| {
                reports
                    .iter()
                    .rev()
                    .copied()
                    .find(|s| !s["id"].as_str().unwrap_or("").starts_with("outcome:"))
            })
            .or_else(|| reports.last().copied());
        if let Some(s) = step {
            lessons.push(lesson("change","Map the change",format!("Start with the request, then connect it to this reported result:\n\n{}",s["text"].as_str().unwrap_or("")),"Explain the before and after in one sentence. Point to the completed step or saved result that supports it.","Does the original request show what was actually implemented?","It describes the intent. Use completed steps and result evidence to explain what was reported as implemented.",json!(["observation",s["id"]])));
        }
        if let Some(s) = reports
            .iter()
            .rev()
            .find(|s| !s["id"].as_str().unwrap_or("").starts_with("plan:"))
        {
            lessons.push(lesson("evidence","Read the evidence",format!("The record reports:\n\n{}\n\nA report is a starting point for review, not a new verification run.",s["text"].as_str().unwrap_or("")),"Choose one reported check. Say what it covers and one thing you would still check by hand in this environment.","Does opening this learning trail rerun the checks?","No. These are saved reports. No commands or tests run when you open a lesson.",json!([s["id"]])));
        }
        for entry in notes["lessons"].as_array().into_iter().flatten().take(6) {
            lessons.push(lesson(
                "note",
                entry["title"].as_str().unwrap_or(""),
                entry["explanation"].as_str().unwrap_or("").into(),
                entry["try_this"].as_str().unwrap_or(""),
                entry["question"].as_str().unwrap_or(""),
                entry["answer"].as_str().unwrap_or(""),
                json!([entry["source_id"]]),
            ));
        }
        for &(key, title, tip, exercise, question, answer, docs) in TOPICS {
            if let Some(s) = sources.iter().rev().find(|s| {
                ["report", "command", "research"].contains(&s["kind"].as_str().unwrap_or(""))
                    && mentions(&s["text"].as_str().unwrap_or("").to_lowercase(), key)
            }) {
                packages.push(json!({"name":title,"tip":tip,"docs":docs,"source_ids":[s["id"]],"kind":"Mentioned in saved work"}));
                lessons.push(lesson(
                    key,
                    title,
                    tip.into(),
                    exercise,
                    question,
                    answer,
                    json!([s["id"]]),
                ));
            }
        }
        if lessons.is_empty() {
            lessons.push(lesson("review","Start with the original itch","This itch is marked ready or accepted, but no completed steps or result notes were saved. There is not enough history to explain the implementation.".into(),"Compare the current result with the original request. Ask the working agent to save what changed, the checks, and the commands actually used.","Can a title alone tell you which package or command was used?","No. That needs recorded work. This trail leaves those gaps visible.",json!(["observation"])));
        }
        for entry in notes["commands"].as_array().into_iter().flatten().take(6) {
            let mut c = entry.clone();
            c["source_ids"] = json!([entry["source_id"]]);
            c["kind"] = json!("Recorded command");
            commands.push(c);
        }
        for s in sources
            .iter()
            .filter(|s| s["kind"] == "command" && s["text"] != "[Credential-bearing text omitted]")
        {
            if !commands.iter().any(|c| c["command"] == s["text"]) {
                commands.push(json!({"command":s["text"],"explanation":"Saved in the work report. Review arguments and workspace before reuse.","caution":"Reported history; this view does not run or verify it.","source_ids":[s["id"]],"kind":"Recorded command"}));
            }
        }
        for entry in notes["packages"].as_array().into_iter().flatten().take(6) {
            let mut p = entry.clone();
            p["source_ids"] = json!([entry["source_id"]]);
            p["kind"] = json!("Agent-written package tip");
            p["docs"] = json!("");
            packages.push(p);
        }
    }
    lessons.truncate(16);
    commands.truncate(8);
    packages.truncate(12);
    for l in &mut lessons {
        let supporting: Vec<_> = l["source_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| sources.iter().find(|s| s["id"] == *id))
            .collect();
        l["id"] = json!(format!(
            "{id}-{}-{}",
            l["id"].as_str().unwrap_or(""),
            digest(&json!(supporting))
        ));
    }
    let progress = &doc["progress"][id];
    let read: Vec<_> = progress["read"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| lessons.iter().any(|l| l["id"] == **v))
        .cloned()
        .collect();
    let workspace = sources.iter().rev().find(|s| {
        s["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("evidence:") && id.ends_with(":workspace"))
    });
    let actual_workspace = workspace.is_some();
    let workspace = workspace.or_else(|| sources.iter().find(|s| s["id"] == "context:workspace"));
    let environment = sources
        .iter()
        .rev()
        .find(|s| s["id"].as_str().unwrap_or("").ends_with(":environment"));
    json!({"id":id,"title":safe(row["title"].as_str().unwrap_or("")),"project":row["project"],"eligible":eligible,"stage":if state=="resolved"{"Accepted by you"}else if eligible{"Ready to test"}else{"Work not finished"},"revision":digest(&json!([basis,notes,1])),"source_revision":basis,"work_revision":work_basis,"itch_version":row["version"],"workspace":workspace.map(|s|s["text"].clone()).unwrap_or(json!("")),"environment":environment.map(|s|s["text"].clone()).unwrap_or(json!("")),"workspace_note":if actual_workspace{"Reported workspace for this work; not a live environment scan."}else if workspace.is_some(){"Snapshot saved at capture; not a live environment scan."}else{"No workspace was saved with this itch."},"lessons":lessons,"commands":commands,"packages":packages,"sources":sources,"notes_stale":stale,"notes_version":saved["version"].as_u64().unwrap_or(0),"progress":{"version":progress["version"].as_u64().unwrap_or(0),"read":read,"total":lessons.len()},"coverage":"Built locally from recent saved work. Reports are not rerun. Tool explanations are general guidance; package installation and versions are not inferred."})
}
