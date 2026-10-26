//! Small operating-system touches: taskbar identity, opening the exports
//! folder and a native error box. None of them reads or writes app data.

use std::path::Path;

use dumbmanager_core::CoreResult;

pub use imp::{open_folder, set_app_user_model_id, show_error};

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
