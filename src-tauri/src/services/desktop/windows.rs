//! Enumerate once and bind opaque choices to capture items, not reusable HWNDs.
#[cfg(windows)]
mod platform {
    use serde_json::{json, Value};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    };
    use windows::{
        core::IInspectable,
        Foundation::TypedEventHandler,
        Graphics::Capture::GraphicsCaptureItem,
        Win32::{
            Foundation::HWND,
            System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
            UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow},
        },
    };
    use windows_capture::{settings::GraphicsCaptureItemType, window::Window};

    struct Choice {
        token: String,
        title: String,
        item: BoundWindow,
        identity: Identity,
        closed: Arc<AtomicBool>,
    }
    static CHOICES: OnceLock<Mutex<Vec<Choice>>> = OnceLock::new();
    fn choices() -> &'static Mutex<Vec<Choice>> {
        CHOICES.get_or_init(|| Mutex::new(Vec::new()))
    }

    // Only the Window variant is stored. GraphicsCaptureItem is agile and
    // Window is Send; the library's enum also contains non-Send variants.
    pub struct BoundWindow(GraphicsCaptureItem, Window);
    #[derive(Clone, Copy)]
    pub struct Identity {
        hwnd: usize,
        thread: u32,
        process: u32,
    }
    impl Identity {
        fn new(window: Window) -> Option<Self> {
            let hwnd = HWND(window.as_raw_hwnd());
            let mut process = 0;
            let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
            if thread == 0 || process == 0 {
                return None;
            }
            Some(Self {
                hwnd: hwnd.0 as usize,
                thread,
                process,
            })
        }
        pub fn alive(self) -> bool {
            let hwnd = HWND(self.hwnd as *mut _);
            let mut process = 0;
            unsafe {
                IsWindow(Some(hwnd)).as_bool()
                    && GetWindowThreadProcessId(hwnd, Some(&mut process)) == self.thread
                    && process == self.process
            }
        }
    }
    impl TryInto<GraphicsCaptureItemType> for BoundWindow {
        type Error = std::convert::Infallible;
        fn try_into(self) -> Result<GraphicsCaptureItemType, Self::Error> {
            Ok(GraphicsCaptureItemType::Window((self.0, self.1)))
        }
    }

    pub fn windows() -> Vec<Value> {
        // The API is called on Tauri's blocking pool, where the apartment may
        // already be initialized. A dedicated MTA makes creation predictable.
        std::thread::spawn(|| {
            let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
            if !initialized {
                return Vec::new();
            }
            let mut result = Vec::new();
            let mut rows = Vec::new();
            for window in Window::enumerate()
                .unwrap_or_default()
                .into_iter()
                .take(100)
            {
                let title = window.title().unwrap_or_default();
                if title.is_empty() || title == "Program Manager" {
                    continue;
                }
                let Some(identity) = Identity::new(window) else {
                    continue;
                };
                let Ok(GraphicsCaptureItemType::Window((capture, window))) =
                    TryInto::<GraphicsCaptureItemType>::try_into(window)
                else {
                    continue;
                };
                let closed = Arc::new(AtomicBool::new(false));
                let marker = closed.clone();
                if capture
                    .Closed(
                        &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(
                            move |_, _| {
                                marker.store(true, Ordering::Release);
                                Ok(())
                            },
                        ),
                    )
                    .is_err()
                {
                    continue;
                }
                let token = uuid::Uuid::new_v4().simple().to_string();
                rows.push(json!({"hwnd":token,"title":title}));
                result.push(Choice {
                    token,
                    title,
                    item: BoundWindow(capture, window),
                    identity,
                    closed,
                });
            }
            *choices().lock().unwrap_or_else(|e| e.into_inner()) = result;
            unsafe { RoUninitialize() };
            rows
        })
        .join()
        .unwrap_or_default()
    }

    pub fn take(token: &str) -> Option<(BoundWindow, Identity, Value)> {
        let mut choices = choices().lock().unwrap_or_else(|e| e.into_inner());
        let index = choices.iter().position(|c| c.token == token)?;
        let c = choices.swap_remove(index);
        let size = c.item.0.Size().ok()?;
        if c.closed.load(Ordering::Acquire)
            || !c.identity.alive()
            || size.Width <= 0
            || size.Height <= 0
            || size.Width > 4096
            || size.Height > 4096
        {
            return None;
        }
        Some((c.item, c.identity, json!({"hwnd":c.token,"title":c.title})))
    }
}
#[cfg(windows)]
pub use platform::{take, windows, Identity};
#[cfg(not(windows))]
pub fn windows() -> Vec<serde_json::Value> {
    Vec::new()
}
