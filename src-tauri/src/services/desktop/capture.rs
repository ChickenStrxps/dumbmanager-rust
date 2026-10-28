//! Explicitly armed, bounded selected-window replay.
#[cfg(windows)]
mod platform {
    use super::super::{replay, windows};
    use dumbmanager_core::legacy::{LegacyError, LegacyResult};
    use serde_json::{json, Value};
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{Arc, Mutex, OnceLock},
        thread,
        time::{Duration, Instant},
    };
    use windows_capture::{
        capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
        encoder::{ImageEncoder, ImageEncoderPixelFormat, ImageFormat},
        frame::Frame,
        graphics_capture_api::InternalCaptureControl,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
    };

    const EVENT_BYTES: usize = 20 * 1024 * 1024;
    const RING_BYTES: usize = 8 * 1024 * 1024;
    type Bytes = Arc<Vec<u8>>;
    struct Event {
        token: String,
        request_key: String,
        frames: Vec<(Instant, Bytes)>,
        bytes: usize,
        started: Instant,
        finish_at: Option<Instant>,
        id: Option<String>,
        truncated: bool,
    }
    struct Shared {
        active: bool,
        window: Value,
        error: Option<String>,
        latest: Option<Bytes>,
        ring: VecDeque<(Instant, Bytes)>,
        ring_bytes: usize,
        event: Option<Event>,
        saving: Option<String>,
        root: PathBuf,
        identity: windows::Identity,
    }
    struct Session {
        shared: Arc<Mutex<Shared>>,
        control: Option<CaptureControl<Handler, String>>,
    }
    static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    fn session() -> &'static Mutex<Option<Session>> {
        SESSION.get_or_init(|| Mutex::new(None))
    }
    fn failure(message: &str) -> LegacyError {
        LegacyError::unavailable(message)
    }

    struct Handler {
        shared: Arc<Mutex<Shared>>,
        last: Option<Instant>,
    }
    impl GraphicsCaptureApiHandler for Handler {
        type Flags = Arc<Mutex<Shared>>;
        type Error = String;
        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self {
                shared: ctx.flags,
                last: None,
            })
        }
        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            control: InternalCaptureControl,
        ) -> Result<(), String> {
            let now = Instant::now();
            if self
                .last
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(100))
            {
                return Ok(());
            }
            self.last = Some(now);
            let width = frame.width();
            let height = frame.height();
            // Limit the CPU staging texture as well as output and event storage.
            if width == 0 || height == 0 || width > 4096 || height > 4096 {
                let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                s.active = false;
                s.error = Some(
                    "Replay stopped: the selected window exceeds 4096 pixels in one dimension."
                        .into(),
                );
                control.stop();
                return Ok(());
            }
            let result = (|| -> Result<Vec<u8>, String> {
                let mut buffer = frame.buffer().map_err(|e| e.to_string())?;
                let pitch = buffer.row_pitch() as usize;
                let source = buffer.as_raw_buffer();
                let scale = (640.0 / width as f64).min(360.0 / height as f64).min(1.0);
                let w = ((width as f64 * scale) as u32).max(1);
                let h = ((height as f64 * scale) as u32).max(1);
                let mut pixels = vec![0u8; w as usize * h as usize * 4];
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        let src = (y * height as usize / h as usize) * pitch
                            + (x * width as usize / w as usize) * 4;
                        let dst = (y * w as usize + x) * 4;
                        pixels[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
                        pixels[dst + 3] = 255;
                    }
                }
                ImageEncoder::new(ImageFormat::Jpeg, ImageEncoderPixelFormat::Bgra8)
                    .and_then(|encoder| encoder.encode(&pixels, w, h))
                    .map_err(|e| e.to_string())
            })();
            match result {
                Ok(bytes) if bytes.len() <= 2 * 1024 * 1024 => {
                    let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                    if s.active {
                        s.latest = Some(Arc::new(bytes));
                    }
                }
                _ => {
                    let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                    s.active = false;
                    s.error = Some("Replay stopped because a frame could not be encoded.".into());
                    control.stop();
                }
            }
            Ok(())
        }
        fn on_closed(&mut self) -> Result<(), String> {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            s.active = false;
            s.error = Some("Replay stopped because the selected window closed.".into());
            Ok(())
        }
    }
    fn sample(shared: Arc<Mutex<Shared>>) {
        loop {
            thread::sleep(Duration::from_millis(100));
            let work = {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                if s.active && !s.identity.alive() {
                    s.active = false;
                    s.error = Some("Replay stopped because the selected window closed.".into());
                }
                if !s.active {
                    s.latest = None;
                    s.ring.clear();
                    s.event = None;
                    drop(s);
                    let control = {
                        let mut slot = session().lock().unwrap_or_else(|e| e.into_inner());
                        slot.as_mut()
                            .filter(|current| Arc::ptr_eq(&current.shared, &shared))
                            .and_then(|current| current.control.take())
                    };
                    if let Some(control) = control {
                        let _ = control.stop();
                    }
                    break;
                }
                let now = Instant::now();
                if let Some(bytes) = s.latest.clone() {
                    s.ring_bytes += bytes.len();
                    s.ring.push_back((now, bytes.clone()));
                    while s.ring.len() > 60 || s.ring_bytes > RING_BYTES {
                        if let Some((_, old)) = s.ring.pop_front() {
                            s.ring_bytes -= old.len();
                        }
                    }
                    if let Some(event) = s.event.as_mut() {
                        let during = event.finish_at.is_none()
                            && now.duration_since(event.started) <= Duration::from_secs(18);
                        let after = event.finish_at.is_some_and(|end| now <= end);
                        if during || after {
                            if event.frames.len() < 300 && event.bytes + bytes.len() <= EVENT_BYTES
                            {
                                event.bytes += bytes.len();
                                event.frames.push((now, bytes));
                            } else {
                                event.truncated = true;
                            }
                        } else if event.finish_at.is_none() {
                            event.truncated = true;
                        }
                    }
                }
                let done = s
                    .event
                    .as_ref()
                    .is_some_and(|e| e.finish_at.is_some_and(|end| now >= end));
                if done {
                    let event = s.event.take().unwrap();
                    let id = event.id.clone().unwrap();
                    s.saving = Some(id.clone());
                    Some((event, id, s.root.clone(), s.window.clone()))
                } else {
                    None
                }
            };
            if let Some((event, id, root, window)) = work {
                let frames: Vec<(f64, Vec<u8>)> = event
                    .frames
                    .first()
                    .map(|first| {
                        event
                            .frames
                            .iter()
                            .map(|(t, data)| {
                                (t.duration_since(first.0).as_secs_f64(), (**data).clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let result = replay::save(&root, &id, &frames, event.truncated, &window);
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.saving = None;
                if let Err(e) = result {
                    s.error = Some(format!("Replay could not be saved: {}", e.error));
                }
            }
        }
    }
    pub fn arm(root: PathBuf, token: &str) -> LegacyResult<Value> {
        let mut slot = session().lock().unwrap_or_else(|e| e.into_inner());
        if slot
            .as_ref()
            .is_some_and(|s| s.shared.lock().unwrap_or_else(|e| e.into_inner()).active)
        {
            return Err(LegacyError::conflict(
                "Turn replay off before choosing another window.",
            ));
        }
        if let Some(mut old) = slot.take() {
            if let Some(control) = old.control.take() {
                let _ = control.stop();
            }
        }
        let (item, identity, window) = windows::take(token).ok_or_else(|| failure("That window is closed, unavailable, or larger than 4096 pixels. Refresh apps and choose it again."))?;
        let shared = Arc::new(Mutex::new(Shared {
            active: true,
            window,
            error: None,
            latest: None,
            ring: VecDeque::new(),
            ring_bytes: 0,
            event: None,
            saving: None,
            root,
            identity,
        }));
        let settings = Settings::new(
            item,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            shared.clone(),
        );
        let control = Handler::start_free_threaded(settings)
            .map_err(|_| failure("Windows could not start capture for this selected window."))?;
        let worker = shared.clone();
        if thread::Builder::new()
            .name("dumbmanager-replay".into())
            .spawn(move || sample(worker))
            .is_err()
        {
            let _ = control.stop();
            return Err(failure("Could not start replay buffering."));
        }
        *slot = Some(Session {
            shared,
            control: Some(control),
        });
        Ok(status_from(slot.as_ref()))
    }
    fn status_from(slot: Option<&Session>) -> Value {
        if let Some(session) = slot {
            let mut s = session.shared.lock().unwrap_or_else(|e| e.into_inner());
            if session
                .control
                .as_ref()
                .is_some_and(|control| control.is_finished())
                && s.active
            {
                s.active = false;
                s.error = Some("Windows capture stopped unexpectedly.".into());
            }
            return json!({"armed":s.active,"window":if s.active {s.window.clone()} else {Value::Null},
                "buffer_seconds": if s.active {s.ring.len() as f64 / 10.0} else {0.0},
                "buffer_bytes":if s.active {s.ring_bytes} else {0},"pending":usize::from(s.saving.is_some() || s.event.as_ref().is_some_and(|e| e.id.is_some())),
                "error":s.error,"frames_per_second":10,"max_size":"640x360","idle_capture_processes":0,
                "note":"One selected window, no audio."});
        }
        json!({"armed":false,"window":null,"buffer_seconds":0,"buffer_bytes":0,"pending":0,"error":null,"frames_per_second":10,"max_size":"640x360","idle_capture_processes":0,"note":"Replay is off."})
    }
    pub fn status() -> Value {
        let slot = session().lock().unwrap_or_else(|e| e.into_inner());
        status_from(slot.as_ref())
    }
    pub fn disarm() -> Value {
        let mut slot = session().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut old) = slot.take() {
            old.shared.lock().unwrap_or_else(|e| e.into_inner()).active = false;
            if let Some(control) = old.control.take() {
                let _ = control.stop();
            }
        }
        status_from(None)
    }
    pub fn begin(request_key: &str) -> Value {
        let slot = session().lock().unwrap_or_else(|e| e.into_inner());
        let mut token = None;
        if let Some(session) = slot.as_ref() {
            let mut s = session.shared.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let replace = s.event.as_ref().is_none_or(|e| {
                e.id.is_none()
                    && (e.request_key != request_key
                        || now.duration_since(e.started) > Duration::from_secs(18))
            });
            if s.active && !request_key.is_empty() && request_key.len() <= 128 && s.saving.is_none()
            {
                if replace {
                    let frames: Vec<_> = s.ring.iter().cloned().collect();
                    let bytes = frames.iter().map(|(_, b)| b.len()).sum();
                    s.event = Some(Event {
                        token: uuid::Uuid::new_v4().simple().to_string(),
                        request_key: request_key.to_owned(),
                        frames,
                        bytes,
                        started: now,
                        finish_at: None,
                        id: None,
                        truncated: false,
                    });
                }
                token = s
                    .event
                    .as_ref()
                    .filter(|e| e.id.is_none() && e.request_key == request_key)
                    .map(|e| e.token.clone());
            }
        }
        let mut result = status_from(slot.as_ref());
        result["event_token"] = json!(token);
        result
    }
    pub fn cancel(request_key: &str) -> Value {
        let slot = session().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = slot.as_ref() {
            let mut s = session.shared.lock().unwrap_or_else(|e| e.into_inner());
            if s.event
                .as_ref()
                .is_some_and(|e| e.id.is_none() && e.request_key == request_key)
            {
                s.event = None;
            }
        }
        status_from(slot.as_ref())
    }
    pub fn finish(id: &str, token: &str, request_key: &str) {
        let Ok(id) = uuid::Uuid::parse_str(id).map(|id| id.simple().to_string()) else {
            return;
        };
        let slot = session().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = slot.as_ref() {
            let mut s = session.shared.lock().unwrap_or_else(|e| e.into_inner());
            if !s.active || s.saving.is_some() || token.is_empty() {
                return;
            }
            if let Some(event) = s.event.as_mut() {
                if event.id.is_none()
                    && event.token == token
                    && event.request_key == request_key
                    && event.started.elapsed() <= Duration::from_secs(18)
                {
                    event.id = Some(id);
                    event.finish_at = Some(Instant::now() + Duration::from_secs(6));
                }
            }
        }
    }
    pub fn pending(id: &str) -> bool {
        let slot = session().lock().unwrap_or_else(|e| e.into_inner());
        slot.as_ref().is_some_and(|session| {
            let s = session.shared.lock().unwrap_or_else(|e| e.into_inner());
            s.saving.as_deref() == Some(id)
                || s.event.as_ref().and_then(|e| e.id.as_deref()) == Some(id)
        })
    }
}
#[cfg(windows)]
pub use platform::{arm, begin, cancel, disarm, finish, pending, status};
#[cfg(not(windows))]
mod unsupported {
    use dumbmanager_core::legacy::{LegacyError, LegacyResult};
    use serde_json::{json, Value};
    use std::path::PathBuf;
    pub fn status() -> Value {
        json!({"armed":false,"window":null,"buffer_seconds":0,"buffer_bytes":0,"pending":0,"error":"Selected-window replay requires Windows.","frames_per_second":10,"max_size":"640x360","idle_capture_processes":0})
    }
    pub fn arm(_: PathBuf, _: &str) -> LegacyResult<Value> {
        Err(LegacyError::unavailable(
            "Selected-window replay requires Windows.",
        ))
    }
    pub fn begin(_: &str) -> Value {
        status()
    }
    pub fn cancel(_: &str) -> Value {
        status()
    }
    pub fn disarm() -> Value {
        status()
    }
    pub fn finish(_: &str, _: &str, _: &str) {}
    pub fn pending(_: &str) -> bool {
        false
    }
}
#[cfg(not(windows))]
pub use unsupported::{arm, begin, cancel, disarm, finish, pending, status};
