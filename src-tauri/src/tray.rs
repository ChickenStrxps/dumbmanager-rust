//! Tray icon: show the window, open the exports folder, quit.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{include_image, AppHandle, Manager};

use crate::state::AppState;
use crate::{platform, window, APP_NAME};

const TRAY_ID: &str = "dumbmanager-rust-tray";

pub fn create_named(app: &AppHandle, name: &str) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", format!("Show {name}"), true, None::<&str>)?;
    let exports = MenuItem::with_id(
        app,
        "open-exports",
        "Open exports folder",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", format!("Quit {name}"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &exports, &separator, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        // Path is relative to src-tauri; the PNG must be RGBA.
        .icon(include_image!("../assets/icons/32x32.png"))
        .tooltip(name)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => window::reveal(app),
            "open-exports" => open_exports(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::reveal(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Same as the `open_export_folder` command, off the UI thread.
fn open_exports(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let opened = state
            .paths
            .ensure_exports_dir()
            .and_then(|dir| platform::open_folder(&dir));
        if let Err(error) = opened {
            platform::show_error(APP_NAME, &error.to_string());
        }
    });
}
