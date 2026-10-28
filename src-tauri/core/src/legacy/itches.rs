use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use super::{decode, get_context, input::Args, setting, LegacyError, LegacyResult};
use crate::store::{load_itch, next_revision, record_event};
use crate::{time, validate, CoreResult, ItchStatus};

pub(super) fn title_key(title: &str) -> String {
    let mut key = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            key.push(c);
        } else if !key.is_empty() && !key.ends_with('.') {
            key.push('.');
        }
        if key.len() >= 100 {
            break;
        }
    }
    let key = key.trim_end_matches('.');
    if key.is_empty() {
        "moment".to_owned()
    } else {
        key.to_owned()
    }
}

/// The exact original state is always persisted separately. Core Open is the
/// closest coarse bucket for working; archived/unfinished moments are stashed.
pub(super) fn core_status(state: &str) -> ItchStatus {
    match state {
        "working" => ItchStatus::Open,
        "ready" => ItchStatus::Ready,
        "resolved" => ItchStatus::Resolved,
        _ => ItchStatus::Stashed,
    }
}

pub(super) fn get(conn: &Connection, id: &str) -> LegacyResult<Value> {
    let id = validate::itch_id(id)?;
    let raw: Option<String> = conn
        .query_row("SELECT data FROM legacy_itches WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(decode(&raw.ok_or_else(|| {
        LegacyError::not_found("This local itch no longer exists. Reload the list.")
    })?)?)
}

pub(super) fn records(conn: &Connection, limit: Option<i64>) -> CoreResult<Vec<Value>> {
    let raw=conn.prepare("SELECT legacy_itches.data FROM legacy_itches JOIN itches USING(id) ORDER BY itches.revision DESC LIMIT ?1")?
        .query_map([limit.unwrap_or(-1)],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
    raw.iter().map(|row| decode(row)).collect()
}

pub(super) fn history(conn: &Connection, id: &str) -> LegacyResult<Value> {
    let row = get(conn, id)?;
    let mut statement=conn.prepare("SELECT version,at,action,changes FROM legacy_events WHERE itch_id=?1 ORDER BY seq DESC LIMIT 500")?;
    let raw = statement
        .query_map([row["id"].as_str().unwrap_or("")], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut events = Vec::new();
    for (version, at, action, changes) in raw {
        events.push(json!({"version":version,"at":at,"action":action,"changes":decode(&changes)?}));
    }
    Ok(
        json!({"items":events,"coverage":"Most recent 500 compatibility events. native_snapshot is a migration baseline, not reconstructed earlier history."}),
    )
}

fn array_mut<'a>(row: &'a mut Value, key: &str) -> LegacyResult<&'a mut Vec<Value>> {
    row[key]
        .as_array_mut()
        .ok_or_else(|| LegacyError::new("A saved local list is unreadable.", "storage", 500))
}

/// All core and original fields plus the audit event commit in one transaction.
/// No synthetic Ready transition is inserted when the original UI explicitly
/// accepts an open itch: that is a real acceptance, allowed by its contract.
fn put(conn: &Connection, row: &Value, action: &str, changes: &Value) -> LegacyResult<()> {
    if row.to_string().len() > 131_072 {
        return Err(LegacyError::invalid(
            "This itch is over the local record size limit. Export it before adding more history.",
        ));
    }
    let id = row["id"]
        .as_str()
        .ok_or_else(|| LegacyError::invalid("Missing local itch ID."))?;
    let previous = load_itch(conn, id)?;
    let state = row["state"]
        .as_str()
        .ok_or_else(|| LegacyError::invalid("Missing local itch state."))?;
    let status = core_status(state);
    let now = row["updated_at"]
        .as_str()
        .ok_or_else(|| LegacyError::invalid("Missing local timestamp."))?;
    let revision = next_revision(conn)?;
    if let Some(previous) = previous {
        conn.execute(
            "UPDATE itches SET title=?2,project=?3,status=?4,updated_at=?5,revision=?6 WHERE id=?1",
            params![
                id,
                row["title"].as_str(),
                row["project"].as_str(),
                status.as_str(),
                now,
                revision
            ],
        )?;
        if previous.status != status {
            record_event(conn, id, now, "status", Some(previous.status), Some(status))?;
        } else {
            record_event(conn, id, now, "edited", None, None)?;
        }
    } else {
        // The first core model requires a kind, whereas original captures have
        // none. `idea` is an internal storage default, never reported as an
        // observed classification in the original contract.
        conn.execute("INSERT INTO itches(id,title,description,project,kind,status,created_at,updated_at,revision)
            VALUES(?1,?2,?3,?4,'idea',?5,?6,?7,?8)",
            params![id,row["title"].as_str(),row["original"].as_str(),row["project"].as_str(),status.as_str(),row["created_at"].as_str(),now,revision])?;
        record_event(conn, id, now, "created", None, Some(status))?;
    }
    conn.execute("INSERT INTO legacy_itches(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![id,row.to_string()])?;
    conn.execute(
        "INSERT INTO legacy_events(itch_id,version,at,action,changes) VALUES(?1,?2,?3,?4,?5)",
        params![
            id,
            row["version"].as_i64(),
            now,
            action,
            changes.to_string()
        ],
    )?;
    Ok(())
}

pub(super) fn capture(
    conn: &Connection,
    args: &Args<'_>,
    media: Option<&[Value]>,
) -> LegacyResult<Value> {
    let title = args.text("title", 500)?;
    let original = args.text("original", 2000)?;
    let moment = args.boolean("moment", false)?;
    let active = setting(conn, "legacy_active_context")?;
    let cid = args.text("context_id", 100)?;
    let cid = if cid.is_empty() && args.boolean("use_active", true)? {
        active.as_str().unwrap_or("")
    } else {
        cid
    };
    // Always validate the boolean, even when an explicit context was supplied.
    args.boolean("use_active", true)?;
    let mut context = if cid.is_empty() {
        Value::Null
    } else {
        get_context(conn, cid)?
    };
    let snapshot = args.json("context_snapshot", json!({}), 22_000)?;
    let snapshot_obj = snapshot
        .as_object()
        .ok_or_else(|| LegacyError::invalid("Expected a context snapshot object."))?;
    if !snapshot_obj.is_empty() {
        let snapshot_args = Args::new(
            &snapshot,
            &[
                "id",
                "project",
                "thread_id",
                "task",
                "testing",
                "entities",
                "candidates",
                "workspace",
                "version",
                "updated_at",
                "source",
            ],
        )?;
        if context.is_null()
            || snapshot["id"] != context["id"]
            || snapshot["project"] != context["project"]
            || snapshot["thread_id"] != context["thread_id"]
        {
            return Err(LegacyError::invalid(
                "The saved context snapshot does not match its project/thread.",
            ));
        }
        snapshot_args.integer("version", 0, 1, context["version"].as_i64().unwrap_or(0))?;
        for field in [
            "task",
            "testing",
            "entities",
            "workspace",
            "updated_at",
            "source",
        ] {
            snapshot_args.text(field, 2000)?;
        }
        let candidates = snapshot_args.json("candidates", json!([]), 10_000)?;
        super::input::text_array(&candidates, 32, 240)?;
        context = snapshot;
    }
    let project = args.text("project", 100)?;
    let project = if project.is_empty() {
        context["project"].as_str().unwrap_or("unsorted")
    } else {
        project
    };
    if !context.is_null() && context["project"] != project {
        return Err(LegacyError::invalid(
            "The context belongs to a different project. Select the correct context.",
        ));
    }
    let attachments = args.json("attachments", json!([]), 1000)?;
    let attachments = attachments
        .as_array()
        .ok_or_else(|| LegacyError::invalid("Expected an array of uploaded evidence IDs."))?;
    if !attachments.is_empty() && media.is_none() {
        return Err(LegacyError::unavailable("Evidence uploads are unavailable in this native build. This itch was not saved; keep the text and remove the unsupported attachment before retrying."));
    }
    let media = resolved_media(attachments, media.unwrap_or(&[]))?;
    if title.is_empty() && !moment {
        return Err(LegacyError::invalid("Type an itch or mark a moment."));
    }
    let event = args.text("event_at", 40)?;
    if !event.is_empty() && !valid_timestamp(event) {
        return Err(LegacyError::invalid(
            "Use an ISO 8601 event timestamp with a timezone.",
        ));
    }
    let stamp = time::now_iso();
    let at = if event.is_empty() { &stamp } else { event };
    let original = if original.is_empty() { title } else { original };
    let row = json!({"id":uuid::Uuid::new_v4().hyphenated().to_string(),"project":project,
        "title":if title.is_empty(){"That moment…"}else{title},"attachments":media,
        "key":title_key(title),"state":if title.is_empty(){"needs_description"}else{"open"},
        "version":1,"original":original,"context_id":cid,"context":context,
        "created_at":stamp,"updated_at":stamp,"event_at":at,
        "occurrences":[{"at":at,"note":original}],"plan":[],"outcomes":[],"feedback":[],
        "evidence":[],"run_state":"not_sent","next_step":"","desired_outcome":"",
        "test_url":"","recheck":null});
    put(
        conn,
        &row,
        "capture",
        &json!({"moment":moment,"event_at":at}),
    )?;
    Ok(row)
}

fn resolved_media(ids: &[Value], media: &[Value]) -> LegacyResult<Vec<Value>> {
    if ids.len() > 8 || ids.len() != media.len() {
        return Err(LegacyError::invalid(
            "Resolve up to eight uploaded evidence IDs before saving.",
        ));
    }
    let mut unique = std::collections::HashSet::new();
    for (id, entry) in ids.iter().zip(media) {
        let id = id
            .as_str()
            .ok_or_else(|| LegacyError::invalid("Expected uploaded evidence IDs."))?;
        if !unique.insert(id)
            || entry["id"] != id
            || !entry.is_object()
            || entry.to_string().len() > 4000
            || entry.get("path").is_some()
            || entry.get("base64").is_some()
        {
            return Err(LegacyError::invalid("Invalid resolved evidence reference."));
        }
    }
    Ok(media.to_vec())
}

pub(super) fn attach(
    conn: &Connection,
    args: &Args<'_>,
    media: Option<&[Value]>,
) -> LegacyResult<Value> {
    let ids = args.json("attachments", json!([]), 1000)?;
    let ids = ids
        .as_array()
        .ok_or_else(|| LegacyError::invalid("Expected uploaded evidence IDs."))?;
    let added = resolved_media(
        ids,
        media.ok_or_else(|| {
            LegacyError::unavailable(
                "Resolve evidence through this installation before attaching it.",
            )
        })?,
    )?;
    let mut row = get(conn, args.required_text("id", 100)?)?;
    let attachments = array_mut(&mut row, "attachments")?;
    let mut changed = false;
    for item in added {
        if !attachments.iter().any(|a| a["id"] == item["id"]) {
            attachments.push(item);
            changed = true;
        }
    }
    if attachments.len() > 8 {
        return Err(LegacyError::invalid(
            "An itch can keep up to eight uploaded receipts.",
        ));
    }
    if changed {
        bump(&mut row);
        put(conn, &row, "attach", &json!({"ids":ids}))?;
    }
    Ok(row)
}

pub(super) fn record_handoff(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let mut row = get(conn, args.required_text("id", 100)?)?;
    let receipt = json!({"request_key":args.required_text("request_key",128)?,"thread_id":args.required_text("thread_id",200)?,"message_id":args.required_text("message_id",200)?,"at":args.required_text("at",60)?});
    if row.get("handoffs").is_none() {
        row["handoffs"] = json!([]);
    }
    let history = array_mut(&mut row, "handoffs")?;
    if let Some(old) = history
        .iter()
        .find(|h| h["request_key"] == receipt["request_key"])
    {
        if old != &receipt {
            return Err(LegacyError::conflict(
                "This handoff key already records another confirmed message.",
            ));
        }
        return Ok(row);
    }
    if history.len() >= 500 {
        return Err(LegacyError::invalid(
            "Export this itch before adding more handoff history.",
        ));
    }
    history.push(receipt.clone());
    bump(&mut row);
    put(conn, &row, "handoff", &receipt)?;
    Ok(row)
}

pub(super) fn merge(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let id = args.required_text("id", 100)?;
    let into = args.required_text("into", 100)?;
    if id == into {
        return Err(LegacyError::invalid("Pick two different itches to merge."));
    }
    let mut drop = get(conn, id)?;
    let mut keep = get(conn, into)?;
    if drop["project"] != keep["project"] {
        return Err(LegacyError::invalid(
            "Only merge itches from the same project.",
        ));
    }
    if matches!(
        drop["state"].as_str(),
        Some("resolved" | "archived" | "suggested" | "declined")
    ) || matches!(
        keep["state"].as_str(),
        Some("archived" | "suggested" | "declined")
    ) {
        return Err(LegacyError::invalid("Only merge open itches."));
    }
    let mut occurrences = keep["occurrences"].as_array().cloned().unwrap_or_default();
    let added = drop["occurrences"].as_array().cloned().unwrap_or_default();
    if occurrences.len() + added.len() > 500 {
        return Err(LegacyError::invalid(
            "Too many occurrences to merge; archive/export first.",
        ));
    }
    for mut item in added {
        item["merged_from"] = json!(id);
        occurrences.push(item);
    }
    occurrences.sort_by(|a, b| {
        a["at"]
            .as_str()
            .unwrap_or("")
            .cmp(b["at"].as_str().unwrap_or(""))
    });
    keep["occurrences"] = json!(occurrences);
    let added = drop["attachments"].as_array().cloned().unwrap_or_default();
    let attachments = array_mut(&mut keep, "attachments")?;
    for item in added {
        if !attachments.iter().any(|a| a["id"] == item["id"]) {
            attachments.push(item);
        }
    }
    let added = drop["feedback"].as_array().cloned().unwrap_or_default();
    let feedback = array_mut(&mut keep, "feedback")?;
    feedback.extend(added);
    if feedback.len() > 80 {
        feedback.drain(..feedback.len() - 80);
    }
    if keep.get("merged").is_none() {
        keep["merged"] = json!([]);
    }
    let merged = array_mut(&mut keep, "merged")?;
    merged.push(json!({"id":id,"title":drop["title"],"at":time::now_iso()}));
    if merged.len() > 80 {
        merged.drain(..merged.len() - 80);
    }
    if keep["state"] == "resolved" {
        keep["state"] = json!("open");
    }
    drop["state"] = json!("archived");
    drop["merged_into"] = json!(into);
    bump(&mut keep);
    bump(&mut drop);
    // Both puts and the caller's retry receipt share the same SQLite transaction.
    put(conn, &drop, "merge_into", &json!({"id":into}))?;
    put(conn, &keep, "merge_from", &json!({"id":id}))?;
    Ok(json!({"kept":keep,"archived":drop}))
}

pub(super) fn update(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let mut row = get(conn, args.required_text("id", 100)?)?;
    let expected = args.integer("expected_version", 0, 0, 1_000_000)?;
    if expected != 0 && row["version"] != expected {
        return Err(LegacyError::conflict(
            "This itch changed. Reload it before saving.",
        ));
    }
    let changes = args.json("changes", json!({}), 24_000)?;
    let fields = [
        "title",
        "desired_outcome",
        "next_step",
        "test_url",
        "state",
        "plan",
        "feedback",
        "recheck",
        "run_state",
        "evidence",
    ];
    let changes_args = Args::new(&changes, &fields)?;
    for (key, value) in changes
        .as_object()
        .ok_or_else(|| LegacyError::invalid("Expected an object of itch changes."))?
    {
        let clean = match key.as_str() {
            "title" => json!(changes_args.required_text(key, 2000)?),
            "desired_outcome" | "next_step" => json!(changes_args.text(key, 2000)?),
            "test_url" => {
                let url = changes_args.text(key, 2000)?;
                if !url.is_empty()
                    && !["http://", "https://", "t3-thread://v1/"]
                        .iter()
                        .any(|scheme| url.starts_with(scheme))
                {
                    return Err(LegacyError::invalid(
                        "Use an HTTP(S) test URL or exact T3 thread link.",
                    ));
                }
                json!(url)
            }
            "state" => {
                let state = changes_args.text(key, 40)?;
                if !["open", "needs_description", "working", "ready", "archived"].contains(&state) {
                    return Err(LegacyError::invalid(
                        "Use manager.complete with explicit acceptance for resolved outcomes.",
                    ));
                }
                json!(state)
            }
            "run_state" => {
                let state = changes_args.text(key, 40)?;
                if ![
                    "not_sent",
                    "brief_ready",
                    "agent_reported_running",
                    "agent_reported_done",
                    "failed",
                ]
                .contains(&state)
                {
                    return Err(LegacyError::invalid("Invalid run state."));
                }
                json!(state)
            }
            "plan" => {
                let steps = value
                    .as_array()
                    .ok_or_else(|| LegacyError::invalid("Expected an array of plan steps."))?;
                if steps.len() > 30 {
                    return Err(LegacyError::invalid("Use at most 30 plan steps."));
                }
                let mut ids = std::collections::HashSet::new();
                let mut plan = Vec::new();
                for task in steps {
                    let task_args = Args::new(task, &["id", "text", "done"])?;
                    let id = task_args.text("id", 100)?;
                    let id = if id.is_empty() {
                        uuid::Uuid::new_v4().simple().to_string()
                    } else {
                        id.to_owned()
                    };
                    if !ids.insert(id.clone()) {
                        return Err(LegacyError::invalid("Plan step IDs must be unique."));
                    }
                    plan.push(json!({"id":id,"text":task_args.required_text("text",1000)?,"done":task_args.boolean("done",false)?}));
                }
                json!(plan)
            }
            "feedback" | "evidence" => {
                let values = value.as_array().ok_or_else(|| {
                    LegacyError::invalid("Evidence and feedback must be bounded lists.")
                })?;
                if values.len() > 80 || value.to_string().len() > 15_000 {
                    return Err(LegacyError::invalid(
                        "Evidence and feedback must be bounded lists.",
                    ));
                }
                value.clone()
            }
            "recheck" => {
                if !value.is_null()
                    && (!value.is_object()
                        || !matches!(
                            value["status"].as_str(),
                            Some(
                                "still_relevant"
                                    | "already_addressed"
                                    | "conditions_changed"
                                    | "related"
                                    | "cannot_verify"
                            )
                        )
                        || value.to_string().len() > 12_000)
                {
                    return Err(LegacyError::invalid(
                        "A recheck needs an explicit supported status and bounded evidence.",
                    ));
                }
                value.clone()
            }
            _ => return Err(LegacyError::invalid("Unsupported itch change.")),
        };
        row[key] = clean;
    }
    if changes.get("title").is_some() && row["state"] == "needs_description" {
        row["state"] = json!("open");
    }
    bump(&mut row);
    put(conn, &row, "update", &changes)?;
    Ok(row)
}

pub(super) fn complete(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    if !args.boolean("accepted", false)? {
        return Err(LegacyError::invalid(
            "User acceptance is required. Report ready for review with manager.update first.",
        ));
    }
    let summary = args.required_text("summary", 2000)?;
    let evidence = args.text("evidence", 2000)?;
    let elsewhere = args.boolean("resolved_elsewhere", false)?;
    let mut row = get(conn, args.required_text("id", 100)?)?;
    if row["state"] == "needs_description" {
        return Err(LegacyError::invalid(
            "Describe this moment before accepting an outcome.",
        ));
    }
    if row["state"] == "resolved" {
        return Ok(row);
    }
    let outcomes = array_mut(&mut row, "outcomes")?;
    if outcomes.len() >= 500 {
        return Err(LegacyError::invalid(
            "This itch reached its outcome limit; export it before adding another.",
        ));
    }
    let outcome = json!({"at":time::now_iso(),"summary":summary,"evidence":evidence,"acceptance":"user_confirmed","resolved_elsewhere":elsewhere});
    outcomes.push(outcome.clone());
    row["state"] = json!("resolved");
    bump(&mut row);
    put(conn, &row, "complete", &outcome)?;
    Ok(row)
}

pub(super) fn repeat(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let note = args.text("note", 2000)?;
    let mut row = get(conn, args.required_text("id", 100)?)?;
    let cid = setting(conn, "legacy_active_context")?;
    let context = match cid.as_str() {
        Some(id) if !id.is_empty() => get_context(conn, id)?,
        _ => Value::Null,
    };
    let context = if context["project"] == row["project"] {
        context
    } else {
        Value::Null
    };
    let occurrences = array_mut(&mut row, "occurrences")?;
    if occurrences.len() >= 500 {
        return Err(LegacyError::invalid(
            "This itch reached its occurrence limit; export it before adding another.",
        ));
    }
    let occurrence = json!({"at":time::now_iso(),"note":note,"context":context});
    occurrences.push(occurrence.clone());
    row["state"] = json!("open");
    row["recheck"] = Value::Null;
    row["run_state"] = json!("not_sent");
    for task in array_mut(&mut row, "plan")? {
        task["done"] = json!(false);
    }
    bump(&mut row);
    put(conn, &row, "repeat", &occurrence)?;
    Ok(row)
}

fn bump(row: &mut Value) {
    row["version"] = json!(row["version"].as_u64().unwrap_or(0) + 1);
    row["updated_at"] = json!(time::now_iso());
}

/// The frontend sends Date.toISOString(); also accept a real numeric offset.
/// Validation does not replace the user's observation time with save time.
fn valid_timestamp(raw: &str) -> bool {
    if !raw.is_ascii() || !(20..=40).contains(&raw.len()) {
        return false;
    }
    if raw.as_bytes()[4] != b'-'
        || raw.as_bytes()[7] != b'-'
        || raw.as_bytes()[10] != b'T'
        || raw.as_bytes()[13] != b':'
        || raw.as_bytes()[16] != b':'
    {
        return false;
    }
    let number = |start, end| raw[start..end].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0, 4),
        number(5, 7),
        number(8, 10),
        number(11, 13),
        number(14, 16),
        number(17, 19),
    ) else {
        return false;
    };
    if year == 0 || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    if day == 0 || day > max_day {
        return false;
    }
    let mut tail = &raw[19..];
    if let Some(fraction) = tail.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 || count > 9 {
            return false;
        }
        tail = &fraction[count..];
    }
    tail == "Z"
        || (tail.len() == 6
            && matches!(tail.as_bytes()[0], b'+' | b'-')
            && tail.as_bytes()[3] == b':'
            && tail[1..3].parse::<u32>().is_ok_and(|n| n <= 23)
            && tail[4..6].parse::<u32>().is_ok_and(|n| n <= 59))
}
