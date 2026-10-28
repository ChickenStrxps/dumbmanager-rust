//! Explicitly configured, local T3 orchestration. No original app data is read.
use crate::state::AppState;
use dumbmanager_core::legacy::{self, LegacyError, LegacyResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    net::{SocketAddr, TcpStream},
    path::Path,
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tungstenite::{
    client::IntoClientRequest,
    protocol::{Message, WebSocketConfig},
};
use uuid::Uuid;

const CONFIG: &str = "t3-config";
const RECEIPTS: &str = "t3-receipts";
const MAX_REPLY: u64 = 4 * 1024 * 1024;
const MAX_PREVIEW: u64 = 2 * 1024 * 1024;
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
fn err(code: &str, status: u16, text: &str) -> LegacyError {
    LegacyError {
        error: text.into(),
        code: code.into(),
        status,
    }
}
fn unavailable() -> LegacyError {
    err(
        "t3_unavailable",
        503,
        "Local T3 is unavailable. Your draft is kept.",
    )
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn checked_thread(id: &str) -> LegacyResult<()> {
    let s = id.strip_prefix("mcp:").unwrap_or(id);
    let parsed =
        Uuid::parse_str(s).map_err(|_| LegacyError::invalid("Choose an exact T3 thread."))?;
    if parsed.hyphenated().to_string() != s.to_ascii_lowercase() {
        return Err(LegacyError::invalid("Choose an exact T3 thread."));
    }
    Ok(())
}
fn id_ok(id: &str, max: usize) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn selected(v: &Value) -> &str {
    v.get("selected").and_then(Value::as_str).unwrap_or("")
}
fn port(v: &Value) -> LegacyResult<u16> {
    let n = v.as_u64().unwrap_or(0);
    if (1024..=65535).contains(&n) {
        Ok(n as u16)
    } else {
        Err(LegacyError::invalid(
            "Choose a local port between 1024 and 65535.",
        ))
    }
}
fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}
fn target(config: &Value) -> String {
    let id = selected(config);
    let p = config
        .pointer(&format!("/items/{id}/port"))
        .and_then(Value::as_u64);
    if let Some(port) = p.filter(|_| !id.is_empty()) {
        let item = &config["items"][id];
        let identity = format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", text(item, "cli"), text(item, "home")).as_bytes())
        );
        format!("{id}@{}#{}", endpoint(port as u16), &identity[..16])
    } else {
        "t3@unconfigured".into()
    }
}
fn target_guard(config: &Value, expected: &str) -> LegacyResult<()> {
    if !expected.is_empty() && expected != target(config) {
        Err(err(
            "t3_target_changed",
            409,
            "The T3 installation changed. Reload before continuing.",
        ))
    } else {
        Ok(())
    }
}
fn valid_distribution(id: &str) -> bool {
    matches!(id, "t3" | "knife" | "custom")
}
fn config(state: &AppState) -> LegacyResult<Value> {
    let v = state.runtime.read_document(CONFIG)?;
    if v.is_null() {
        Ok(json!({"selected":"","items":{}}))
    } else if v.is_object() {
        Ok(v)
    } else {
        Err(unavailable())
    }
}
struct Connection {
    port: u16,
    token: String,
}
fn connection(state: &AppState, cfg: &Value) -> LegacyResult<Connection> {
    let id = selected(cfg);
    if !valid_distribution(id) {
        return Err(err(
            "t3_not_connected",
            503,
            "Configure and connect a local T3 session in settings first.",
        ));
    }
    let p = port(&cfg["items"][id]["port"])?;
    if !cfg["items"][id]["configured"].as_bool().unwrap_or(false) {
        return Err(err(
            "t3_not_connected",
            503,
            "Connect this local T3 session first.",
        ));
    }
    let token = state
        .runtime
        .secret(session_key(
            id,
            cfg["items"][id]["slot"].as_u64().unwrap_or(0).min(1),
        ))?
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_whitespace))
        .ok_or_else(|| {
            err(
                "t3_not_connected",
                503,
                "Connect this local T3 session first.",
            )
        })?;
    Ok(Connection { port: p, token })
}
fn http(conn: &Connection, route: &str, auth: bool, max: u64) -> LegacyResult<(Vec<u8>, String)> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| unavailable())?;
    let mut request = client.get(format!("{}{route}", endpoint(conn.port)));
    if auth {
        request = request
            .bearer_auth(&conn.token)
            .header("x-t3-orchestration-protocol", "2");
    }
    let mut response = request.send().map_err(|_| unavailable())?;
    if !response.status().is_success() {
        return Err(unavailable());
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let mut raw = Vec::new();
    response
        .by_ref()
        .take(max + 1)
        .read_to_end(&mut raw)
        .map_err(|_| unavailable())?;
    if raw.len() as u64 > max {
        return Err(err(
            "preview_too_large",
            413,
            "T3 response exceeds the supported size. Open it in T3.",
        ));
    }
    Ok((raw, mime))
}
fn get_json(conn: &Connection, route: &str) -> LegacyResult<Value> {
    let (raw, _) = http(conn, route, true, MAX_REPLY)?;
    serde_json::from_slice(&raw).map_err(|_| unavailable())
}
fn rpc(conn: &Connection, tag: &str, payload: &Value) -> LegacyResult<Value> {
    let addr = SocketAddr::from(([127, 0, 0, 1], conn.port));
    let stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(4)).map_err(|_| unavailable())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .map_err(|_| unavailable())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(8)))
        .map_err(|_| unavailable())?;
    let mut req = format!("ws://127.0.0.1:{}/ws?orchestrationProtocol=2", conn.port)
        .into_client_request()
        .map_err(|_| unavailable())?;
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", conn.token)
            .parse()
            .map_err(|_| unavailable())?,
    );
    let mut cfg = WebSocketConfig::default();
    cfg.max_message_size = Some(MAX_REPLY as usize);
    cfg.max_frame_size = Some(MAX_REPLY as usize);
    let (mut ws, _) = tungstenite::client::client_with_config(req, stream, Some(cfg))
        .map_err(|_| unavailable())?;
    let req = json!({"_tag":"Request","id":"1","tag":tag,"payload":payload,"headers":[]});
    ws.send(Message::Text(req.to_string().into()))
        .map_err(|_| unavailable())?;
    let deadline = Instant::now() + Duration::from_secs(12);
    for _ in 0..128 {
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        ws.get_mut()
            .set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(8)),
            ))
            .map_err(|_| unavailable())?;
        let msg = ws.read().map_err(|_| unavailable())?;
        if let Message::Text(s) = msg {
            let v: Value = serde_json::from_str(&s).map_err(|_| unavailable())?;
            let rows: Vec<&Value> = if let Some(a) = v.as_array() {
                a.iter().collect()
            } else {
                vec![&v]
            };
            for row in rows {
                if text(row, "_tag") == "Ping" {
                    ws.send(Message::Text(json!({"_tag":"Pong"}).to_string().into()))
                        .map_err(|_| unavailable())?;
                }
                if text(row, "_tag") == "Exit"
                    && row
                        .get("requestId")
                        .map(|v| v == "1" || v == 1)
                        .unwrap_or(false)
                {
                    if text(&row["exit"], "_tag") == "Success" {
                        return Ok(row["exit"]["value"].clone());
                    }
                    return Err(err(
                        "t3_rejected",
                        409,
                        "T3 rejected the message. Open the thread to check its state.",
                    ));
                }
            }
        } else if matches!(msg, Message::Close(_)) {
            return Err(unavailable());
        }
    }
    Err(unavailable())
}
fn shell(conn: &Connection) -> LegacyResult<Value> {
    let v = get_json(conn, "/api/orchestration/shell")?;
    if !v["threads"].is_array() || !v["projects"].is_array() {
        return Err(unavailable());
    }
    Ok(v)
}
fn compact(row: &Value) -> Value {
    json!({"id":row["id"],"title":row.get("title").and_then(Value::as_str).unwrap_or("Untitled"),"project_id":row["projectId"],"status":row.get("activityRunStatus").filter(|v| !v.is_null()).unwrap_or(&row["status"]),"active":!row["activeRunId"].is_null(),"updated_at":row["updatedAt"],"archived":!row["archivedAt"].is_null(),"model":row["modelSelection"]["model"]})
}
fn threads(conn: &Connection, args: &Value) -> LegacyResult<Value> {
    let query = text(args, "query").to_lowercase();
    if query.len() > 200 {
        return Err(LegacyError::invalid("Search is too long."));
    }
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100);
    if !(1..=200).contains(&limit) {
        return Err(LegacyError::invalid("Limit must be between 1 and 200."));
    }
    let data = shell(conn)?;
    let projects: HashMap<&str, &str> = data["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| {
            Some((
                p["id"].as_str()?,
                p["title"]
                    .as_str()
                    .or_else(|| p["name"].as_str())
                    .unwrap_or("Project"),
            ))
        })
        .collect();
    let mut rows: Vec<Value> = data["threads"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| {
            t["deletedAt"].is_null()
                && text(&t["lineage"], "relationshipToParent") != "subagent"
                && t["id"].is_string()
        })
        .filter_map(|t| {
            let mut r = compact(t);
            let project = projects
                .get(text(t, "projectId"))
                .copied()
                .unwrap_or("Project");
            r["project"] = json!(project);
            let search = format!("{} {}", text(&r, "title"), project).to_lowercase();
            if query.split_whitespace().all(|w| search.contains(w)) {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    rows.sort_by(|a, b| text(b, "updated_at").cmp(text(a, "updated_at")));
    let total = rows.len();
    rows.truncate(limit as usize);
    Ok(json!({"items":rows,"total":total,"partial":total>limit as usize,"connected":true}))
}
fn previews(projection: &Value) -> Vec<Value> {
    let items = projection
        .get("visibleTurnItems")
        .or_else(|| projection.get("turnItems"))
        .and_then(Value::as_array);
    let mut map = HashMap::new();
    for entry in items.into_iter().flatten() {
        let item = entry.get("item").unwrap_or(entry);
        let name = text(item, "toolName");
        let prefix = name.strip_suffix("html_render");
        if text(item, "type") != "dynamic_tool"
            || text(item, "status") != "completed"
            || !prefix
                .map(|s| {
                    s.is_empty()
                        || ((s.ends_with('.') || s.ends_with('_')) && id_ok(&s[..s.len() - 1], 256))
                })
                .unwrap_or(false)
            || item["output"]["isError"].as_bool().unwrap_or(false)
        {
            continue;
        }
        let r = &item["output"]["htmlRender"];
        let id = text(r, "attachmentId");
        let h = r["height"].as_f64().unwrap_or(0.);
        if !(id_ok(id, 256) && h > 0. && h < 100000.) {
            continue;
        }
        let title = r["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("Design preview")
            .chars()
            .take(200)
            .collect::<String>();
        map.insert(id.to_owned(),json!({"id":id,"title":title,"height":h.round().clamp(160.,1000.),"at":item.get("completedAt").filter(|v|!v.is_null()).unwrap_or(&item["startedAt"])}));
    }
    let mut rows: Vec<Value> = map.into_values().collect();
    rows.sort_by(|a, b| text(a, "at").cmp(text(b, "at")));
    let n = rows.len().saturating_sub(4);
    rows.drain(..n);
    rows
}
fn projection(conn: &Connection, id: &str) -> LegacyResult<Value> {
    checked_thread(id)?;
    let v = get_json(
        conn,
        &format!("/api/orchestration/threads/{id}/bounded?compactTurnItems=1"),
    )?;
    if !v["projection"].is_object() || !v["projection"]["thread"].is_object() {
        return Err(unavailable());
    }
    Ok(v)
}
fn read(conn: &Connection, id: &str) -> LegacyResult<Value> {
    let data = projection(conn, id)?;
    let p = &data["projection"];
    let mut messages = Vec::new();
    for m in p["messages"].as_array().into_iter().flatten() {
        let role = text(m, "role");
        let full = text(m, "text");
        let attachments=m["attachments"].as_array().map(|a|a.iter().take(8).map(|v|json!({"id":v["id"],"type":v["type"],"name":v["name"],"mimeType":v["mimeType"],"sizeBytes":v["sizeBytes"]})).collect::<Vec<_>>()).unwrap_or_default();
        if !matches!(role, "user" | "assistant") || (full.is_empty() && attachments.is_empty()) {
            continue;
        }
        let body = full.chars().take(6000).collect::<String>();
        messages.push(json!({"id":m["id"],"role":role,"text":body,"truncated":full.chars().count()>6000,"streaming":m["streaming"].as_bool().unwrap_or(false),"at":m["createdAt"],"attachments":attachments}));
    }
    messages.sort_by(|a, b| text(a, "at").cmp(text(b, "at")));
    let more = messages.len() > 8;
    let start = messages.len().saturating_sub(8);
    messages.drain(..start);
    let mut thread = compact(&p["thread"]);
    let runs = p["runs"].as_array();
    let active = runs.into_iter().flatten().rfind(|r| {
        matches!(
            text(r, "status"),
            "running" | "starting" | "preparing" | "waiting"
        )
    });
    thread["active"] = json!(active.is_some());
    if let Some(r) = active {
        thread["status"] = r["status"].clone();
    }
    let queued = runs
        .into_iter()
        .flatten()
        .filter(|r| text(r, "status") == "queued")
        .count();
    Ok(
        json!({"thread":thread,"messages":messages,"previews":previews(p),"queued":queued,"partial":data["hasMoreHistory"].as_bool().unwrap_or(false)||more,"note":"Eight recent messages Ã‚Â· latest four published visuals. Open T3 for tools, approvals and full history."}),
    )
}
fn preview(conn: &Connection, id: &str, attachment: &str) -> LegacyResult<Value> {
    let data = projection(conn, id)?;
    let reference = previews(&data["projection"])
        .into_iter()
        .find(|r| text(r, "id") == attachment)
        .ok_or_else(|| {
            LegacyError::not_found("That visual is no longer in the bounded thread view.")
        })?;
    let asset = rpc(
        conn,
        "assets.createUrl",
        &json!({"resource":{"_tag":"attachment","attachmentId":attachment,"fileName":"concept.html","mimeType":"text/html","disposition":"inline"}}),
    )?;
    let route = text(&asset, "relativeUrl");
    if !route.starts_with("/api/assets/")
        || route.len() > 4110
        || route.contains("..")
        || !route
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/_-.?=&%".contains(&c))
    {
        return Err(unavailable());
    }
    let (raw, mime) = http(conn, route, false, MAX_PREVIEW)?;
    if mime != "text/html" {
        return Err(unavailable());
    }
    let html = String::from_utf8(raw).map_err(|_| unavailable())?;
    let mut result = reference;
    result["html"] = json!(html);
    Ok(result)
}
fn session_key(id: &str, slot: u64) -> &'static str {
    match (id, slot) {
        ("t3", 0) => "t3-regular-a",
        ("t3", _) => "t3-regular-b",
        ("knife", 0) => "t3-knife-a",
        ("knife", _) => "t3-knife-b",
        ("custom", 0) => "t3-custom-a",
        _ => "t3-custom-b",
    }
}
fn list(state: &AppState, cfg: &Value) -> LegacyResult<Value> {
    let mut items = Vec::new();
    for (id, label, default_port) in [
        ("t3", "T3 Code", 3773),
        ("knife", "The Knife (isolated)", 4773),
        ("custom", "Custom local T3", 3773),
    ] {
        let item = &cfg["items"][id];
        let p = item["port"].as_u64().unwrap_or(default_port);
        let configured = item["configured"].as_bool().unwrap_or(false);
        items.push(json!({"id":id,"label":label,"installed":item["cli"].as_str().map(|s|Path::new(s).is_file()).unwrap_or(false),"selected":selected(cfg)==id,"configured":configured,"endpoint":endpoint(p as u16),"cli":item["cli"],"home":item["home"],"port":p,"verified_at":item["verified_at"],"expires_at":item["expires_at"]}));
    }
    let _ = state;
    Ok(
        json!({"items":items,"selected":selected(cfg),"target":target(cfg),"note":"Settings are stored only in DumbManager Rust. Opening settings does not connect or start an app."}),
    )
}
fn configure(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "distribution");
    if !valid_distribution(id) {
        return Err(LegacyError::invalid("Choose a T3 distribution."));
    }
    let cli = text(args, "cli").trim();
    let home = text(args, "home").trim();
    let p = port(&args["port"])?;
    if cli.len() > 4096
        || home.len() > 4096
        || !Path::new(cli).is_absolute()
        || !Path::new(cli).is_file()
        || !Path::new(home).is_absolute()
        || !Path::new(home).is_dir()
        || cli
            .chars()
            .chain(home.chars())
            .any(|c| c.is_control() || "\"&|<>^%!".contains(c))
    {
        return Err(LegacyError::invalid(
            "Choose an existing absolute CLI file and its profile directory.",
        ));
    }
    let mut cfg = config(state)?;
    let previous = cfg["items"][id].clone();
    if !cfg["items"].is_object() {
        cfg["items"] = json!({});
    }
    let unchanged =
        text(&previous, "cli") == cli && text(&previous, "home") == home && previous["port"] == p;
    cfg["items"][id] = if unchanged {
        previous
    } else {
        json!({"cli":cli,"home":home,"port":p,"configured":false})
    };
    if !unchanged && selected(&cfg) == id {
        cfg["selected"] = json!("");
    }
    state.runtime.write_document(CONFIG, &cfg)?;
    list(state, &cfg)
}
fn issue(item: &Value) -> LegacyResult<(String, Value)> {
    let cli = text(item, "cli");
    let home = text(item, "home");
    let p = port(&item["port"])?;
    if !Path::new(cli).is_file() || !Path::new(home).is_dir() {
        return Err(LegacyError::invalid(
            "Configure an existing T3 CLI and profile first.",
        ));
    }
    let mut cmd = Command::new(cli);
    cmd.args([
        "auth",
        "session",
        "issue",
        "--scope",
        "orchestration:read",
        "--scope",
        "orchestration:operate",
        "--ttl",
        "30d",
        "--label",
        "DumbManager local chat",
        "--json",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    for (k, _) in std::env::vars_os() {
        let name = k.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("T3CODE_")
            || name.starts_with("T3_")
            || name.starts_with("KNIFE_")
            || name.starts_with("OTEL_")
            || matches!(
                name.as_str(),
                "CODEX_HOME" | "CLAUDE_CONFIG_DIR" | "NODE_OPTIONS" | "ELECTRON_RUN_AS_NODE"
            )
        {
            cmd.env_remove(k);
        }
    }
    cmd.env("T3CODE_HOME", home)
        .env("T3CODE_CLI_PATH", cli)
        .env("T3CODE_PORT", p.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().map_err(|_| unavailable())?;
    let stdout = child.stdout.take().ok_or_else(unavailable)?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(32001).read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let raw = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| unavailable())?;
    if !status.map(|s| s.success()).unwrap_or(false) || raw.len() > 32000 {
        return Err(err(
            "t3_unavailable",
            503,
            "Pairing failed. Open the selected T3 app and retry; existing connection is kept.",
        ));
    }
    let v: Value = serde_json::from_slice(&raw).map_err(|_| unavailable())?;
    let token = text(&v, "token");
    if token.is_empty()
        || token.len() > 32000
        || token.chars().any(|c| c.is_whitespace() || !c.is_ascii())
    {
        return Err(unavailable());
    }
    Ok((token.into(), v["expiresAt"].clone()))
}
fn select_distribution(state: &AppState, args: &Value, always_issue: bool) -> LegacyResult<Value> {
    let mut cfg = config(state)?;
    target_guard(&cfg, text(args, "expected_target"))?;
    let id = if always_issue && text(args, "distribution").is_empty() {
        if selected(&cfg).is_empty() {
            "t3".to_owned()
        } else {
            selected(&cfg).to_owned()
        }
    } else {
        text(args, "distribution").to_owned()
    };
    if !valid_distribution(&id) {
        return Err(LegacyError::invalid(
            "Configure and choose a T3 distribution first.",
        ));
    }
    let mut item = cfg["items"][&id].clone();
    let p = port(&item["port"])?;
    if let Some(requested_port) = args.get("port") {
        if port(requested_port)? != p {
            return Err(LegacyError::invalid(
                "Save the selected port in connection details before pairing.",
            ));
        }
    }
    if !item.is_object() {
        return Err(LegacyError::invalid("Configure this distribution first."));
    }
    let reconnect = always_issue
        || args["reconnect"].as_bool().unwrap_or(false)
        || !item["configured"].as_bool().unwrap_or(false);
    let slot = item["slot"].as_u64().unwrap_or(0).min(1);
    let next = if reconnect { 1 - slot } else { slot };
    let (token, expires) = if reconnect {
        issue(&item)?
    } else {
        (
            state
                .runtime
                .secret(session_key(&id, slot))?
                .ok_or_else(|| {
                    err(
                        "t3_not_connected",
                        503,
                        "Use Reconnect to pair this distribution.",
                    )
                })?,
            item["expires_at"].clone(),
        )
    };
    let candidate = Connection { port: p, token };
    shell(&candidate).map_err(|_|err("t3_unavailable",503,"Could not verify the selected installation. Open it and retry; the existing connection is kept."))?;
    if reconnect {
        state
            .runtime
            .set_secret(session_key(&id, next), &candidate.token)?;
    }
    let before = target(&cfg);
    item["configured"] = json!(true);
    item["slot"] = json!(next);
    item["expires_at"] = expires;
    item["verified_at"] = json!(dumbmanager_core::time::now_iso());
    cfg["items"][&id] = item;
    cfg["selected"] = json!(id);
    state.runtime.write_document(CONFIG, &cfg)?;
    let mut result = list(state, &cfg)?;
    result["connected"] = json!(true);
    result["changed"] = json!(before != target(&cfg));
    result["note"] = json!(
        "T3 verified and selected. Choose a thread from this installation. No message was sent."
    );
    Ok(result)
}
fn attachments(args: &Value) -> LegacyResult<Vec<Value>> {
    let encoded = args
        .get("attachments")
        .and_then(Value::as_str)
        .unwrap_or("[]");
    if encoded.len() > 12000 {
        return Err(LegacyError::invalid("Attachment details are too large."));
    }
    let rows: Vec<Value> = serde_json::from_str(encoded)
        .map_err(|_| LegacyError::invalid("Invalid attachment details."))?;
    if rows.len() > 8 {
        return Err(LegacyError::invalid("Use up to eight attachments."));
    }
    let mut total = 0u64;
    let mut ids = std::collections::HashSet::new();
    for a in &rows {
        let id = text(a, "id");
        let typ = text(a, "type");
        let name = text(a, "name");
        let mime = text(a, "mimeType");
        let size = a["sizeBytes"].as_u64().unwrap_or(0);
        let obj = a
            .as_object()
            .ok_or_else(|| LegacyError::invalid("Invalid attachment details."))?;
        let valid_mime = mime
            .split_once('/')
            .map(|(a, b)| {
                !a.is_empty()
                    && !b.is_empty()
                    && a.bytes()
                        .chain(b.bytes())
                        .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
            })
            .unwrap_or(false);
        if obj.len() != 5
            || !id_ok(id, 128)
            || !ids.insert(id.to_owned())
            || !matches!(typ, "image" | "file")
            || name.is_empty()
            || name.chars().count() > 255
            || !valid_mime
            || size == 0
            || size
                > if typ == "image" {
                    10 * 1024 * 1024
                } else {
                    50 * 1024 * 1024
                }
            || (typ == "image"
                && !matches!(
                    mime,
                    "image/jpeg" | "image/png" | "image/webp" | "image/gif"
                ))
        {
            return Err(LegacyError::invalid(
                "Use up to eight valid uploaded attachments.",
            ));
        }
        total = total.saturating_add(size);
    }
    if total > 80 * 1024 * 1024 {
        return Err(LegacyError::invalid("Attachments exceed 80 MiB total."));
    }
    Ok(rows)
}
fn send(state: &AppState, cfg: &Value, conn: &Connection, args: &Value) -> LegacyResult<Value> {
    let thread = text(args, "thread_id");
    checked_thread(thread)?;
    let msg = text(args, "message");
    let key = text(args, "request_key");
    let files = attachments(args)?;
    if (msg.trim().is_empty() && files.is_empty())
        || msg.chars().count() > 12000
        || key.trim().is_empty()
        || key.len() > 120
    {
        return Err(LegacyError::invalid(
            "Use a message up to 12,000 characters and a stable request key.",
        ));
    }
    let hash = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([thread, msg, files])).map_err(|_| unavailable())?
        )
    );
    let destination = target(cfg);
    let saved = state.runtime.update_document(RECEIPTS, |doc| {
        if !doc.is_object() { *doc = json!({}); }
        if !doc["items"].is_object() { doc["items"] = json!({}); }
        let old = &doc["items"][key];
        if !old.is_null() {
            if text(old, "digest") != hash {
                return Err(LegacyError::conflict("This send key belongs to different text, attachments or thread."));
            }
            if text(old, "target") != destination {
                return Err(err("t3_target_changed", 409, "This retry belongs to another T3 installation."));
            }
        } else {
            doc["items"][key] = json!({"digest":hash,"target":destination,"command":Uuid::new_v4().to_string(),"message":Uuid::new_v4().to_string(),"result":null});
        }
        Ok(())
    })?;
    let old = &saved["items"][key];
    if old["result"].is_object() {
        return Ok(old["result"].clone());
    }
    let command_id = text(old, "command");
    let message_id = text(old, "message");
    let command = json!({"type":"message.dispatch","commandId":command_id,"threadId":thread,"messageId":message_id,"text":msg,"attachments":files,"createdBy":"user","creationSource":"web","dispatchMode":{"type":"queue_after_active"}});
    let response = rpc(conn, "orchestration.dispatchCommand", &command).map_err(|e| {
        if e.code == "t3_rejected" {
            e
        } else {
            err(
                "t3_unconfirmed",
                503,
                "Delivery is unconfirmed. Retry the same words and thread; the send key is kept.",
            )
        }
    })?;
    if !response["sequence"].is_number() {
        return Err(err(
            "t3_unconfirmed",
            503,
            "T3 did not return a receipt. Retry the same send key.",
        ));
    }
    let receipt = json!({"accepted":true,"thread_id":thread,"message_id":message_id,"sequence":response["sequence"],"note":"Sent to T3. Queues after active work; starts when the thread can run."});
    state.runtime.update_document(RECEIPTS, |doc| {
        doc["items"][key]["result"] = receipt.clone();
        Ok(())
    })?;
    Ok(receipt)
}

const PROJECTS: &str = "native-projects.json";
pub fn projects(state: &AppState) -> LegacyResult<Value> {
    let doc = state.runtime.read_document(PROJECTS)?;
    let items = doc
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(json!({"items":items}))
}
fn project_list(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let rows = projects(state)?;
    let query = text(args, "query").to_lowercase();
    if query.len() > 200 {
        return Err(LegacyError::invalid("Project query is too long."));
    }
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(50);
    if !(1..=200).contains(&limit) || offset > 100000 {
        return Err(LegacyError::invalid("Invalid project page."));
    }
    let filtered: Vec<Value> = rows["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| {
            format!("{} {}", text(p, "id"), text(p, "name"))
                .to_lowercase()
                .contains(&query)
        })
        .cloned()
        .collect();
    let total = filtered.len();
    let items: Vec<_> = filtered
        .into_iter()
        .skip(offset)
        .take(limit as usize)
        .collect();
    let next = offset.saturating_add(items.len());
    Ok(
        json!({"items":items,"total":total,"next_offset":if next<total {json!(next)} else {Value::Null},"available":true,"connected":false}),
    )
}
fn project_register(state: &AppState, args: &Value, replace: bool) -> LegacyResult<Value> {
    let id = text(args, "id").trim();
    let name = text(args, "name").trim();
    let path = text(args, "path").trim();
    if !id_ok(id, 100)
        || name.is_empty()
        || name.chars().count() > 200
        || path.len() > 4096
        || !Path::new(path).is_absolute()
        || !Path::new(path).is_dir()
    {
        return Err(LegacyError::invalid(
            "Choose an existing absolute project folder and a unique ID and name.",
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| LegacyError::invalid("Project folder is unavailable."))?;
    let canonical = canonical
        .to_str()
        .ok_or_else(|| LegacyError::invalid("Project folder name cannot be represented."))?;
    let row = json!({"id":id,"name":name,"path":canonical});
    state.runtime.update_document(PROJECTS, |doc| {
        if !doc["items"].is_array() {
            doc["items"] = json!([]);
        }
        let items = doc["items"].as_array_mut().unwrap();
        if items.iter().any(|v| {
            (text(v, "id") == id && !replace)
                || (text(v, "id") != id && text(v, "path").eq_ignore_ascii_case(canonical))
        }) {
            return Err(LegacyError::conflict(
                "That project ID or folder is already registered.",
            ));
        }
        if let Some(existing) = items.iter_mut().find(|v| text(v, "id") == id) {
            *existing = row.clone();
            return Ok(());
        }
        if items.len() >= 200 {
            return Err(LegacyError::invalid(
                "At most 200 projects can be registered.",
            ));
        }
        items.push(row.clone());
        Ok(())
    })?;
    Ok(row)
}
fn project_remove(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id");
    if !id_ok(id, 100) {
        return Err(LegacyError::invalid("Choose an exact project ID."));
    }
    let mut found = false;
    state.runtime.update_document(PROJECTS, |doc| {
        if let Some(rows) = doc["items"].as_array_mut() {
            let len = rows.len();
            rows.retain(|v| text(v, "id") != id);
            found = len != rows.len();
        }
        if !found {
            return Err(LegacyError::not_found("Project is not registered."));
        }
        Ok(())
    })?;
    Ok(json!({"removed":true,"id":id}))
}
/// Shared explicit dispatch for other native services. The caller must supply the saved target and a stable key.
pub fn send_message(state: &AppState, args: &Value, expected_target: &str) -> LegacyResult<Value> {
    if expected_target.is_empty() {
        return Err(LegacyError::invalid(
            "Save the exact T3 installation before sending.",
        ));
    }
    let _guard = lock();
    let cfg = config(state)?;
    target_guard(&cfg, expected_target)?;
    let conn = connection(state, &cfg)?;
    send(state, &cfg, &conn, args)
}

/// Discord freezes an explicit preview and its evidence before confirmation.
pub fn bridge_snapshot(row: &Value) -> Value {
    json!({"id":row["id"],"version":row["version"],"title":row["title"],"original":row["original"],"project":row["project"],"desired_outcome":row["desired_outcome"],"attachments":row["attachments"]})
}
pub fn bridge_send(
    state: &AppState,
    args: &Value,
    expected_target: &str,
    snapshot: &Value,
) -> LegacyResult<Value> {
    if expected_target.is_empty() {
        return Err(LegacyError::invalid("Choose an exact T3 installation."));
    }
    let _guard = lock();
    let cfg = config(state)?;
    target_guard(&cfg, expected_target)?;
    let conn = connection(state, &cfg)?;
    manager_send_locked(state, &cfg, &conn, args, Some(snapshot))
}

pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    if path == "/api/cook" {
        return Some(cook(state, payload));
    }
    if path == "/api/upload" {
        return Some(upload_request(state, payload));
    }
    if path != "/api/call" {
        return None;
    }
    let (name, args) = match legacy::call_envelope(payload) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(
        name,
        "t3.distributions"
            | "t3.configure"
            | "t3.select_distribution"
            | "t3.connect"
            | "t3.threads"
            | "t3.read"
            | "t3.preview"
            | "t3.send"
            | "manager.send"
            | "projects.list"
            | "projects.register"
            | "projects.configure"
            | "projects.remove"
    ) {
        return None;
    }
    Some((|| {
        let _guard = lock();
        let allowed: &[&str] = match name {
            "t3.distributions" => &[],
            "t3.configure" => &["distribution", "cli", "home", "port"],
            "t3.select_distribution" => &["distribution", "reconnect", "expected_target"],
            "t3.connect" => &["distribution", "port"],
            "t3.threads" => &["query", "limit"],
            "t3.read" => &["thread_id"],
            "t3.preview" => &["thread_id", "attachment_id"],
            "t3.send" => &["thread_id", "message", "request_key", "attachments"],
            "manager.send" => &[
                "id",
                "thread_id",
                "message",
                "request_key",
                "video_mode",
                "followup",
            ],
            "projects.list" => &["query", "limit", "offset"],
            "projects.register" | "projects.configure" => &["id", "name", "path"],
            "projects.remove" => &["id"],
            _ => &[],
        };
        if args
            .as_object()
            .is_none_or(|o| o.keys().any(|key| !allowed.contains(&key.as_str())))
        {
            return Err(LegacyError::invalid("Unexpected capability argument."));
        }
        if name == "t3.select_distribution"
            && args.get("reconnect").is_some_and(|v| !v.is_boolean())
        {
            return Err(LegacyError::invalid("Reconnect must be true or false."));
        }
        match name {
            "projects.list" => project_list(state, &args),
            "projects.register" => project_register(state, &args, false),
            "projects.configure" => project_register(state, &args, true),
            "projects.remove" => project_remove(state, &args),
            "t3.distributions" => list(state, &config(state)?),
            "t3.configure" => {
                target_guard(&config(state)?, text(payload, "t3_target"))?;
                configure(state, &args)
            }
            "t3.select_distribution" => select_distribution(state, &args, false),
            "t3.connect" => select_distribution(state, &args, true),
            _ => {
                let cfg = config(state)?;
                target_guard(&cfg, text(payload, "t3_target"))?;
                let conn = connection(state, &cfg)?;
                match name {
                    "t3.threads" => threads(&conn, &args),
                    "t3.read" => read(&conn, text(&args, "thread_id")),
                    "t3.preview" => preview(
                        &conn,
                        text(&args, "thread_id"),
                        text(&args, "attachment_id"),
                    ),
                    "t3.send" => send(state, &cfg, &conn, &args),
                    "manager.send" => manager_send_locked(state, &cfg, &conn, &args, None),
                    _ => unreachable!(),
                }
            }
        }
    })())
}

fn upload_bytes(
    conn: &Connection,
    bytes: Vec<u8>,
    name: &str,
    mime: &str,
    mode: &str,
) -> LegacyResult<Value> {
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 {
        return Err(err(
            "too_large",
            413,
            "Attach a nonempty file up to 50 MiB.",
        ));
    }
    let safe_name = name
        .chars()
        .take(220)
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    let safe_name = if safe_name.trim().is_empty() {
        "attachment"
    } else {
        safe_name.trim()
    };
    let mime = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let mime = if mime
        .split_once('/')
        .map(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes()
                    .chain(b.bytes())
                    .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
        })
        .unwrap_or(false)
    {
        mime
    } else {
        "application/octet-stream".into()
    };
    let image = matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    );
    if image && bytes.len() > 10 * 1024 * 1024 {
        return Err(err("too_large", 413, "T3 images must be at most 10 MiB."));
    }
    if !matches!(mode, "original" | "frameboard" | "") {
        return Err(LegacyError::invalid("Choose frameboard or original video."));
    }
    if mime.starts_with("video/") && mode != "original" {
        return Err(LegacyError::unavailable(
            "Video frameboards are unavailable. Choose original video.",
        ));
    }
    let kind = if image { "image" } else { "file" };
    let issue = rpc(
        conn,
        "attachments.createUploadUrl",
        &json!({"type":kind,"name":safe_name,"mimeType":mime,"sizeBytes":bytes.len()}),
    )?;
    let route = text(&issue, "relativeUrl");
    let id = text(&issue, "attachmentId");
    let suffix = route.strip_prefix("/api/attachments/upload/").unwrap_or("");
    if suffix.is_empty()
        || suffix.len() > 4096
        || !suffix
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        || !id_ok(id, 128)
    {
        return Err(unavailable());
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| unavailable())?;
    let size = bytes.len();
    let response = client
        .post(format!("{}{route}", endpoint(conn.port)))
        .header(reqwest::header::CONTENT_TYPE, &mime)
        .body(bytes)
        .send()
        .map_err(|_| unavailable())?;
    if !response.status().is_success() {
        return Err(unavailable());
    }
    Ok(
        json!({"attachment":{"id":id,"type":kind,"name":safe_name,"mimeType":mime,"sizeBytes":size},"note":if mime.starts_with("video/"){"Original video file Ã‚Â· native video understanding depends on the selected T3 provider."}else{""},"thumbnail":null}),
    )
}
fn upload_source(
    state: &AppState,
    conn: &Connection,
    bytes: Vec<u8>,
    name: &str,
    mime: &str,
    mode: &str,
) -> LegacyResult<Value> {
    if mime.starts_with("video/") && mode != "original" {
        let (jpeg, duration) = crate::services::media::video_frameboard(state, &bytes)?;
        let name = format!(
            "{}-frameboard.jpg",
            name.rsplit_once('.').map_or(name, |(stem, _)| stem)
        );
        let mut result = upload_bytes(conn, jpeg, &name, "image/jpeg", "original")?;
        result["note"] = json!(format!("Video frameboard: 9 sampled timestamps over {duration:.1}s; no audio. Original retained in local evidence."));
        return Ok(result);
    }
    upload_bytes(conn, bytes, name, mime, mode)
}
fn upload_request(state: &AppState, payload: &Value) -> LegacyResult<Value> {
    use base64::Engine;
    let _guard = lock();
    let cfg = config(state)?;
    target_guard(&cfg, text(payload, "t3_target"))?;
    checked_thread(text(payload, "thread_id"))?;
    let conn = connection(state, &cfg)?;
    let raw = text(payload, "base64");
    if raw.len() > (50_usize * 1024 * 1024).div_ceil(3) * 4 {
        return Err(err("too_large", 413, "File exceeds 50 MiB."));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| LegacyError::invalid("Invalid upload bytes."))?;
    upload_source(
        state,
        &conn,
        bytes,
        text(payload, "filename"),
        text(payload, "mime_type"),
        text(payload, "video_mode"),
    )
}
const HANDOFFS: &str = "t3-handoffs.json";
fn manager_send_locked(
    state: &AppState,
    cfg: &Value,
    conn: &Connection,
    args: &Value,
    expected_snapshot: Option<&Value>,
) -> LegacyResult<Value> {
    use base64::Engine;
    let id = text(args, "id");
    let thread = text(args, "thread_id");
    let key = text(args, "request_key");
    let mode = if text(args, "video_mode").is_empty() {
        "frameboard"
    } else {
        text(args, "video_mode")
    };
    let followup = text(args, "followup");
    let requested = text(args, "message");
    checked_thread(thread)?;
    if id.is_empty()
        || id.len() > 100
        || key.trim().is_empty()
        || key.len() > 100
        || requested.chars().count() > 12000
        || followup.chars().count() > 2000
        || !matches!(mode, "original" | "frameboard")
    {
        return Err(LegacyError::invalid(
            "Use an itch, exact thread, stable retry key and video mode.",
        ));
    }
    let digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([id, thread, requested, mode, followup])).unwrap()
        )
    );
    let dest = target(cfg);
    let existing = state.runtime.read_document(HANDOFFS)?;
    let existing = &existing["items"][key];
    if !existing.is_null()
        && (text(existing, "digest") != digest || text(existing, "target") != dest)
    {
        return Err(LegacyError::conflict(
            "This handoff key belongs to a different request or T3 installation.",
        ));
    }
    let mut pending = if existing.is_object() {
        existing.clone()
    } else {
        let row = state
            .store()
            .legacy_call("manager.get", &json!({"id":id}))?;
        if expected_snapshot.is_some_and(|expected| expected != &bridge_snapshot(&row)) {
            return Err(err("bridge_preview_changed",409,"This itch changed after the preview. Cancel it and prepare a new preview before sending."));
        }
        let brief = if requested.is_empty() {
            state
                .store()
                .legacy_call("manager.brief", &json!({"id":id}))?["brief"]
                .as_str()
                .unwrap_or("")
                .to_owned()
        } else {
            requested.to_owned()
        };
        let local = crate::services::media::references(state, &row["attachments"])?;
        let brief = if local.is_empty() || expected_snapshot.is_some() {
            brief
        } else {
            brief.replace("Open this itch in DumbManager to inspect it. These local files are not automatically sent to T3.", "The visual evidence is attached to this message. Original video files retain audio; frameboards contain sampled timestamps and no audio.")
        };
        let message = if followup.is_empty() {
            brief
        } else {
            format!("Follow-up request: {followup}\n\n{brief}\n\nContinue from the latest result on this same itch. Recheck the requested change; completed plan steps are history. Preserve earlier evidence and outcomes. Report ready for review; acceptance stays with the user.")
        };
        if message.chars().count() > 12000 || message.trim().is_empty() {
            return Err(LegacyError::invalid(
                "Shorten this handoff to 12,000 characters before sending.",
            ));
        }
        json!({"digest":digest,"target":dest,"itch_id":id,"thread_id":thread,"message":message,"followup":followup,"local":local,"uploaded":[],"result":null})
    };
    if pending["result"].is_object() {
        return finish_handoff(state, key, &pending["result"], text(&pending, "followup"));
    }
    state.runtime.update_document(HANDOFFS, |doc| {
        if !doc["items"].is_object() {
            doc["items"] = json!({});
        }
        doc["items"][key] = pending.clone();
        Ok(())
    })?;
    let local = pending["local"].as_array().cloned().unwrap_or_default();
    let completed = pending["uploaded"].as_array().map(Vec::len).unwrap_or(0);
    let mut sources = Vec::new();
    for entry in local.iter().skip(completed) {
        let data = if mode == "original" && entry["frameboard"].is_object() {
            crate::services::media::read_original(state, text(entry, "id"))?
        } else {
            crate::services::media::read_attachment(state, text(entry, "id"))?
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text(&data, "base64"))
            .map_err(|_| unavailable())?;
        let mut entry = entry.clone();
        if mode == "original" && entry["frameboard"].is_object() {
            entry["mimeType"] = data["mime_type"].clone();
            entry["name"] = entry["frameboard"]["original_name"].clone();
        }
        sources.push((entry, bytes));
    }
    for (entry, bytes) in sources {
        let uploaded = upload_source(
            state,
            conn,
            bytes,
            text(&entry, "name"),
            text(&entry, "mimeType"),
            mode,
        )?;
        pending["uploaded"]
            .as_array_mut()
            .unwrap()
            .push(uploaded["attachment"].clone());
        state.runtime.update_document(HANDOFFS, |doc| {
            doc["items"][key] = pending.clone();
            Ok(())
        })?;
    }
    let sent = send(
        state,
        cfg,
        conn,
        &json!({"thread_id":thread,"message":pending["message"],"request_key":format!("itch:{key}"),"attachments":pending["uploaded"].to_string()}),
    )?;
    let mut result = sent;
    result["id"] = json!(id);
    result["sent_at"] = json!(dumbmanager_core::time::now_iso());
    result["attachment_count"] = json!(pending["uploaded"].as_array().unwrap().len());
    result["thread_url"] = json!(format!("t3-thread://v1/{thread}"));
    pending["result"] = result.clone();
    state.runtime.update_document(HANDOFFS, |doc| {
        doc["items"][key] = pending.clone();
        Ok(())
    })?;
    finish_handoff(state, key, &result, text(&pending, "followup"))
}
fn finish_handoff(
    state: &AppState,
    key: &str,
    result: &Value,
    followup: &str,
) -> LegacyResult<Value> {
    let mut store = state.store();
    let row = store.legacy_record_handoff(&json!({"id":result["id"],"request_key":key,"thread_id":result["thread_id"],"message_id":result["message_id"],"at":result["sent_at"]}))?;
    if !followup.is_empty() {
        let mut feedback = row["feedback"].as_array().cloned().unwrap_or_default();
        if !feedback
            .iter()
            .any(|entry| text(entry, "request_key") == key)
        {
            feedback.push(json!({"at":result["sent_at"],"text":followup,"request_key":key}));
            store.legacy_call("manager.update", &json!({"id":result["id"],"expected_version":row["version"],"changes":{"state":"open","run_state":"not_sent","feedback":feedback}}))?;
        }
    }
    Ok(result.clone())
}
#[derive(Default)]
struct CookState {
    focus: Option<(String, String, String, Instant)>,
    target: Option<(String, Value, String, Instant)>,
    states: HashMap<String, Value>,
    active: bool,
}
fn cook_state() -> &'static Mutex<CookState> {
    static COOK: OnceLock<Mutex<CookState>> = OnceLock::new();
    COOK.get_or_init(|| Mutex::new(CookState::default()))
}
fn cook(state: &AppState, payload: &Value) -> LegacyResult<Value> {
    let action = text(payload, "action");
    let owner = text(payload, "owner");
    let now = Instant::now();
    match action {
        "focus" => {
            if owner.is_empty() || owner.len() > 100 {
                return Err(LegacyError::invalid("Open an itch popup first."));
            }
            let row = state
                .store()
                .legacy_call("manager.get", &json!({"id":text(payload,"id")}))?;
            let mut c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            if c.focus.as_ref().is_some_and(|f| f.0 != owner && f.3 > now)
                && !payload["activate"].as_bool().unwrap_or(false)
            {
                return Ok(json!({"ready":false}));
            }
            c.focus = Some((
                owner.into(),
                text(&row, "id").into(),
                text(&row, "title").into(),
                now + Duration::from_secs(180),
            ));
            Ok(json!({"ready":true,"id":row["id"]}))
        }
        "unfocus" => {
            let mut c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            if c.focus.as_ref().is_some_and(|f| f.0 == owner) {
                c.focus = None;
            }
            Ok(json!({"cleared":true}))
        }
        "evidence_target" => {
            let c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            Ok(c.focus
                .as_ref()
                .filter(|f| f.3 > now)
                .map(|f| json!({"id":f.1,"title":f.2}))
                .unwrap_or(json!({})))
        }
        "prepare" => {
            if owner.is_empty() || owner.len() > 100 {
                return Err(LegacyError::invalid("Open an itch popup first."));
            }
            let args = &payload["args"];
            let key = text(args, "request_key");
            checked_thread(text(args, "thread_id"))?;
            if key.is_empty()
                || key.len() > 100
                || !matches!(text(args, "video_mode"), "frameboard" | "original")
            {
                return Err(LegacyError::invalid("Invalid handoff draft."));
            }
            state
                .store()
                .legacy_call("manager.get", &json!({"id":text(args,"id")}))?;
            let cfg = config(state)?;
            target_guard(&cfg, text(payload, "t3_target"))?;
            let destination = target(&cfg);
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(args).unwrap()));
            let mut c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            if c.target.as_ref().is_some_and(|t| t.0 != owner && t.3 > now)
                && !payload["activate"].as_bool().unwrap_or(false)
            {
                return Ok(json!({"ready":false}));
            }
            if let Some(previous) = c.states.get(key) {
                if text(previous, "digest") != digest || text(previous, "target") != destination {
                    return Err(LegacyError::conflict(
                        "That send key belongs to a different handoff.",
                    ));
                }
            }
            c.target = Some((
                owner.into(),
                args.clone(),
                destination,
                now + Duration::from_secs(180),
            ));
            Ok(json!({"ready":true,"id":args["id"],"request_key":key}))
        }
        "clear" => {
            let mut c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            if c.target.as_ref().is_some_and(|t| t.0 == owner) {
                c.target = None;
            }
            Ok(json!({"cleared":true}))
        }
        "status" => {
            let c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            Ok(c.states
                .get(text(payload, "request_key"))
                .cloned()
                .unwrap_or(json!({"busy":false})))
        }
        "send" => {
            let (args, destination, key) = {
                let mut c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
                let t = c.target.clone().filter(|t| t.3 > now).ok_or_else(|| {
                    err(
                        "no_target",
                        409,
                        "Open an itch popup and choose its T3 thread first.",
                    )
                })?;
                let key = text(&t.1, "request_key").to_owned();
                if owner.is_empty() || owner != t.0 || text(payload, "request_key") != key {
                    return Err(LegacyError::conflict(
                        "The open itch changed. Retry from its popup.",
                    ));
                }
                if let Some(previous) = c.states.get(&key) {
                    if previous["busy"].as_bool().unwrap_or(false) || previous["result"].is_object()
                    {
                        return Ok(previous.clone());
                    }
                }
                if c.active {
                    return Err(err("busy", 409, "An itch is already being sent."));
                }
                let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&t.1).unwrap()));
                c.active = true;
                c.states.insert(key.clone(),json!({"id":t.1["id"],"request_key":key,"busy":true,"digest":digest,"target":t.2}));
                (t.1, t.2, key)
            };
            let result = (|| {
                let _guard = lock();
                let cfg = config(state)?;
                target_guard(&cfg, &destination)?;
                let conn = connection(state, &cfg)?;
                manager_send_locked(state, &cfg, &conn, &args, None)
            })();
            let mut c = cook_state().lock().unwrap_or_else(|e| e.into_inner());
            c.active = false;
            let state_row = c.states.get_mut(&key).unwrap();
            state_row["busy"] = json!(false);
            match result {
                Ok(r) => state_row["result"] = r,
                Err(e) => {
                    state_row["error"] = json!(e.error);
                    state_row["code"] = json!(e.code);
                }
            }
            Ok(state_row.clone())
        }
        _ => Err(LegacyError::invalid("Unknown cook action.")),
    }
}
