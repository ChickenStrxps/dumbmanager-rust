//! Local implementation of the original DumbManager JSON contract.
//!
//! These records live in the Rust application's stamped database. This module
//! has no Python, process, filesystem, credential, or network adapter. The
//! original state is kept verbatim alongside the coarse first-milestone core
//! status; e.g. original `open` means stashed, not an agent running.

mod input;
mod itches;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use crate::store::{load_settings, Store};
use crate::{time, CoreError, CoreResult, Itch, ItchStatus, Motion};
use input::Args;

pub type LegacyResult<T> = Result<T, LegacyError>;

/// Matches the original HTTP error body; the transport uses `status` for its
/// synthetic Response. Do not turn this into a network failure: failed writes
/// must remain visible, rather than entering the frontend's offline outbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyError {
    pub error: String,
    pub code: String,
    pub status: u16,
}

impl LegacyError {
    pub fn invalid(message: &str) -> Self {
        Self::new(message, "invalid", 400)
    }

    pub fn unavailable(message: &str) -> Self {
        Self::new(message, "unavailable", 501)
    }

    pub fn conflict(message: &str) -> Self {
        Self::new(message, "conflict", 409)
    }

    pub fn not_found(message: &str) -> Self {
        Self::new(message, "not_found", 404)
    }

    fn new(message: &str, code: &str, status: u16) -> Self {
        Self {
            error: message.to_owned(),
            code: code.to_owned(),
            status,
        }
    }
}

impl std::fmt::Display for LegacyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}

impl std::error::Error for LegacyError {}

impl From<CoreError> for LegacyError {
    fn from(error: CoreError) -> Self {
        let status = match &error {
            CoreError::Invalid(_) => 400,
            CoreError::NotFound(_) => 404,
            CoreError::Unavailable(_) => 501,
            CoreError::Busy(_) => 503,
            _ => 500,
        };
        Self::new(&error.to_string(), error.code(), status)
    }
}

impl From<rusqlite::Error> for LegacyError {
    fn from(error: rusqlite::Error) -> Self {
        CoreError::from(error).into()
    }
}

const SCHEMA_V2: &str = "
CREATE TABLE legacy_itches (
    id TEXT PRIMARY KEY NOT NULL REFERENCES itches(id) ON DELETE CASCADE,
    data TEXT NOT NULL
) STRICT;
CREATE TABLE legacy_contexts (
    id TEXT PRIMARY KEY NOT NULL,
    project TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    revision INTEGER NOT NULL UNIQUE,
    data TEXT NOT NULL,
    UNIQUE(project, thread_id)
) STRICT;
CREATE TABLE legacy_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    itch_id TEXT NOT NULL REFERENCES itches(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    at TEXT NOT NULL,
    action TEXT NOT NULL,
    changes TEXT NOT NULL,
    UNIQUE(itch_id, version)
) STRICT;
CREATE TABLE legacy_requests (
    key TEXT PRIMARY KEY NOT NULL,
    fingerprint TEXT NOT NULL,
    result TEXT NOT NULL
) STRICT;
";

pub(crate) fn migrate(tx: &Transaction<'_>) -> CoreResult<()> {
    tx.execute_batch(SCHEMA_V2)?;
    let ids = tx
        .prepare("SELECT id FROM itches ORDER BY revision")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        let itch = crate::store::load_itch(tx, &id)?.ok_or_else(|| {
            CoreError::Storage("A native record disappeared during migration.".into())
        })?;
        sync_native(tx, &itch)?;
    }
    Ok(())
}

fn decode(raw: &str) -> CoreResult<Value> {
    serde_json::from_str(raw)
        .map_err(|_| CoreError::Storage("A local compatibility record is unreadable.".into()))
}

/// Preserve first-milestone native records and keep the two command surfaces
/// coherent. No outcomes, occurrences, or agent reports are reconstructed.
pub(crate) fn sync_native(tx: &Transaction<'_>, itch: &Itch) -> CoreResult<()> {
    let previous: Option<String> = tx
        .query_row(
            "SELECT data FROM legacy_itches WHERE id = ?1",
            [&itch.id],
            |r| r.get(0),
        )
        .optional()?;
    let mut row = previous
        .as_deref()
        .map(decode)
        .transpose()?
        .unwrap_or_else(|| {
            json!({
                "id":itch.id, "project":itch.project, "title":itch.title,
                "key":itches::title_key(&itch.title), "state":"open", "version":0,
                "original":if itch.description.is_empty(){&itch.title}else{&itch.description},
                "context_id":"", "context":null, "created_at":itch.created_at,
                "updated_at":itch.updated_at, "event_at":itch.created_at,
                "attachments":[], "occurrences":[], "plan":[], "outcomes":[],
                "feedback":[], "evidence":[], "run_state":"not_sent", "next_step":"",
                "desired_outcome":"", "test_url":"", "recheck":null,
                "provenance":"native_core"
            })
        });
    let old_state = row["state"].as_str().unwrap_or("open").to_owned();
    let projected = itches::core_status(&old_state);
    // A title-only native edit must not turn an archived itch into an open one.
    if previous.is_none() || itch.status != projected {
        row["state"] = json!(match itch.status {
            ItchStatus::Open | ItchStatus::Stashed => "open",
            ItchStatus::Ready => "ready",
            ItchStatus::Resolved => "resolved",
        });
    }
    row["title"] = json!(itch.title);
    row["project"] = json!(itch.project);
    row["description"] = json!(itch.description);
    row["native_kind"] = json!(itch.kind);
    row["updated_at"] = json!(itch.updated_at);
    row["version"] = json!(row["version"].as_u64().unwrap_or(0) + 1);
    tx.execute(
        "INSERT INTO legacy_itches(id,data) VALUES(?1,?2)
         ON CONFLICT(id) DO UPDATE SET data=excluded.data",
        params![itch.id, row.to_string()],
    )?;
    tx.execute(
        "INSERT INTO legacy_events(itch_id,version,at,action,changes) VALUES(?1,?2,?3,?4,?5)",
        params![
            itch.id,
            row["version"].as_i64().unwrap_or(1),
            time::now_iso(),
            if previous.is_none() {
                "native_snapshot"
            } else {
                "native_edit"
            },
            serde_json::to_string(itch)
                .map_err(|_| CoreError::Storage("Could not preserve a native record.".into()))?
        ],
    )?;
    Ok(())
}

pub const EXTERNAL_UNAVAILABLE: &str = "This capability is unavailable in DumbManager Rust. It has no connection to the original DumbManager, DumbAPI, T3, or external services. Nothing was sent or changed.";

pub fn typing_status() -> Value {
    json!({"provider":"off", "model":"", "key_saved":false,
        "configured":false,"available":false,"revision":"",
        "notice":"Fresh AI suggestions are unavailable in this native build. Saved local wording is still available."})
}

/// Validate the transport boundary before routing it. Paths are exact; neither
/// URLs, traversal, arbitrary capabilities, nor arbitrary filesystem access
/// are accepted. This is also called by the desktop's card-export adapter.
pub fn validate_request(path: &str, payload: &Value) -> LegacyResult<()> {
    if path.len() > 80 || !path.starts_with("/api/") || path.contains(['?', '#', '\\']) {
        return Err(LegacyError::invalid("Expected an exact local /api/ path."));
    }
    if !payload.is_object() || payload.to_string().len() > 65_536 {
        return Err(LegacyError::invalid(
            "Expected a JSON object of at most 64 KiB.",
        ));
    }
    Ok(())
}

/// Allowlisted envelope, shared with native-only exports. `t3_target` is a
/// compatibility field only, and never becomes a connection or destination.
pub fn call_envelope(payload: &Value) -> LegacyResult<(&str, Value)> {
    let envelope = Args::new(payload, &["name", "args", "idempotency_key", "t3_target"])?;
    let name = envelope.required_text("name", 100)?;
    envelope.text("t3_target", 200)?;
    let mut args = payload.get("args").cloned().unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return Err(LegacyError::invalid(
            "Expected object capability arguments.",
        ));
    }
    let key = envelope.text("idempotency_key", 128)?;
    if !key.is_empty() {
        if let Some(existing) = args.get("request_key") {
            if existing != key {
                return Err(LegacyError::invalid("Retry keys disagree."));
            }
        }
        args["request_key"] = json!(key);
    }
    Ok((name, args))
}

impl Store {
    /// Only the native T3 adapter calls this after a confirmed delivery. It is
    /// deliberately absent from the page's capability allowlist.
    pub fn legacy_record_handoff(&mut self, payload: &Value) -> LegacyResult<Value> {
        let args = Args::new(
            payload,
            &["id", "request_key", "thread_id", "message_id", "at"],
        )?;
        let tx = self.write_tx()?;
        let row = itches::record_handoff(&tx, &args)?;
        tx.commit()?;
        Ok(row)
    }

    /// Raw result / structured error, exactly as the old `/api/*` frontend
    /// expects. The Tauri command invokes this on its blocking pool.
    pub fn legacy_api(&mut self, path: &str, payload: &Value) -> LegacyResult<Value> {
        validate_request(path, payload)?;
        match path {
            "/api/call" => {
                let (name, args) = call_envelope(payload)?;
                self.legacy_call(name, &args)
            }
            "/api/native" => {
                let args = Args::new(payload, &["action", "itch_id"])?;
                if args.text("action", 40)?.is_empty() || args.text("action", 40)? == "status" {
                    Ok(
                        json!({"running":false,"available":false,"bindings":{},"test_remaining":0,
                        "test_hits":[],"shortcut_conflicts":[],
                        "note":"Global shortcut helper and snipping are unavailable in this native build."}),
                    )
                } else {
                    Err(LegacyError::unavailable(
                        "Global shortcut helper and snipping are unavailable in this native build.",
                    ))
                }
            }
            "/api/replay" => {
                let args = Args::new(payload, &["action", "hwnd", "id", "keep", "index"])?;
                match args.text("action", 40)? {
                    "" | "status" => Ok(json!({"armed":false,"available":false,"buffer_seconds":0,
                        "error":"Replay capture is unavailable in this native build."})),
                    _ => Err(LegacyError::unavailable(
                        "Replay capture is unavailable in this native build.",
                    )),
                }
            }
            "/api/jobs" => {
                let args = Args::new(payload, &["id"])?;
                if args.text("id", 100)?.is_empty() {
                    Ok(
                        json!({"items":[],"available":false,"note":"External jobs are unavailable in this native build."}),
                    )
                } else {
                    Err(LegacyError::unavailable(EXTERNAL_UNAVAILABLE))
                }
            }
            "/api/cook" | "/api/preview" | "/api/upload" | "/api/itch-upload"
            | "/api/itch-media" | "/api/shutdown" => {
                Err(LegacyError::unavailable(EXTERNAL_UNAVAILABLE))
            }
            _ => Err(LegacyError::not_found("Unknown native API path.")),
        }
    }

    pub fn legacy_call(&mut self, name: &str, payload: &Value) -> LegacyResult<Value> {
        self.legacy_call_with_media(name, payload, None)
    }

    /// Desktop-owned references are resolved against this installation's media
    /// catalog before entering the domain transaction. IPC cannot supply these
    /// objects directly; headless callers without that catalog retain refusal.
    pub fn legacy_call_with_media(
        &mut self,
        name: &str,
        payload: &Value,
        media: Option<&[Value]>,
    ) -> LegacyResult<Value> {
        if name.len() > 100 || !payload.is_object() || payload.to_string().len() > 65_536 {
            return Err(LegacyError::invalid(
                "Invalid or oversized capability request.",
            ));
        }
        // No dynamic function lookup, executor, subprocess, or original API.
        let allowed: &[&str] = match name {
            "manager.state" | "manager.status" | "manager.export" | "manager.typing_status" => &[],
            "manager.capture" => &[
                "title",
                "project",
                "context_id",
                "original",
                "moment",
                "event_at",
                "use_active",
                "context_snapshot",
                "attachments",
            ],
            "manager.get" | "manager.history" => &["id"],
            "manager.attach" => &["id", "attachments"],
            "manager.merge" => &["id", "into"],
            "manager.list" => &["project", "state", "query", "limit", "offset"],
            "manager.update" => &["id", "changes", "expected_version"],
            "manager.repeat" => &["id", "note"],
            "manager.complete" => &[
                "id",
                "summary",
                "evidence",
                "accepted",
                "resolved_elsewhere",
            ],
            "manager.settings" => &["motion"],
            "manager.context" | "manager.context.select" => &["id"],
            "manager.context.publish" => &[
                "project",
                "thread_id",
                "task",
                "testing",
                "entities",
                "candidates",
                "workspace",
                "select",
                "source",
            ],
            "manager.brief" => &["id", "direction"],
            "manager.suggest" => &[
                "text",
                "project",
                "context_id",
                "jev",
                "mode",
                "thread_id",
                "candidates",
                "surface",
                "fresh",
            ],
            "projects.list" => &["limit", "offset", "query"],
            "t3.distributions" => &[],
            "t3.threads" => &["query", "limit", "offset"],
            _ => return Err(LegacyError::unavailable(EXTERNAL_UNAVAILABLE)),
        };
        let mut fields = allowed.to_vec();
        fields.push("request_key");
        let args = Args::new(payload, &fields)?;
        let key = args.text("request_key", 128)?;
        let fingerprint = json!({"name":name,"args":payload}).to_string();
        let data_dir = self.path().parent().map(|p| p.display().to_string());
        let tx = self.write_tx()?;
        if !key.is_empty() {
            let cached: Option<(String, String)> = tx
                .query_row(
                    "SELECT fingerprint,result FROM legacy_requests WHERE key=?1",
                    [key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((old, result)) = cached {
                if old != fingerprint {
                    return Err(LegacyError::conflict(
                        "Request key already used for different input.",
                    ));
                }
                return Ok(decode(&result)?);
            }
        }
        let result = match name {
            "manager.state" => state(&tx)?,
            "manager.status" => {
                let itches: i64 = tx.query_row("SELECT count(*) FROM itches", [], |r| r.get(0))?;
                let contexts: i64 =
                    tx.query_row("SELECT count(*) FROM legacy_contexts", [], |r| r.get(0))?;
                json!({"name":"DumbManager Rust","version":env!("CARGO_PKG_VERSION"),"backend":"Rust",
                    "data_dir":data_dir,"counts":{"itch":itches,"context":contexts},"time":time::now_iso()})
            }
            "manager.capture" => itches::capture(&tx, &args, media)?,
            "manager.attach" => itches::attach(&tx, &args, media)?,
            "manager.merge" => itches::merge(&tx, &args)?,
            "manager.get" => itches::get(&tx, args.required_text("id", 100)?)?,
            "manager.history" => itches::history(&tx, args.required_text("id", 100)?)?,
            "manager.list" => list(&tx, &args)?,
            "manager.update" => itches::update(&tx, &args)?,
            "manager.repeat" => itches::repeat(&tx, &args)?,
            "manager.complete" => itches::complete(&tx, &args)?,
            "manager.settings" => {
                let motion = args.boolean("motion", true)?;
                tx.execute(
                    "INSERT INTO settings(key,value) VALUES('motion',?1)
                    ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [if motion { "full" } else { "off" }],
                )?;
                tx.execute(
                    "INSERT INTO settings(key,value) VALUES('updated_at',?1)
                    ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [time::now_iso()],
                )?;
                json!({"motion":motion})
            }
            "manager.context.publish" => publish_context(&tx, &args)?,
            "manager.context.select" => {
                let id = args.text("id", 100)?;
                if !id.is_empty() {
                    get_context(&tx, id)?;
                }
                set_setting(&tx, "legacy_active_context", &json!(id))?;
                json!({"active_context":id})
            }
            "manager.context" => {
                let id = args.text("id", 100)?;
                let active = setting(&tx, "legacy_active_context")?;
                let id = if id.is_empty() {
                    active.as_str().unwrap_or("")
                } else {
                    id
                };
                if id.is_empty() {
                    json!({"state":"unlinked","contexts":contexts(&tx,80)?})
                } else {
                    get_context(&tx, id)?
                }
            }
            "manager.brief" => brief(&tx, &args)?,
            "manager.suggest" => suggest(&tx, &args)?,
            "manager.typing_status" => typing_status(),
            "manager.export" => {
                let records = itches::records(&tx, None)?
                    .into_iter()
                    .map(|record| json!({"kind":"itch","record":record}))
                    .chain(
                        contexts(&tx, i64::MAX)?
                            .into_iter()
                            .map(|record| json!({"kind":"context","record":record})),
                    )
                    .collect::<Vec<_>>();
                json!({"schema":1,"exported_at":time::now_iso(),"backend":"Rust","records":records,
                    "note":"Rust app records only. No credentials, other installations, or external chat history."})
            }
            "projects.list" => {
                json!({"items":[],"total":0,"next_offset":null,"available":false,"connected":false,
                "note":"No external project roots are configured in DumbManager Rust. Local itch project labels are preserved."})
            }
            "t3.distributions" => {
                json!({"items":[],"selected":"","target":"","connected":false,"available":false,"error":EXTERNAL_UNAVAILABLE})
            }
            "t3.threads" => {
                json!({"items":[],"partial":false,"connected":false,"available":false,"error":EXTERNAL_UNAVAILABLE})
            }
            _ => unreachable!("the capability was allowlisted above"),
        };
        if !key.is_empty() {
            tx.execute(
                "INSERT INTO legacy_requests(key,fingerprint,result) VALUES(?1,?2,?3)",
                params![key, fingerprint, result.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(result)
    }
}

fn state(conn: &Connection) -> LegacyResult<Value> {
    let active = setting(conn, "legacy_active_context")?;
    Ok(
        json!({"itches":itches::records(conn,Some(500))?,"contexts":contexts(conn,80)?,
        "active_context":active,"preferences":{"motion":load_settings(conn)?.motion==Motion::Full},
        "backend":"Rust","typing_words":[],"typing_generator":typing_status(),
        "coverage":"Most recent 500 local itches and 80 saved contexts; use manager.list/export for older records. External services are unavailable."}),
    )
}

fn list(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let project = args.text("project", 100)?;
    let status = args.text("state", 40)?;
    let query = args.text("query", 500)?.to_lowercase();
    let limit = args.integer("limit", 50, 1, 100)? as usize;
    let offset = args.integer("offset", 0, 0, 1_000_000)? as usize;
    let mut statement = conn.prepare("SELECT legacy_itches.data FROM legacy_itches JOIN itches USING(id) ORDER BY itches.revision DESC")?;
    let mut cursor = statement.query([])?;
    let mut items = Vec::new();
    let mut total = 0usize;
    // Stream every row to preserve Unicode/literal search and an honest total,
    // while retaining only one requested page in memory.
    while let Some(raw) = cursor.next()? {
        let row = decode(&raw.get::<_, String>(0)?)?;
        if (project.is_empty() || row["project"] == project)
            && (status.is_empty() || row["state"] == status)
            && query
                .split_whitespace()
                .all(|word| row.to_string().to_lowercase().contains(word))
        {
            if total >= offset && items.len() < limit {
                items.push(row);
            }
            total += 1;
        }
    }
    Ok(json!({"items":items,"total":total,
        "limit":limit,"next_offset":if offset+limit<total{Some(offset+limit)}else{None}}))
}

fn setting(conn: &Connection, key: &str) -> CoreResult<Value> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    raw.as_deref()
        .map(decode)
        .transpose()
        .map(|value| value.unwrap_or(Value::Null))
}

fn set_setting(conn: &Connection, key: &str, value: &Value) -> CoreResult<()> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value.to_string()])?;
    Ok(())
}

fn contexts(conn: &Connection, limit: i64) -> CoreResult<Vec<Value>> {
    let raw = conn
        .prepare("SELECT data FROM legacy_contexts ORDER BY revision DESC LIMIT ?1")?
        .query_map([limit], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    raw.iter().map(|r| decode(r)).collect()
}

fn get_context(conn: &Connection, id: &str) -> LegacyResult<Value> {
    let raw: Option<String> = conn
        .query_row("SELECT data FROM legacy_contexts WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(decode(&raw.ok_or_else(|| {
        LegacyError::not_found("Saved context not found.")
    })?)?)
}

fn publish_context(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let project = args.required_text("project", 100)?;
    let thread = args
        .required_text("thread_id", 145)?
        .strip_prefix("t3-thread://v1/")
        .unwrap_or(args.required_text("thread_id", 145)?);
    if thread.is_empty() || thread.chars().count() > 120 {
        return Err(LegacyError::invalid(
            "Use an exact thread ID of at most 120 characters.",
        ));
    }
    let task = args.required_text("task", 2000)?;
    let testing = args.text("testing", 2000)?;
    let entities = args.text("entities", 2000)?;
    let workspace = args.text("workspace", 2000)?;
    let source = match args.text("source", 40)? {
        "" => "agent_published",
        value => value,
    };
    if !["agent_published", "user_entered"].contains(&source) {
        return Err(LegacyError::invalid("Invalid context source."));
    }
    let candidates = args.json("candidates", json!([]), 10_000)?;
    let candidates = input::text_array(&candidates, 32, 240)?;
    let previous: Option<String> = conn
        .query_row(
            "SELECT data FROM legacy_contexts WHERE project=?1 AND thread_id=?2",
            params![project, thread],
            |r| r.get(0),
        )
        .optional()?;
    let previous = previous.as_deref().map(decode).transpose()?;
    let id = previous
        .as_ref()
        .and_then(|r| r["id"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let version = previous
        .as_ref()
        .and_then(|r| r["version"].as_u64())
        .unwrap_or(0)
        + 1;
    let revision: i64 = conn.query_row(
        "SELECT COALESCE(MAX(revision),0)+1 FROM legacy_contexts",
        [],
        |r| r.get(0),
    )?;
    let row = json!({"id":id,"project":project,"thread_id":thread,"task":task,"testing":testing,
        "entities":entities,"workspace":workspace,"candidates":candidates,"version":version,
        "updated_at":time::now_iso(),"source":source});
    conn.execute(
        "INSERT INTO legacy_contexts(id,project,thread_id,revision,data) VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,data=excluded.data",
        params![id, project, thread, revision, row.to_string()],
    )?;
    if args.boolean("select", false)? {
        set_setting(conn, "legacy_active_context", &json!(id))?;
    }
    Ok(row)
}

fn brief(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let row = itches::get(conn, args.required_text("id", 100)?)?;
    let direction = args.text("direction", 2000)?;
    let ctx = &row["context"];
    let field = |value: &Value| value.as_str().unwrap_or("").to_owned();
    let mut brief=format!("DumbManager Rust itch {}\n\nProject: {}\n\nObservation: {}\n\nDesired outcome: {}\n\nTask when observed: {}\n\nTesting: {}\n\nWorkspace note: {}",
        field(&row["id"]),field(&row["project"]),field(&row["title"]),field(&row["desired_outcome"]),field(&ctx["task"]),field(&ctx["testing"]),field(&ctx["workspace"]));
    if !direction.is_empty() {
        brief.push_str(&format!("\n\nRequested feel adjustment: {direction}"));
    }
    if row["feedback"].as_array().is_some_and(|v| !v.is_empty()) {
        brief.push_str(&format!("\n\nSaved feedback: {}", row["feedback"]));
    }
    brief.push_str("\n\nRecheck current conditions and preserve evidence. Report ready for review; only explicit user acceptance resolves this itch. Send through an explicitly configured native T3 connection or copy this brief manually. Do not use the original DumbAPI to update this isolated record.");
    Ok(
        json!({"id":row["id"],"brief":brief,"thread_url":Value::Null,"handoff":"copy_brief",
        "note":"Prepared locally. No thread was opened, verified, or sent a message."}),
    )
}

fn suggest(conn: &Connection, args: &Args<'_>) -> LegacyResult<Value> {
    let typed = args.required_text("text", 500)?;
    let project = args.text("project", 100)?;
    let mode = match args.text("mode", 20)? {
        "" => "itch",
        v => v,
    };
    if !["itch", "chat"].contains(&mode) {
        return Err(LegacyError::invalid(
            "Suggestion mode must be itch or chat.",
        ));
    }
    args.boolean("jev", true)?;
    args.boolean("fresh", false)?;
    args.text("surface", 1800)?;
    let thread = args.text("thread_id", 120)?;
    let supplied = args.json("candidates", json!([]), 10_000)?;
    let supplied = input::text_array(&supplied, 32, 240)?;
    let active = setting(conn, "legacy_active_context")?;
    let requested = args.text("context_id", 100)?;
    let requested = if requested.is_empty() && project.is_empty() && mode != "chat" {
        active.as_str().unwrap_or("")
    } else {
        requested
    };
    let context = if mode == "chat" {
        contexts(conn, 80)?
            .into_iter()
            .find(|r| !thread.is_empty() && r["thread_id"] == thread)
    } else if requested.is_empty() {
        None
    } else {
        Some(get_context(conn, requested)?)
    };
    let context = context.filter(|r| project.is_empty() || r["project"] == project);
    let scope = if project.is_empty() {
        context
            .as_ref()
            .and_then(|c| c["project"].as_str())
            .unwrap_or("")
    } else {
        project
    };
    let mut pool = Vec::new();
    if mode == "chat" {
        if !thread.is_empty() {
            pool.extend(supplied);
        }
    } else {
        if let Some(c) = &context {
            pool.extend(
                c["candidates"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned)),
            );
        }
        pool.extend(
            itches::records(conn, Some(300))?
                .iter()
                .filter(|r| {
                    !matches!(
                        r["state"].as_str(),
                        Some("archived" | "declined" | "suggested")
                    ) && (scope.is_empty() || r["project"] == scope)
                })
                .filter_map(|r| r["title"].as_str().map(str::to_owned)),
        );
    }
    let mut seen = std::collections::HashSet::new();
    let prefix = typed.to_lowercase();
    let items = pool
        .into_iter()
        .filter(|s| {
            s.to_lowercase().starts_with(&prefix)
                && s.chars().count() > typed.chars().count()
                && s.chars().count() <= 500
                && seen.insert(s.clone())
        })
        .take(8)
        .map(|value| {
            let suffix = value
                .chars()
                .skip(typed.chars().count())
                .collect::<String>();
            json!({"text":format!("{typed}{suffix}"),"score":1,"source":"local","kind":"phrase"})
        })
        .collect::<Vec<_>>();
    let context_thread = context
        .as_ref()
        .and_then(|r| r["thread_id"].as_str())
        .unwrap_or("");
    Ok(
        json!({"items":items,"source":"local","pick":null,"generation":{"status":"unavailable","notice":"AI generation and Jev ranking are unavailable; these are saved local prefix matches only."},
        "context":{"thread_id":context_thread,"source":"published","at":context.as_ref().map(|r|&r["updated_at"])}}),
    )
}
