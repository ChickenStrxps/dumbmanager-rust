//! Enumerate visible Windows top-level windows only on explicit request.
use serde_json::{json, Value};
#[cfg(windows)]
pub fn windows() -> Vec<Value> {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(
            callback: unsafe extern "system" fn(Hwnd, isize) -> i32,
            param: isize,
        ) -> i32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn IsIconic(h: Hwnd) -> i32;
        fn GetWindowTextLengthW(h: Hwnd) -> i32;
        fn GetWindowTextW(h: Hwnd, p: *mut u16, n: i32) -> i32;
    }
    unsafe extern "system" fn visit(h: Hwnd, param: isize) -> i32 {
        let rows = &mut *(param as *mut Vec<Value>);
        if rows.len() >= 100 {
            return 0;
        }
        if IsWindowVisible(h) == 0 || IsIconic(h) != 0 || GetWindowTextLengthW(h) <= 0 {
            return 1;
        }
        let mut text = [0u16; 1000];
        let len = GetWindowTextW(h, text.as_mut_ptr(), 1000).max(0) as usize;
        let title = String::from_utf16_lossy(&text[..len.min(999)]);
        if !title.is_empty() && title != "Program Manager" {
            rows.push(json!({"hwnd":(h as usize).to_string(),"title":title}));
        }
        1
    }
    let mut rows = Vec::new();
    unsafe { EnumWindows(visit, &mut rows as *mut _ as isize) };
    rows
}
#[cfg(not(windows))]
pub fn windows() -> Vec<Value> {
    Vec::new()
}
