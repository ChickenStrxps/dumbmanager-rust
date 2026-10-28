//! An explicitly configured, app-owned Chromium session. No client CDP passthrough.
use crate::state::AppState;
use dumbmanager_core::legacy::{call_envelope, LegacyError, LegacyResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

#[path = "browser/advice.rs"]
mod advice;
const SETTINGS: &str = "browser-settings";
const KEY: &str = "browser-jev-key";
const IDLE: Duration = Duration::from_secs(900);
const PAGE: &str = include_str!("browser/page_state.js");
static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
static REAPER: OnceLock<()> = OnceLock::new();
fn sessions() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}
fn err(message: &str, code: &str, status: u16) -> LegacyError {
    LegacyError {
        error: message.into(),
        code: code.into(),
        status,
    }
}
fn stopped() -> LegacyError {
    err(
        "Start the browser first. The temporary session may have expired.",
        "browser_stopped",
        409,
    )
}
fn changed() -> LegacyError {
    err(
        "The page changed. Refresh before acting on it.",
        "browser_changed",
        409,
    )
}
fn unavailable() -> LegacyError {
    err(
        "Browser response unavailable. Refresh before retrying an action.",
        "browser_unavailable",
        503,
    )
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn web_url(input: &str) -> LegacyResult<String> {
    let input = input.trim();
    if input == "about:blank" {
        return Ok(input.into());
    }
    if input.is_empty() || input.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(LegacyError::invalid("Use an HTTP or HTTPS page address."));
    }
    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let u = url::Url::parse(&candidate)
        .map_err(|_| LegacyError::invalid("Use an HTTP or HTTPS page address."))?;
    if !["http", "https"].contains(&u.scheme())
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(LegacyError::invalid(
            "Use an HTTP or HTTPS address without embedded credentials.",
        ));
    }
    Ok(u.to_string())
}
struct Cdp {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
}
impl Cdp {
    fn connect(url: &str, port: u16) -> LegacyResult<Self> {
        let parsed = url::Url::parse(url).map_err(|_| unavailable())?;
        if parsed.scheme() != "ws"
            || parsed.host_str() != Some("127.0.0.1")
            || parsed.port() != Some(port)
            || !parsed.path().starts_with("/devtools/")
            || !parsed.username().is_empty()
        {
            return Err(unavailable());
        }
        let (mut ws, _) = tungstenite::connect(url).map_err(|_| unavailable())?;
        if let MaybeTlsStream::Plain(stream) = ws.get_mut() {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| unavailable())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| unavailable())?;
        }
        Ok(Self { ws, next: 0 })
    }
    fn call(&mut self, method: &str, params: Value) -> LegacyResult<Value> {
        self.next += 1;
        let id = self.next;
        self.ws
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| unavailable())?;
        for _ in 0..500 {
            let message = self.ws.read().map_err(|_| unavailable())?;
            if let Message::Text(t) = message {
                if t.len() > 8 * 1024 * 1024 {
                    return Err(unavailable());
                }
                let v: Value = serde_json::from_str(&t).map_err(|_| unavailable())?;
                if v["id"].as_u64() == Some(id) {
                    return if v.get("error").is_some() {
                        Err(unavailable())
                    } else {
                        Ok(v["result"].clone())
                    };
                }
            }
        }
        Err(unavailable())
    }
}
// A Job Object with KILL_ON_JOB_CLOSE prevents orphaned owned helpers even if the app exits.
#[cfg(windows)]
mod job {
    use std::{ffi::c_void, os::windows::io::AsRawHandle, process::Child};
    #[repr(C)]
    struct Basic {
        time1: i64,
        time2: i64,
        flags: u32,
        min: usize,
        max: usize,
        active: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    struct Io {
        values: [u64; 6],
    }
    #[repr(C)]
    struct Extended {
        basic: Basic,
        io: Io,
        process_mem: usize,
        job_mem: usize,
        peak_process: usize,
        peak_job: usize,
    }
    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread_id: u32,
        owner_pid: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(a: *const c_void, n: *const u16) -> *mut c_void;
        fn SetInformationJobObject(h: *mut c_void, c: i32, p: *const c_void, n: u32) -> i32;
        fn AssignProcessToJobObject(h: *mut c_void, p: *mut c_void) -> i32;
        fn CloseHandle(h: *mut c_void) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Thread32First(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, id: u32) -> *mut c_void;
        fn ResumeThread(thread: *mut c_void) -> u32;
    }
    pub struct Job(*mut c_void);
    unsafe impl Send for Job {}
    impl Job {
        pub fn attach(child: &Child) -> Option<Self> {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let mut info: Extended = std::mem::zeroed();
                info.basic.flags = 0x2000;
                if SetInformationJobObject(
                    h,
                    9,
                    (&info as *const Extended).cast(),
                    std::mem::size_of::<Extended>() as u32,
                ) == 0
                    || AssignProcessToJobObject(h, child.as_raw_handle().cast()) == 0
                {
                    CloseHandle(h);
                    return None;
                }
                Some(Self(h))
            }
        }
        // The root was created suspended. Only its own initial thread may be
        // resumed, and only after it belongs to the kill-on-close job.
        pub fn resume(child: &Child) -> bool {
            unsafe {
                let snapshot = CreateToolhelp32Snapshot(4, 0);
                if snapshot as isize == -1 {
                    return false;
                }
                let mut entry: ThreadEntry = std::mem::zeroed();
                entry.size = std::mem::size_of::<ThreadEntry>() as u32;
                let mut found = false;
                let mut ok = Thread32First(snapshot, &mut entry) != 0;
                while ok {
                    if entry.owner_pid == child.id() {
                        let thread = OpenThread(0x0002, 0, entry.thread_id);
                        if thread.is_null() {
                            found = false;
                            break;
                        }
                        let previous = ResumeThread(thread);
                        CloseHandle(thread);
                        if previous != 1 {
                            found = false;
                            break;
                        }
                        found = true;
                    }
                    ok = Thread32Next(snapshot, &mut entry) != 0;
                }
                CloseHandle(snapshot);
                found
            }
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
#[cfg(not(windows))]
mod job {
    use std::process::Child;
    pub struct Job;
    impl Job {
        pub fn attach(_: &Child) -> Option<Self> {
            Some(Self)
        }
        pub fn resume(_: &Child) -> bool {
            true
        }
    }
}
struct Session {
    child: Child,
    job: Option<job::Job>,
    profile: PathBuf,
    port: u16,
    browser_ws: String,
    tab: String,
    page: Option<Cdp>,
    page_tab: String,
    width: i64,
    height: i64,
    last: Instant,
    frames: VecDeque<(String, Value)>,
    snapshot: String,
    observed: Value,
    suggestion: Option<Value>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.page.take();
        self.job.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.profile);
    }
}
impl Session {
    fn http(&self, route: &str) -> LegacyResult<Value> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| unavailable())?;
        let response = client
            .get(format!("http://127.0.0.1:{}{}", self.port, route))
            .send()
            .map_err(|_| unavailable())?;
        if !response.status().is_success()
            || response.content_length().is_some_and(|x| x > 1_048_576)
        {
            return Err(unavailable());
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        response
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() > 1_048_576 {
            return Err(unavailable());
        }
        serde_json::from_slice(&bytes).map_err(|_| unavailable())
    }
    fn tabs(&self) -> LegacyResult<Vec<Value>> {
        let v = self.http("/json/list")?;
        Ok(v.as_array()
            .ok_or_else(unavailable)?
            .iter()
            .filter(|v| s(v, "type") == "page")
            .take(8)
            .cloned()
            .collect())
    }
    fn touch(&mut self) -> LegacyResult<()> {
        if self.child.try_wait().map_err(|_| stopped())?.is_some() {
            return Err(stopped());
        }
        self.last = Instant::now();
        Ok(())
    }
    fn page(&mut self, touch: bool) -> LegacyResult<&mut Cdp> {
        if touch {
            self.touch()?
        } else if self.child.try_wait().map_err(|_| stopped())?.is_some() {
            return Err(stopped());
        }
        let tabs = self.tabs()?;
        let target = tabs
            .iter()
            .find(|t| s(t, "id") == self.tab)
            .or_else(|| tabs.first())
            .ok_or_else(stopped)?;
        let id = s(target, "id").to_string();
        if self.page.is_none() || self.page_tab != id {
            let mut c = Cdp::connect(s(target, "webSocketDebuggerUrl"), self.port)?;
            c.call("Page.enable", json!({}))?;
            c.call("Page.bringToFront", json!({}))?;
            self.page = Some(c);
            self.page_tab = id.clone();
        }
        self.tab = id;
        let page = self.page.as_mut().unwrap();
        page.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":self.width,"height":self.height,"deviceScaleFactor":1,"mobile":false}),
        )?;
        Ok(page)
    }
    fn browser(&self) -> LegacyResult<Cdp> {
        Cdp::connect(&self.browser_ws, self.port)
    }
    fn document(&mut self) -> LegacyResult<Value> {
        let (tab, w, h) = (self.tab.clone(), self.width, self.height);
        let tree = self.page(false)?.call("Page.getFrameTree", json!({}))?;
        let f = &tree["frameTree"]["frame"];
        Ok(json!([
            tab,
            f.get("loaderId").unwrap_or(&f["id"]),
            f["url"],
            w,
            h
        ]))
    }
    fn read(&mut self) -> LegacyResult<Value> {
        let c = self.page(false)?;
        let tree = c.call("Page.getFrameTree", json!({}))?;
        let world = c.call(
            "Page.createIsolatedWorld",
            json!({"frameId":tree["frameTree"]["frame"]["id"],"worldName":"DumbManager Browse"}),
        )?;
        let result = c.call(
            "Runtime.evaluate",
            json!({"expression":PAGE,"contextId":world["executionContextId"],"returnByValue":true}),
        )?;
        let v = result["result"]["value"].clone();
        if !v["nodes"].is_array() {
            return Err(changed());
        }
        Ok(v)
    }
    fn visual(&mut self) -> LegacyResult<Value> {
        let doc = self.document()?;
        let shot = self.page(false)?.call(
            "Page.captureScreenshot",
            json!({"format":"jpeg","quality":65,"captureBeyondViewport":false}),
        )?;
        let image = s(&shot, "data");
        if image.is_empty() || image.len() > 7_000_000 {
            return Err(unavailable());
        }
        let id = token();
        self.frames.push_back((id.clone(), doc.clone()));
        while self.frames.len() > 8 {
            self.frames.pop_front();
        }
        Ok(
            json!({"running":true,"frame":id,"tab":self.tab,"url":doc[2],"image":image,"mime":"image/jpeg","width":self.width,"height":self.height,"time":dumbmanager_core::time::now_iso()}),
        )
    }
    fn snapshot(&mut self, connected: bool) -> LegacyResult<Value> {
        self.page(true)?;
        let value = self.read()?;
        let mut visual = self.visual()?;
        let tabs=self.tabs()?.iter().map(|t|json!({"id":t["id"],"title":s(t,"title").chars().take(120).collect::<String>(),"url":s(t,"url").chars().take(2000).collect::<String>()})).collect::<Vec<_>>();
        self.snapshot = token();
        self.observed = value.clone();
        self.suggestion = None;
        visual["snapshot"] = json!(self.snapshot);
        visual["url"] = value["url"].clone();
        visual["title"] = value["title"].clone();
        visual["nodes"] = value["nodes"].clone();
        visual["tabs"] = json!(tabs);
        visual["jev_connected"] = json!(connected);
        visual["viewport_token"] = json!(fingerprint(&value));
        Ok(visual)
    }
    fn fresh(&mut self, id: &str) -> LegacyResult<Value> {
        let value = self.read()?;
        if id.is_empty()
            || id != self.snapshot
            || fingerprint(&value) != fingerprint(&self.observed)
        {
            self.snapshot.clear();
            return Err(changed());
        }
        Ok(value)
    }
}
fn fingerprint(v: &Value) -> String {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("text");
        o.remove("passages");
    }
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
fn start(state: &AppState, url: &str, connected: bool) -> LegacyResult<Value> {
    let destination = web_url(if url.is_empty() { "about:blank" } else { url })?;
    let mut guard = sessions().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(session) = guard.as_mut() {
        if session.child.try_wait().ok().flatten().is_none() {
            return session.snapshot(connected);
        }
    }
    *guard = None;
    let config = state.runtime.read_document(SETTINGS)?;
    let executable = Path::new(s(&config, "executable"));
    if !executable.is_absolute() || !executable.is_file() {
        return Err(err(
            "Choose a Chromium or Edge executable in Connections first.",
            "browser_missing",
            503,
        ));
    }
    let base = state.runtime.root().join("browser-sessions");
    fs::create_dir_all(&base).map_err(|_| unavailable())?;
    if fs::symlink_metadata(&base)
        .map_err(|_| unavailable())?
        .file_type()
        .is_symlink()
    {
        return Err(unavailable());
    }
    let profile = base.join(format!("owned-{}", token()));
    fs::create_dir(&profile).map_err(|_| unavailable())?;
    let mut command = Command::new(executable);
    command.args([
        "--headless=new",
        "--remote-debugging-address=127.0.0.1",
        "--remote-debugging-port=0",
        "--window-size=1100,700",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--disable-extensions",
        "--disable-sync",
        "--disable-component-update",
        "--block-new-web-contents",
    ]);
    command
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000004);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => {
            let _ = fs::remove_dir_all(profile);
            return Err(unavailable());
        }
    };
    let job = match job::Job::attach(&child) {
        Some(j) => j,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(profile);
            return Err(err(
                "Could not secure the browser process tree.",
                "browser_unavailable",
                503,
            ));
        }
    };
    if !job::Job::resume(&child) {
        drop(job);
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(profile);
        return Err(err(
            "Could not safely start the browser process.",
            "browser_unavailable",
            503,
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut endpoint = None;
    while Instant::now() < deadline {
        if let Ok(content) = fs::read_to_string(profile.join("DevToolsActivePort")) {
            let mut lines = content.lines();
            if let (Some(Ok(port)), Some(path)) =
                (lines.next().map(str::parse::<u16>), lines.next())
            {
                if path.starts_with("/devtools/browser/") {
                    endpoint = Some((port, format!("ws://127.0.0.1:{port}{path}")));
                    break;
                }
            }
        }
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let (port, browser_ws) = match endpoint {
        Some(e) => e,
        None => {
            drop(job);
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(profile);
            return Err(unavailable());
        }
    };
    let mut session = Session {
        child,
        job: Some(job),
        profile,
        port,
        browser_ws,
        tab: String::new(),
        page: None,
        page_tab: String::new(),
        width: 1100,
        height: 700,
        last: Instant::now(),
        frames: VecDeque::new(),
        snapshot: String::new(),
        observed: Value::Null,
        suggestion: None,
    };
    session
        .browser()?
        .call("Browser.setDownloadBehavior", json!({"behavior":"deny"}))?;
    let tabs = session.tabs()?;
    session.tab = s(tabs.first().ok_or_else(stopped)?, "id").to_string();
    if destination != "about:blank" {
        navigate(&mut session, &destination)?;
        thread::sleep(Duration::from_millis(250));
    }
    let result = session.snapshot(connected)?;
    *guard = Some(session);
    REAPER.get_or_init(|| {
        thread::spawn(|| loop {
            thread::sleep(Duration::from_secs(15));
            let mut g = sessions().lock().unwrap_or_else(|e| e.into_inner());
            if g.as_mut().is_some_and(|s| {
                s.last.elapsed() >= IDLE || s.child.try_wait().ok().flatten().is_some()
            }) {
                *g = None
            }
        });
    });
    Ok(result)
}
fn navigate(session: &mut Session, url: &str) -> LegacyResult<()> {
    let url = web_url(url)?;
    let result = session
        .page(true)?
        .call("Page.navigate", json!({"url":url}))?;
    if result.get("errorText").is_some() {
        return Err(err(
            "This website could not load. Check the connection and try Refresh.",
            "browser_navigation",
            409,
        ));
    }
    Ok(())
}
fn key_event(session: &mut Session, key: &str) -> LegacyResult<()> {
    let code = match key {
        "Enter" => 13,
        "Tab" => 9,
        "Escape" => 27,
        "Backspace" => 8,
        "Delete" => 46,
        "ArrowUp" => 38,
        "ArrowDown" => 40,
        "ArrowLeft" => 37,
        "ArrowRight" => 39,
        "Home" => 36,
        "End" => 35,
        "PageUp" => 33,
        "PageDown" => 34,
        " " => 32,
        "SelectAll" => 65,
        _ => return Err(LegacyError::invalid("Unsupported browser key.")),
    };
    for kind in ["keyDown", "keyUp"] {
        let mut p = json!({"type":kind,"key":if key=="SelectAll"{"a"}else{key},"windowsVirtualKeyCode":code,"modifiers":if key=="SelectAll"{2}else{0}});
        if kind == "keyDown" && (key == "Enter" || key == " ") {
            p["text"] = json!(if key == "Enter" { "\r" } else { " " });
        }
        session.page(true)?.call("Input.dispatchKeyEvent", p)?;
    }
    Ok(())
}
fn click(session: &mut Session, x: i64, y: i64) -> LegacyResult<()> {
    if !(0..session.width).contains(&x) || !(0..session.height).contains(&y) {
        return Err(LegacyError::invalid("Point inside the page preview."));
    }
    for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
        session.page(true)?.call(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":x,"y":y,"button":"left","clickCount":1}),
        )?;
    }
    Ok(())
}
fn interact(session: &mut Session, a: &Value) -> LegacyResult<Value> {
    session.touch()?;
    let doc = session.document()?;
    let frame = s(a, "frame");
    if frame.is_empty()
        || !session
            .frames
            .iter()
            .any(|(id, d)| id == frame && *d == doc)
    {
        return Err(changed());
    }
    let op = s(a, "operation");
    let x = a["x"].as_i64().unwrap_or(0);
    let y = a["y"].as_i64().unwrap_or(0);
    let dx = a["delta_x"].as_i64().unwrap_or(0);
    let dy = a["delta_y"].as_i64().unwrap_or(0);
    let text = s(a, "text");
    let key = if s(a, "key").is_empty() {
        "Enter"
    } else {
        s(a, "key")
    };
    if !["click", "scroll", "type", "key"].contains(&op)
        || !(0..session.width).contains(&x)
        || !(0..session.height).contains(&y)
        || dx.unsigned_abs() > 1400
        || dy.unsigned_abs() > 1400
        || text.chars().count() > 4000
    {
        return Err(LegacyError::invalid(
            "Unsupported or out-of-bounds browser input.",
        ));
    }
    if op == "key"
        && ![
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            " ",
            "SelectAll",
        ]
        .contains(&key)
    {
        return Err(LegacyError::invalid("Unsupported browser key."));
    }
    session.frames.clear();
    session.snapshot.clear();
    session.suggestion = None;
    match op {
        "click" => click(session, x, y)?,
        "scroll" => {
            session.page(true)?.call(
                "Input.dispatchMouseEvent",
                json!({"type":"mouseWheel","x":x,"y":y,"deltaX":dx,"deltaY":dy}),
            )?;
        }
        "type" => {
            session
                .page(true)?
                .call("Input.insertText", json!({"text":text}))?;
        }
        "key" => key_event(session, key)?,
        _ => unreachable!(),
    };
    session.visual()
}
fn action(session: &mut Session, a: &Value, connected: bool) -> LegacyResult<Value> {
    session.touch()?;
    let op = s(a, "operation");
    let text = s(a, "text");
    if text.chars().count() > 4000 {
        return Err(LegacyError::invalid("Text exceeds browser bounds."));
    }
    session.frames.clear();
    match op {
        "viewport" => {
            (session.width, session.height) = match text {
                "phone" => (390, 600),
                "desktop" => (1100, 700),
                _ => return Err(LegacyError::invalid("Choose phone or desktop.")),
            };
        }
        "new_tab" => {
            let tabs = session.tabs()?;
            if tabs.len() >= 8 {
                return Err(LegacyError::invalid("Keep up to eight tabs open."));
            }
            let url = web_url(if s(a, "url").is_empty() {
                "about:blank"
            } else {
                s(a, "url")
            })?;
            let result = session
                .browser()?
                .call("Target.createTarget", json!({"url":"about:blank"}))?;
            session.tab = s(&result, "targetId").to_owned();
            session.page = None;
            navigate(session, &url)?;
        }
        "switch_tab" | "close_tab" => {
            let tabs = session.tabs()?;
            let tab = s(a, "tab");
            if !tabs.iter().any(|t| s(t, "id") == tab) {
                return Err(changed());
            }
            if op == "close_tab" {
                if tabs.len() == 1 {
                    return Err(LegacyError::invalid(
                        "Keep one tab open, or stop the browser.",
                    ));
                }
                session
                    .browser()?
                    .call("Target.closeTarget", json!({"targetId":tab}))?;
                session.tab = s(tabs.iter().find(|t| s(t, "id") != tab).unwrap(), "id").to_owned();
            } else {
                session.tab = tab.to_owned();
            }
            session.page = None;
        }
        "navigate" => navigate(session, s(a, "url"))?,
        "reload" => {
            session.page(true)?.call("Page.reload", json!({}))?;
        }
        "back" | "forward" => {
            let history = session
                .page(true)?
                .call("Page.getNavigationHistory", json!({}))?;
            let index =
                history["currentIndex"].as_i64().unwrap_or(0) + if op == "back" { -1 } else { 1 };
            if index >= 0 {
                if let Some(entry) = history["entries"]
                    .as_array()
                    .and_then(|v| v.get(index as usize))
                {
                    session.page(true)?.call(
                        "Page.navigateToHistoryEntry",
                        json!({"entryId":entry["id"]}),
                    )?;
                }
            }
        }
        _ => {
            let value = session.fresh(s(a, "snapshot"))?;
            let nodes = value["nodes"].as_array().ok_or_else(changed)?;
            let node = nodes.iter().find(|n| s(n, "id") == s(a, "node"));
            match op {
                "node" | "click" => {
                    let (x, y) = if op == "node" {
                        let n = node.ok_or_else(changed)?;
                        (n["x"].as_i64().unwrap_or(-1), n["y"].as_i64().unwrap_or(-1))
                    } else {
                        (a["x"].as_i64().unwrap_or(-1), a["y"].as_i64().unwrap_or(-1))
                    };
                    session.snapshot.clear();
                    click(session, x, y)?;
                }
                "type" => {
                    session.snapshot.clear();
                    session
                        .page(true)?
                        .call("Input.insertText", json!({"text":text}))?;
                }
                "key" => {
                    session.snapshot.clear();
                    key_event(
                        session,
                        if s(a, "key").is_empty() {
                            "Enter"
                        } else {
                            s(a, "key")
                        },
                    )?;
                }
                "scroll_up" | "scroll_down" => {
                    session.snapshot.clear();
                    let (x, y) = (session.width / 2, session.height / 2);
                    session.page(true)?.call("Input.dispatchMouseEvent",json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":if op=="scroll_up"{-450}else{450}}))?;
                }
                "select" | "check" | "fill" => {
                    let n = node.ok_or_else(changed)?;
                    if op == "select"
                        && (s(n, "kind") != "select"
                            || !n["options"]
                                .as_array()
                                .is_some_and(|opts| opts.iter().any(|o| s(o, "value") == text)))
                    {
                        return Err(LegacyError::invalid(
                            "Choose one of the visible select options.",
                        ));
                    }
                    if op == "check"
                        && (!["checkbox", "radio"].contains(&s(n, "input_type"))
                            || !["true", "false"].contains(&text)
                            || (s(n, "input_type") == "radio" && text == "false"))
                    {
                        return Err(LegacyError::invalid(
                            "Choose a valid checkbox or radio state.",
                        ));
                    }
                    if op == "fill" && n["editable"] != true {
                        return Err(changed());
                    }
                    let tree = session.page(true)?.call("Page.getFrameTree", json!({}))?;
                    let world=session.page(true)?.call("Page.createIsolatedWorld",json!({"frameId":tree["frameTree"]["frame"]["id"],"worldName":"DumbManager Browse"}))?;
                    let node_json = serde_json::to_string(s(a, "node")).unwrap();
                    let text_json = serde_json::to_string(text).unwrap();
                    let body=match op {"select"=>format!("el.value={text_json};el.dispatchEvent(new Event('input',{{bubbles:true}}));el.dispatchEvent(new Event('change',{{bubbles:true}}));"),"check"=>format!("if(el.checked!=={})el.click();",text),_=>"if(el.readOnly)return false;el.focus({preventScroll:true});if(el.isContentEditable){const r=document.createRange();r.selectNodeContents(el);const s=getSelection();s.removeAllRanges();s.addRange(r)}else el.select();if(document.activeElement!==el)return false;".into()};
                    let expression=format!("(()=>{{const el=globalThis.__dmBrowse.nodes.get({node_json});if(!el||!el.isConnected||el.disabled)return false;{body}return true}})()");
                    session.snapshot.clear();
                    let result=session.page(true)?.call("Runtime.evaluate",json!({"contextId":world["executionContextId"],"returnByValue":true,"expression":expression}))?;
                    if result["result"]["value"] != true {
                        return Err(changed());
                    }
                    if op == "fill" {
                        session
                            .page(true)?
                            .call("Input.insertText", json!({"text":text}))?;
                    }
                }
                _ => return Err(LegacyError::invalid("Unsupported browser action.")),
            }
        }
    }
    session.snapshot.clear();
    thread::sleep(Duration::from_millis(250));
    session.snapshot(connected)
}
fn configure(state: &AppState, a: &Value) -> LegacyResult<Value> {
    if a.get("executable").is_none_or(|value| !value.is_string()) {
        return Err(LegacyError::invalid("Choose a browser executable path."));
    }
    let path = s(a, "executable");
    if path.len() > 4096 || path.contains('\0') {
        return Err(LegacyError::invalid("Invalid executable path."));
    }
    if !path.is_empty() {
        let p = Path::new(path);
        let name = p
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !p.is_absolute()
            || !p.is_file()
            || ![
                "chrome.exe",
                "msedge.exe",
                "chromium.exe",
                "chrome",
                "chromium",
                "msedge",
            ]
            .contains(&name.as_str())
        {
            return Err(LegacyError::invalid(
                "Choose an existing Chromium or Microsoft Edge executable.",
            ));
        }
    }
    if sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
    {
        return Err(LegacyError::conflict(
            "Stop the browser before changing its executable.",
        ));
    }
    state
        .runtime
        .write_document(SETTINGS, &json!({"executable":path}))?;
    Ok(
        json!({"executable":path,"configured":!path.is_empty(),"note":"Saved; start Browse to verify."}),
    )
}
fn status(state: &AppState) -> LegacyResult<Value> {
    let config = state.runtime.read_document(SETTINGS)?;
    let executable = s(&config, "executable");
    let key_result = state.runtime.secret(KEY);
    let key = key_result.as_ref().ok().and_then(|key| key.as_ref());
    let ai = state.runtime.read_document("ai-settings")?;
    let mercury = ai["mercury"] == true
        && state
            .runtime
            .secret("ai-mercury-key")
            .ok()
            .flatten()
            .is_some();
    let mut g = sessions().lock().unwrap_or_else(|e| e.into_inner());
    if g.as_mut()
        .is_some_and(|x| x.last.elapsed() >= IDLE || x.child.try_wait().ok().flatten().is_some())
    {
        *g = None
    }
    Ok(
        json!({"running":g.is_some(),"installed":!executable.is_empty()&&Path::new(executable).is_file(),"configured":!executable.is_empty(),"executable":executable,"jev_connected":key.is_some(),"key_saved":key.is_some(),"key_notice":if key_result.is_err(){"The saved key could not be unlocked. Re-enter it."}else{""},"key_storage_available":cfg!(windows),"mercury_connected":mercury,"idle_seconds":900,"note":"Separate temporary session. Stops after 15 minutes without a browser action."}),
    )
}
pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    if path != "/api/call" {
        return None;
    }
    let (name, args) = match call_envelope(payload) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    if ![
        "browser.status",
        "browser.configure",
        "browser.connect",
        "browser.start",
        "browser.stop",
        "browser.frame",
        "browser.snapshot",
        "browser.interact",
        "browser.action",
        "browser.suggest",
        "browser.plan",
        "browser.apply",
    ]
    .contains(&name)
    {
        return None;
    }
    Some((|| match name {
        "browser.status" => status(state),
        "browser.configure" => configure(state, &args),
        "browser.connect" => {
            if args.get("api_key").is_some_and(|value| !value.is_string()) {
                return Err(LegacyError::invalid("Enter a TypeSafe key as text."));
            }
            let key = s(&args, "api_key");
            if key.len() > 500 || !key.is_ascii() || key.bytes().any(|c| !(33..127).contains(&c)) {
                return Err(LegacyError::invalid(
                    "Enter a TypeSafe API key without whitespace.",
                ));
            }
            if key.is_empty() {
                state.runtime.remove_secret(KEY)?
            } else {
                state.runtime.set_secret(KEY, key)?
            }
            Ok(
                json!({"jev_connected":!key.is_empty(),"key_saved":!key.is_empty(),"key_notice":"","note":if key.is_empty(){"Jev disconnected. Saved key forgotten."}else{"Key saved with Windows encryption. Configuration has not been tested."}}),
            )
        }
        "browser.start" => start(
            state,
            s(&args, "url"),
            state.runtime.secret(KEY).ok().flatten().is_some(),
        ),
        "browser.stop" => {
            *sessions().lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(
                json!({"running":false,"jev_connected":state.runtime.secret(KEY).ok().flatten().is_some()}),
            )
        }
        "browser.frame" | "browser.snapshot" | "browser.interact" | "browser.action" => {
            let connected = state.runtime.secret(KEY).ok().flatten().is_some();
            let mut g = sessions().lock().unwrap_or_else(|e| e.into_inner());
            let session = g.as_mut().ok_or_else(stopped)?;
            if session.last.elapsed() >= IDLE {
                *g = None;
                return Err(stopped());
            }
            match name {
                "browser.frame" => session.visual(),
                "browser.snapshot" => session.snapshot(connected),
                "browser.interact" => interact(session, &args),
                _ => action(session, &args, connected),
            }
        }
        "browser.suggest" => advice::suggest(state, &args),
        "browser.plan" => advice::plan(state, &args),
        "browser.apply" => advice::apply(state, &args),
        _ => unreachable!(),
    })())
}
