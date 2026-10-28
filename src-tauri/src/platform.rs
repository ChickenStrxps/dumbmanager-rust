//! Small operating-system touches: taskbar identity, opening the exports
//! folder and a native error box. None of them reads or writes app data.

use std::path::Path;

use dumbmanager_core::CoreResult;

pub use imp::{open_folder, set_app_user_model_id, show_error};

/// An explicit native picker returns a path only after the user chooses it.
/// It does not read the selected file or grant a project permission itself.
#[cfg(windows)]
pub fn pick_path(folder: bool) -> CoreResult<Option<String>> {
    use dumbmanager_core::CoreError;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    // This function runs on a dedicated blocking worker, not the WebView thread.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|_| {
                CoreError::Unavailable("Windows could not open the file chooser.".into())
            })?;
        let result = (|| -> windows::core::Result<Option<String>> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | if folder {
                        FOS_PICKFOLDERS
                    } else {
                        FOS_FILEMUSTEXIST
                    },
            )?;
            let owner = CLIPBOARD_OWNER.load(std::sync::atomic::Ordering::Acquire);
            if let Err(error) = dialog.Show(Some(HWND(owner as *mut std::ffi::c_void))) {
                if error.code().0 as u32 == 0x800704c7 {
                    return Ok(None);
                }
                return Err(error);
            }
            let item = dialog.GetResult()?;
            let raw = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = raw.to_string();
            CoTaskMemFree(Some(raw.0.cast()));
            Ok(Some(path?))
        })();
        CoUninitialize();
        result
            .map_err(|_| CoreError::Unavailable("Windows could not read the selected path.".into()))
    }
}
#[cfg(not(windows))]
pub fn pick_path(_folder: bool) -> CoreResult<Option<String>> {
    Err(dumbmanager_core::CoreError::Unavailable(
        "The native file chooser requires Windows.".into(),
    ))
}

static CLIPBOARD_OWNER: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
pub fn set_clipboard_owner(owner: isize) {
    CLIPBOARD_OWNER.store(owner, std::sync::atomic::Ordering::Release);
}

/// A local exported GIF is copied as a file, preserving its animation. The
/// caller resolves the path from this app's renderer, never from page input.
#[cfg(windows)]
pub fn copy_export_file(path: &Path) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    if !path.is_file()
        || std::fs::symlink_metadata(path).map_or(true, |m| m.file_type().is_symlink())
    {
        return Err(dumbmanager_core::CoreError::invalid(
            "The exported file is unavailable.",
        ));
    }
    let mut data = Vec::new();
    // DROPFILES: pFiles, POINT, fNC, fWide; file names are double-NUL-terminated.
    for value in [20u32, 0, 0, 0, 1] {
        data.extend(value.to_le_bytes());
    }
    for value in path.as_os_str().encode_wide().chain([0, 0]) {
        data.extend(value.to_le_bytes());
    }
    clipboard_data(15, &data)
}

#[cfg(not(windows))]
pub fn copy_export_file(_path: &Path) -> CoreResult<()> {
    Err(dumbmanager_core::CoreError::Unavailable(
        "File clipboard handoff requires Windows.".into(),
    ))
}

#[cfg(windows)]
fn clipboard_data(format: u32, bytes: &[u8]) -> CoreResult<()> {
    use std::ffi::c_void;
    #[link(name = "User32")]
    extern "system" {
        fn OpenClipboard(owner: isize) -> i32;
        fn EmptyClipboard() -> i32;
        fn CloseClipboard() -> i32;
        fn SetClipboardData(format: u32, data: *mut c_void) -> *mut c_void;
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, size: usize) -> *mut c_void;
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
        fn GlobalFree(handle: *mut c_void) -> *mut c_void;
    }
    let owner = CLIPBOARD_OWNER.load(std::sync::atomic::Ordering::Acquire);
    if owner == 0 {
        return Err(dumbmanager_core::CoreError::Unavailable(
            "Open the desktop window to copy a card file.".into(),
        ));
    }
    // SAFETY: GMEM_MOVEABLE block is filled before ownership transfers to
    // Windows. Every failure frees it; successful SetClipboardData owns it.
    let okay = unsafe {
        let handle = GlobalAlloc(2, bytes.len());
        if handle.is_null() {
            return Err(dumbmanager_core::CoreError::Unavailable(
                "Windows could not allocate clipboard memory.".into(),
            ));
        }
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            GlobalFree(handle);
            return Err(dumbmanager_core::CoreError::Unavailable(
                "Windows could not access clipboard memory.".into(),
            ));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
        GlobalUnlock(handle);
        if OpenClipboard(owner) == 0 {
            GlobalFree(handle);
            false
        } else {
            let copied = EmptyClipboard() != 0 && !SetClipboardData(format, handle).is_null();
            CloseClipboard();
            if !copied {
                GlobalFree(handle);
            }
            copied
        }
    };
    if okay {
        Ok(())
    } else {
        Err(dumbmanager_core::CoreError::Unavailable(
            "The clipboard is busy. Try Copy again or save the GIF.".into(),
        ))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use dumbmanager_core::CoreError;
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{SetCurrentProcessExplicitAppUserModelID, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, SW_SHOWNORMAL,
    };

    /// Gives the process its own taskbar identity (the installer's shortcut
    /// uses the same ID), so it never groups with another app. Must run
    /// before the first window is created. Returns false if Windows refused.
    pub fn set_app_user_model_id(id: &str) -> bool {
        // SAFETY: plain Win32 call with a valid, NUL-terminated wide string.
        unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(id)) }.is_ok()
    }

    /// Opens `dir` in Explorer. Callers only pass the verified exports folder.
    pub fn open_folder(dir: &Path) -> CoreResult<()> {
        let target = HSTRING::from(dir.as_os_str());
        // ShellExecute may use COM; initialise it on this worker thread as
        // the Win32 documentation recommends, and balance the call.
        // SAFETY: standard COM initialisation/cleanup pairing on one thread.
        let com =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        // SAFETY: all string arguments are valid wide strings or null.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &target,
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if com.is_ok() {
            // SAFETY: matches the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        // ShellExecuteW reports success with a value greater than 32.
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(CoreError::Io(
                "Windows could not open the exports folder.".to_owned(),
            ))
        }
    }

    /// Blocking native error box (works before and without any web view).
    pub fn show_error(title: &str, message: &str) {
        // SAFETY: valid wide strings; no owner window.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from(title),
                MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use dumbmanager_core::CoreError;

    pub fn set_app_user_model_id(_id: &str) -> bool {
        true
    }

    pub fn open_folder(dir: &Path) -> CoreResult<()> {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(dir)
            .spawn()
            .map(drop)
            .map_err(|_| CoreError::Io("Could not open the exports folder.".to_owned()))
    }

    pub fn show_error(title: &str, message: &str) {
        eprintln!("{title}: {message}");
    }
}
