//! Explicit screen-region selection. Pixels are read only after a completed drag.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn select() -> LegacyResult<Option<Vec<u8>>> {
    if ACTIVE.swap(true, Ordering::AcqRel) {
        return Err(LegacyError::conflict("Finish the current snip first."));
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.store(false, Ordering::Release);
        }
    }
    let _reset = Reset;
    // The selector owns a message queue and all its GDI objects on this thread.
    std::thread::Builder::new()
        .name("dumbmanager-snip".into())
        .spawn(platform_select)
        .map_err(|_| failed())?
        .join()
        .map_err(|_| failed())?
}
fn failed() -> LegacyError {
    LegacyError::unavailable("Windows could not capture this region. Try again or attach an image.")
}
#[cfg(not(windows))]
fn platform_select() -> LegacyResult<Option<Vec<u8>>> {
    Err(LegacyError::unavailable(
        "Screen snipping requires Windows.",
    ))
}

#[cfg(windows)]
fn platform_select() -> LegacyResult<Option<Vec<u8>>> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus},
            WindowsAndMessaging::*,
        },
    };
    #[derive(Default)]
    struct Selection {
        start: Option<POINT>,
        end: POINT,
        chosen: Option<RECT>,
        origin: POINT,
    }
    fn rectangle(a: POINT, b: POINT) -> RECT {
        RECT {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x),
            bottom: a.y.max(b.y),
        }
    }
    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Selection;
        if data.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let s = &mut *data;
        match message {
            WM_LBUTTONDOWN => {
                if GetCursorPos(&mut s.end).is_ok() {
                    s.start = Some(s.end);
                    SetCapture(hwnd);
                }
            }
            WM_MOUSEMOVE => {
                if s.start.is_some() && GetCursorPos(&mut s.end).is_ok() {
                    let _ = InvalidateRect(Some(hwnd), None, true);
                }
            }
            WM_LBUTTONUP => {
                if let Some(start) = s.start.take() {
                    let _ = GetCursorPos(&mut s.end);
                    let r = rectangle(start, s.end);
                    if r.right - r.left >= 2 && r.bottom - r.top >= 2 {
                        s.chosen = Some(r);
                    }
                }
                let _ = ReleaseCapture();
                let _ = DestroyWindow(hwnd);
            }
            WM_KEYDOWN if wparam.0 == 27 => {
                let _ = DestroyWindow(hwnd);
            }
            WM_RBUTTONDOWN | WM_CLOSE | WM_TIMER => {
                let _ = DestroyWindow(hwnd);
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let _ = SetTextColor(dc, COLORREF(0x00ffffff));
                let _ = SetBkMode(dc, TRANSPARENT);
                let text: Vec<u16> = "[ DM ]  Drag a box to snip. Esc or right-click cancels."
                    .encode_utf16()
                    .collect();
                let _ = TextOutW(dc, 24, 24, &text);
                if let Some(start) = s.start {
                    let mut r = rectangle(start, s.end);
                    r.left -= s.origin.x;
                    r.right -= s.origin.x;
                    r.top -= s.origin.y;
                    r.bottom -= s.origin.y;
                    let brush = CreateSolidBrush(COLORREF(0x00c85cff));
                    for inset in 0..3 {
                        let border = RECT {
                            left: r.left + inset,
                            top: r.top + inset,
                            right: r.right - inset,
                            bottom: r.bottom - inset,
                        };
                        FrameRect(dc, &border, brush);
                    }
                    let _ = DeleteObject(brush.into());
                }
                let _ = EndPaint(hwnd, &paint);
            }
            WM_DESTROY => {
                PostQuitMessage(0);
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                return DefWindowProcW(hwnd, message, wparam, lparam);
            }
            _ => return DefWindowProcW(hwnd, message, wparam, lparam),
        }
        LRESULT(0)
    }
    unsafe fn capture(r: RECT) -> LegacyResult<Vec<u8>> {
        let width = r.right - r.left;
        let height = r.bottom - r.top;
        if !(2..=8192).contains(&width)
            || !(2..=8192).contains(&height)
            || i64::from(width) * i64::from(height) > 16_777_216
        {
            return Err(LegacyError::invalid(
                "Choose a smaller region (up to 16 megapixels).",
            ));
        }
        struct Gdi {
            screen: HDC,
            memory: HDC,
            bitmap: HBITMAP,
            previous: HGDIOBJ,
        }
        impl Drop for Gdi {
            fn drop(&mut self) {
                unsafe {
                    if !self.previous.is_invalid() {
                        SelectObject(self.memory, self.previous);
                    }
                    if !self.bitmap.is_invalid() {
                        let _ = DeleteObject(self.bitmap.into());
                    }
                    if !self.memory.is_invalid() {
                        let _ = DeleteDC(self.memory);
                    }
                    if !self.screen.is_invalid() {
                        ReleaseDC(None, self.screen);
                    }
                }
            }
        }
        let screen = GetDC(None);
        let mut objects = Gdi {
            screen,
            memory: HDC::default(),
            bitmap: HBITMAP::default(),
            previous: HGDIOBJ::default(),
        };
        if screen.is_invalid() {
            return Err(failed());
        }
        objects.memory = CreateCompatibleDC(Some(screen));
        if objects.memory.is_invalid() {
            return Err(failed());
        }
        objects.bitmap = CreateCompatibleBitmap(screen, width, height);
        if objects.bitmap.is_invalid() {
            return Err(failed());
        }
        objects.previous = SelectObject(objects.memory, objects.bitmap.into());
        if objects.previous.is_invalid() {
            return Err(failed());
        }
        BitBlt(
            objects.memory,
            0,
            0,
            width,
            height,
            Some(screen),
            r.left,
            r.top,
            SRCCOPY | CAPTUREBLT,
        )
        .map_err(|_| failed())?;
        // GetDIBits requires the bitmap to be deselected from the memory DC.
        SelectObject(objects.memory, objects.previous);
        objects.previous = HGDIOBJ::default();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        if GetDIBits(
            screen,
            objects.bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        ) != height
        {
            return Err(failed());
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        let bytes = dumbmanager_card_renderer::png_rgba(width as u32, height as u32, &pixels)
            .map_err(|_| failed())?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err(LegacyError::invalid(
                "This snip exceeds 10 MiB. Choose a smaller region.",
            ));
        }
        Ok(bytes)
    }
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).map_err(|_| failed())?.0);
        let class: Vec<u16> = format!("DMRustSnip{}\0", uuid::Uuid::new_v4().simple())
            .encode_utf16()
            .collect();
        let class_name = PCWSTR(class.as_ptr());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_CROSS).map_err(|_| failed())?,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(failed());
        }
        let result = (|| {
            let origin = POINT {
                x: GetSystemMetrics(SM_XVIRTUALSCREEN),
                y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            };
            let mut selection = Box::new(Selection {
                origin,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name,
                w!("DumbManager Rust — Snip evidence"),
                WS_POPUP,
                origin.x,
                origin.y,
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
                None,
                None,
                Some(instance),
                Some((&mut *selection as *mut Selection).cast()),
            )
            .map_err(|_| failed())?;
            if SetLayeredWindowAttributes(hwnd, COLORREF(0), 110, LWA_ALPHA).is_err()
                || SetTimer(Some(hwnd), 1, 120_000, None) == 0
            {
                let _ = DestroyWindow(hwnd);
                return Err(failed());
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            let mut message = MSG::default();
            loop {
                let code = GetMessageW(&mut message, None, 0, 0).0;
                if code <= 0 {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if IsWindow(Some(hwnd)).as_bool() {
                let _ = DestroyWindow(hwnd);
            }
            // Wait for the compositor to remove the overlay before reading the selected pixels.
            #[link(name = "dwmapi")]
            extern "system" {
                fn DwmFlush() -> i32;
            }
            let _ = DwmFlush();
            selection.chosen.map(|rect| capture(rect)).transpose()
        })();
        let _ = UnregisterClassW(class_name, Some(instance));
        result
    }
}
