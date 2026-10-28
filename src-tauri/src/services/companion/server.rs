//! Explicitly started, paired HTTP transport for the bundled phone UI.
//! No filesystem paths, provider secrets or native IPC are exposed by this server.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::Manager;
use tiny_http::{Header, Method, Request, Response, StatusCode};

const COOKIE: &str = "dm_rust_phone";
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; media-src 'self' blob:; connect-src 'self'; worker-src 'self' blob:; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const PAIR_HTML: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Pair DumbManager Rust</title><link rel="stylesheet" href="/style.css"><script defer src="/companion-remote.js"></script></head><body><main class="tool-card" style="max-width:32rem;margin:10vh auto;padding:2rem"><pre aria-hidden="true">[ DM ] ~ &gt;&lt;(((°&gt;</pre><h1>Connect this phone</h1><p>Open a fresh invitation from Connections in your desktop app. The invitation expires in five minutes and works once.</p><p id="phone-pair-status" role="status">Waiting for an invitation.</p><button class="primary" id="phone-pair-button" hidden>Connect to this desktop</button><p>Use a trusted network for a direct HTTP connection. Use your own HTTPS proxy for access outside it.</p></main></body></html>"#;

struct Session {
    csrf: String,
    expires: Instant,
}
struct Auth {
    invite: Option<(String, Instant)>,
    sessions: HashMap<String, Session>,
    attempts: u32,
    window: Instant,
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            invite: None,
            sessions: HashMap::new(),
            attempts: 0,
            window: Instant::now(),
        }
    }
}
struct Control {
    stop: Arc<AtomicBool>,
    task: JoinHandle<()>,
    auth: Arc<Mutex<Auth>>,
    origin: String,
}
fn control() -> &'static Mutex<Option<Control>> {
    static C: OnceLock<Mutex<Option<Control>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}
fn token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |n, (a, b)| n | (a ^ b)) == 0
}
fn header<'a>(r: &'a Request, name: &'static str) -> &'a str {
    r.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
        .unwrap_or("")
}
fn cookie(r: &Request) -> &str {
    header(r, "Cookie")
        .split(';')
        .find_map(|s| s.trim().strip_prefix(&format!("{COOKIE}=")))
        .unwrap_or("")
}
fn json_response(status: u16, value: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(&value).unwrap_or_default())
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
}
fn respond(r: Request, mut response: Response<std::io::Cursor<Vec<u8>>>) {
    for (k, v) in [
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        ("Content-Security-Policy", CSP),
        (
            "Permissions-Policy",
            "camera=(), microphone=(), geolocation=()",
        ),
    ] {
        response.add_header(Header::from_bytes(k, v).unwrap());
    }
    let _ = r.respond(response);
}
fn error(r: Request, status: u16, message: &str) {
    respond(
        r,
        json_response(status, json!({"error":message,"code":"phone_access"})),
    );
}
fn body(r: &mut Request, limit: usize) -> LegacyResult<Value> {
    if !header(r, "Content-Type")
        .split(';')
        .next()
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
        || r.body_length().is_none_or(|n| n == 0 || n > limit)
    {
        return Err(LegacyError::invalid(
            "A bounded JSON request with Content-Length is required.",
        ));
    }
    let mut bytes = Vec::new();
    r.as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LegacyError::invalid("Could not read request."))?;
    if bytes.len() > limit {
        return Err(LegacyError::invalid("Request is too large."));
    }
    serde_json::from_slice(&bytes).map_err(|_| LegacyError::invalid("Invalid JSON request."))
}
fn api_allowed(path: &str, p: &Value) -> bool {
    if matches!(
        path,
        "/api/itch-upload" | "/api/itch-media" | "/api/upload" | "/api/cook"
    ) {
        return true;
    }
    if path != "/api/call" {
        return false;
    }
    matches!(
        p["name"].as_str().unwrap_or(""),
        "manager.state"
            | "manager.status"
            | "manager.capture"
            | "manager.attach"
            | "manager.get"
            | "manager.list"
            | "manager.history"
            | "manager.update"
            | "manager.repeat"
            | "manager.brief"
            | "manager.merge"
            | "manager.context"
            | "manager.context.select"
            | "manager.context.publish"
            | "manager.settings"
            | "manager.typing_status"
            | "manager.suggest"
            | "projects.list"
            | "t3.distributions"
            | "t3.status"
            | "t3.threads"
            | "t3.thread"
            | "t3.read"
            | "t3.send"
            | "t3.messages"
            | "manager.send"
            | "manager.research.get"
            | "manager.research.prepare"
            | "manager.research.list"
            | "manager.research.send"
            | "manager.learning.get"
            | "manager.learning.progress"
            | "manager.inspiration.list"
            | "manager.inspiration.get"
            | "manager.inspiration.save"
            | "manager.inspiration.hide"
            | "manager.social.list"
            | "manager.social.draft"
            | "manager.social.save"
            | "manager.social.preview"
            | "manager.social.gif_prepare"
            | "manager.social.gif_store"
            | "manager.social.themes"
            | "manager.social.image"
            | "manager.social.image_review"
            | "manager.social.check"
            | "manager.social.link"
            | "manager.social.unlink"
            | "manager.social.webhook_preview"
            | "manager.social.webhook_status"
    )
}
fn handle(app: &AppHandle, auth: &Mutex<Auth>, origins: &[String], mut r: Request) {
    let authority = header(&r, "Host");
    if !origins.iter().any(|o| {
        o.split_once("://")
            .is_some_and(|(_, host)| host.eq_ignore_ascii_case(authority))
    }) {
        error(
            r,
            403,
            "This host is not configured for the phone listener.",
        );
        return;
    }
    let path = r.url().to_owned();
    if path.len() > 120 || path.contains(['?', '#', '%', '\\']) || path.contains("..") {
        error(r, 404, "Not found.");
        return;
    }
    if r.method() == &Method::Post {
        if !origins.iter().any(|o| o == header(&r, "Origin"))
            || !matches!(header(&r, "Sec-Fetch-Site"), "" | "same-origin" | "none")
        {
            error(r, 403, "Use this listener's own phone page.");
            return;
        }
        if path == "/api/pair" {
            let p = match body(&mut r, 1024) {
                Ok(p) => p,
                Err(_) => {
                    error(r, 400, "Invalid pairing request.");
                    return;
                }
            };
            let mut a = auth.lock().unwrap_or_else(|e| e.into_inner());
            if a.window.elapsed() > Duration::from_secs(60) {
                a.window = Instant::now();
                a.attempts = 0;
            }
            a.attempts += 1;
            if a.attempts > 12 {
                drop(a);
                error(r, 429, "Too many pairing attempts. Wait one minute.");
                return;
            }
            a.sessions.retain(|_, s| s.expires > Instant::now());
            let valid = a.invite.as_ref().is_some_and(|(key, expires)| {
                *expires > Instant::now() && equal(key, p["invite"].as_str().unwrap_or(""))
            });
            if !valid || a.sessions.len() >= 8 {
                drop(a);
                error(r,403,"Invitation expired, was used, or the device limit was reached. Create a new invitation or revoke devices on desktop.");
                return;
            }
            a.invite = None;
            let sid = token();
            let csrf = token();
            a.sessions.insert(
                sid.clone(),
                Session {
                    csrf: csrf.clone(),
                    expires: Instant::now() + Duration::from_secs(86400),
                },
            );
            let secure = header(&r, "Origin").starts_with("https://");
            drop(a);
            let response = json_response(200, json!({"paired":true,"csrf":csrf})).with_header(
                Header::from_bytes(
                    "Set-Cookie",
                    format!(
                        "{COOKIE}={sid}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400{}",
                        if secure { "; Secure" } else { "" }
                    ),
                )
                .unwrap(),
            );
            respond(r, response);
            return;
        }
    }
    let session = {
        let mut a = auth.lock().unwrap_or_else(|e| e.into_inner());
        a.sessions.retain(|_, s| s.expires > Instant::now());
        a.sessions.get(cookie(&r)).map(|s| s.csrf.clone())
    };
    if r.method() == &Method::Get {
        if path == "/api/session" {
            match session {
                Some(csrf) => respond(r, json_response(200, json!({"csrf":csrf}))),
                None => error(r, 401, "Pair this phone again from the desktop app."),
            };
            return;
        }
        if path == "/" && session.is_none() {
            respond(
                r,
                Response::from_data(PAIR_HTML.as_bytes()).with_header(
                    Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap(),
                ),
            );
            return;
        }
        if session.is_none() && !matches!(path.as_str(), "/style.css" | "/companion-remote.js") {
            error(r, 401, "Pair this phone first.");
            return;
        }
        // Only bundled frontend files; never resolve request paths on disk.
        let asset_path = if path == "/" {
            "index.html"
        } else {
            path.trim_start_matches('/')
        };
        if !asset_path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'/'))
            || ![".html", ".js", ".css", ".ttf", ".png", ".svg", ".ico"]
                .iter()
                .any(|ext| asset_path.ends_with(ext))
        {
            error(r, 404, "Not found.");
            return;
        }
        if let Some(mut asset) = app.asset_resolver().get(asset_path.into()) {
            if asset_path == "index.html" {
                asset.bytes = String::from_utf8_lossy(&asset.bytes)
                    .replacen(
                        "<head>",
                        "<head><script src=\"/companion-remote.js\"></script>",
                        1,
                    )
                    .into_bytes();
            }
            respond(
                r,
                Response::from_data(asset.bytes)
                    .with_header(Header::from_bytes("Content-Type", asset.mime_type).unwrap()),
            );
        } else {
            error(r, 404, "Bundled file not found.");
        }
        return;
    }
    if r.method() != &Method::Post {
        error(r, 405, "Use GET or POST.");
        return;
    }
    if session
        .as_ref()
        .is_none_or(|csrf| !equal(csrf, header(&r, "X-DM-CSRF")))
    {
        error(r, 403, "Pair this phone again before making changes.");
        return;
    }
    let limit = if matches!(path.as_str(), "/api/itch-upload" | "/api/upload") {
        69_910_000
    } else if path == "/api/call" {
        21_037_056
    } else {
        65_536
    };
    let p = match body(&mut r, limit) {
        Ok(p) => p,
        Err(e) => {
            respond(
                r,
                json_response(e.status, json!({"error":e.error,"code":e.code})),
            );
            return;
        }
    };
    if !api_allowed(&path, &p) {
        error(r, 403, "This action is available in the desktop app only.");
        return;
    }
    let state = app.state::<AppState>();
    match state.legacy_api(&path, &p) {
        Ok(v) if v["__native_binary"] == true => {
            match STANDARD.decode(v["base64"].as_str().unwrap_or("")) {
                Ok(bytes) => respond(
                    r,
                    Response::from_data(bytes).with_header(
                        Header::from_bytes(
                            "Content-Type",
                            v["mime_type"]
                                .as_str()
                                .unwrap_or("application/octet-stream"),
                        )
                        .unwrap(),
                    ),
                ),
                Err(_) => error(r, 500, "Stored attachment could not be read."),
            }
        }
        Ok(v) => respond(r, json_response(200, v)),
        Err(e) => respond(
            r,
            json_response(e.status, json!({"error":e.error,"code":e.code})),
        ),
    }
}
pub fn start(app: AppHandle, c: &Value) -> LegacyResult<()> {
    let mut current = control().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = current.as_ref() {
        if !c.task.is_finished() {
            return Ok(());
        }
    }
    if let Some(c) = current.take() {
        let _ = c.task.join();
    }
    let bind = format!(
        "{}:{}",
        c["bind"].as_str().unwrap_or("127.0.0.1"),
        c["port"].as_u64().unwrap_or(8877)
    );
    let server = tiny_http::Server::http(&bind).map_err(|_| {
        LegacyError::conflict(
            "Could not bind the phone listener. Choose an available local address and port.",
        )
    })?;
    let direct = url::Url::parse(&format!("http://{bind}"))
        .map_err(|_| LegacyError::invalid("Invalid listener address."))?
        .origin()
        .ascii_serialization();
    let proxy = c["proxy_origin"].as_str().unwrap_or("");
    let origin = if proxy.is_empty() {
        direct.clone()
    } else {
        proxy.to_owned()
    };
    let mut origins = vec![direct];
    if !proxy.is_empty() {
        origins.push(proxy.to_owned());
    }
    let auth = Arc::new(Mutex::new(Auth::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let a = auth.clone();
    let s = stop.clone();
    let task = std::thread::Builder::new()
        .name("dumbmanager-phone".into())
        .spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            while !s.load(Ordering::Acquire) {
                let request = match server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(request)) => request,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                if active.load(Ordering::Acquire) >= 8 {
                    error(request, 503, "Phone listener is busy. Try again.");
                    continue;
                }
                active.fetch_add(1, Ordering::AcqRel);
                let count = active.clone();
                let auth = a.clone();
                let app = app.clone();
                let origins = origins.clone();
                struct Done(Arc<AtomicUsize>);
                impl Drop for Done {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                let guard = Done(count);
                let _ = std::thread::Builder::new()
                    .name("dumbmanager-phone-request".into())
                    .spawn(move || {
                        let _guard = guard;
                        handle(&app, &auth, &origins, request);
                    });
            }
        })
        .map_err(|_| LegacyError::unavailable("Could not start the phone listener."))?;
    *current = Some(Control {
        stop,
        task,
        auth,
        origin,
    });
    Ok(())
}
pub fn stop() {
    let old = control().lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(c) = old {
        c.auth
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .clear();
        c.stop.store(true, Ordering::Release);
        let _ = c.task.join();
    }
}
pub fn running() -> bool {
    control()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|c| !c.task.is_finished())
}
pub fn pairing_active() -> bool {
    control()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|c| {
            c.auth
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .invite
                .as_ref()
                .is_some_and(|(_, t)| *t > Instant::now())
        })
}
pub fn invite(_: &Value) -> LegacyResult<Value> {
    let current = control().lock().unwrap_or_else(|e| e.into_inner());
    let c = current
        .as_ref()
        .ok_or_else(|| LegacyError::conflict("Start the phone listener first."))?;
    let key = token();
    c.auth.lock().unwrap_or_else(|e| e.into_inner()).invite =
        Some((key.clone(), Instant::now() + Duration::from_secs(300)));
    Ok(json!({"url":format!("{}/#invite={key}",c.origin),"expires_in":300,"one_time":true}))
}
pub fn revoke() {
    if let Some(c) = control().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let mut a = c.auth.lock().unwrap_or_else(|e| e.into_inner());
        a.invite = None;
        a.sessions.clear();
    }
}
