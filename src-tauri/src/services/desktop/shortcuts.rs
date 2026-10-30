//! Opt-in global shortcuts. The Win32 queue exists only after an explicit start.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::AppHandle;
use tauri::Emitter;
static APP: OnceLock<AppHandle> = OnceLock::new();
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
#[derive(Default)]
struct State {
    running: bool,
    starting: bool,
    bindings: BTreeMap<String, String>,
    conflicts: Vec<String>,
    test_until: Option<Instant>,
    hits: Vec<u32>,
    note: String,
}
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
}
fn hit(s: &mut State, id: u32) -> bool {
    if !s.running || !s.bindings.contains_key(&id.to_string()) {
        return false;
    }
    if s.test_until.is_some_and(|until| Instant::now() < until) {
        if !s.hits.contains(&id) {
            s.hits.push(id)
        }
        return false;
    }
    s.test_until = None;
    true
}
fn pressed(id: u32) {
    let real = hit(&mut state().lock().unwrap_or_else(|e| e.into_inner()), id);
    if real {
        if let Some(app) = APP.get() {
            // Keep the foreground app in place for the snipping overlay.
            if id != 5 {
                crate::window::reveal(app);
            }
            let _ = app.emit("desktop-shortcut", id);
        }
    }
}
pub(super) fn reveal() {
    if let Some(app) = APP.get() {
        crate::window::reveal(app);
    }
}
pub fn status() -> Value {
    let s = state().lock().unwrap_or_else(|e| e.into_inner());
    let remaining = s
        .test_until
        .map(|t| {
            t.saturating_duration_since(Instant::now())
                .as_secs_f64()
                .ceil() as u64
        })
        .unwrap_or(0);
    json!({"running":s.running,"bindings":s.bindings,"shortcut_conflicts":s.conflicts,"test_remaining":remaining,"test_hits":s.hits,"note":if s.note.is_empty(){"Global shortcuts are off. Enable them below."}else{&s.note}})
}
pub fn start() -> LegacyResult<Value> {
    if APP.get().is_none() {
        return Err(LegacyError::unavailable(
            "The desktop shortcut host is not installed.",
        ));
    }
    {
        let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            return Ok(status_unlocked(&s));
        }
        if s.starting {
            return Err(LegacyError::conflict("Shortcuts are starting. Try again."));
        }
        s.starting = true;
    }
    let result = platform_start();
    if let Err(e) = result {
        let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
        s.starting = false;
        return Err(e);
    }
    Ok(status())
}
fn status_unlocked(s: &State) -> Value {
    json!({"running":s.running,"bindings":s.bindings,"shortcut_conflicts":s.conflicts,"test_remaining":s.test_until.map(|t|t.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64).unwrap_or(0),"test_hits":s.hits,"note":s.note})
}
pub fn test() -> LegacyResult<Value> {
    start()?;
    {
        let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
        s.hits.clear();
        s.test_until = Some(Instant::now() + Duration::from_secs(15));
    }
    Ok(status())
}
#[cfg(not(windows))]
fn platform_start() -> LegacyResult<()> {
    Err(LegacyError::unavailable(
        "Global shortcuts require Windows.",
    ))
}
#[cfg(windows)]
fn platform_start() -> LegacyResult<()> {
    use std::{ffi::c_void, ptr, sync::mpsc, thread};
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: Point,
        private: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn RegisterHotKey(hwnd: *mut c_void, id: i32, modifiers: u32, key: u32) -> i32;
        fn UnregisterHotKey(hwnd: *mut c_void, id: i32) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: *mut c_void, min: u32, max: u32) -> i32;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    thread::Builder::new().name("dumbmanager-shortcuts".into()).spawn(move||{
        let mut bindings=BTreeMap::new();let mut conflicts=Vec::new();
        for (id,key,label) in [(1,0x49,"I"),(2,0x4d,"M"),(3,0x44,"D"),(4,0x43,"C"),(5,0x53,"S")] {
            let mut assigned=false;
            for (mods,prefix) in [(0x4003,"Ctrl+Alt+"),(0x4006,"Ctrl+Shift+"),(0x4007,"Ctrl+Alt+Shift+")] {
                if unsafe{RegisterHotKey(ptr::null_mut(),id,mods,key)}!=0{bindings.insert(id.to_string(),format!("{prefix}{label}"));assigned=true;break}
            }
            if !assigned {conflicts.push(format!("Ctrl+Alt+{label}"));}
        }
        {
            let mut s = state().lock().unwrap_or_else(|e|e.into_inner());
            s.starting = false;
            s.running = !bindings.is_empty();
            s.bindings = bindings;
            s.conflicts = conflicts;
            s.note = "Shortcuts open this app; send still requires the selected itch and thread. Snips stay local until you explicitly share them.".into();
        }
        let _=tx.send(());
        let mut msg:Msg=unsafe{std::mem::zeroed()};while unsafe{GetMessageW(&mut msg,ptr::null_mut(),0,0)}>0 {if msg.message==0x0312 {pressed(msg.wparam as u32)}}
        for id in 1..=5{unsafe{UnregisterHotKey(ptr::null_mut(),id);}}
        let mut s=state().lock().unwrap_or_else(|e|e.into_inner());s.running=false;s.bindings.clear();s.note="Shortcut listener stopped.".into();
    }).map_err(|_|LegacyError::unavailable("Could not start the shortcut listener."))?;
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| LegacyError::unavailable("Windows shortcut registration did not respond."))?;
    if !state().lock().unwrap_or_else(|e| e.into_inner()).running {
        return Err(LegacyError::unavailable(
            "All shortcut combinations are busy.",
        ));
    }
    Ok(())
}
