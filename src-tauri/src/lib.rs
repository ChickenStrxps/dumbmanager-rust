//! DumbManager Rust desktop shell.
//!
//! One window with the bundled UI from `../web`, a tray icon and the IPC
//! commands from `docs/ipc-contract.md`. Validation, storage and export
//! checks live in `dumbmanager-core`; this crate wires them into Tauri.
//!
//! Isolation: own identifier (`io.dumbmanager.rust`), own data folder, own
//! WebView2 profile, own taskbar identity. No network listener, startup
//! task, token discovery or contact with the original DumbManager/DumbAPI.

pub mod card;
mod commands;
mod platform;
mod preview;
pub mod services;
mod state;
mod tray;
pub mod window;

use std::error::Error;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, RunEvent};

use dumbmanager_core::{AppPaths, Store};

use crate::state::AppState;

pub const APP_ID: &str = "io.dumbmanager.rust";
pub const APP_NAME: &str = "DumbManager Rust";
pub const MAIN_WINDOW: &str = "main";

/// Explicit console bridge. It shares this installation's SQLite and DPAPI
/// configuration, never a caller-supplied directory or a legacy app profile.
pub fn run_mcp() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--profile" || !matches!(args[1].as_str(), "mcp" | "dots") {
        return Err("Usage: dumbmanager-mcp --profile mcp|dots".into());
    }
    // This is the same dirs::data_dir()/identifier rule used by Tauri's
    // PathResolver::app_data_dir. No appDirectoriesOverride is configured.
    let root = dirs::data_dir()
        .ok_or("Windows application data is unavailable.")?
        .join(APP_ID);
    let paths = AppPaths::new(root)?;
    let store = Store::open(&paths.database_path())?;
    let state = AppState::new(paths, store, None);
    services::voice::serve_stdio(
        &state,
        &args[1],
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )?;
    state.store().checkpoint()?;
    Ok(())
}

pub fn run() {
    if std::env::args_os().nth(1).is_some() {
        fail(
            "The desktop executable does not accept command-line arguments or data-directory overrides.",
        );
    }
    // Before any window exists, so the taskbar uses this app's own identity.
    platform::set_app_user_model_id(APP_ID);

    run_desktop(app_context(), APP_NAME, start);
}

fn app_context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

/// The desktop command handler, plugins and event loop.
fn run_desktop<F>(context: tauri::Context<tauri::Wry>, name: &'static str, start: F)
where
    F: FnOnce(&AppHandle) -> Result<(), Box<dyn Error>> + Send + 'static,
{
    let builder = tauri::Builder::default();
    // Must be the first plugin: a second launch focuses the running window
    // instead of opening the same database from two processes.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        window::reveal(app);
    }));

    let app = builder
        .register_uri_scheme_protocol("dm-preview", |context, request| {
            let html = (request.method() == tauri::http::Method::GET
                && context.webview_label() == MAIN_WINDOW)
                .then(|| {
                    context
                        .app_handle()
                        .try_state::<AppState>()
                        .and_then(|state| state.previews.get(request.uri().path()))
                })
                .flatten();
            tauri::http::Response::builder()
                .status(if html.is_some() { 200 } else { 404 })
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Content-Security-Policy", preview::CSP)
                .header("Cache-Control", "no-store")
                .header("X-Content-Type-Options", "nosniff")
                .body(
                    html.unwrap_or_else(|| {
                        "<p>This preview expired. Reopen it from the thread.</p>".into()
                    })
                    .into_bytes(),
                )
                .expect("static preview response headers")
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_snapshot,
            commands::itch_list,
            commands::itch_save,
            commands::itch_transition,
            commands::settings_get,
            commands::settings_save,
            commands::card_export,
            commands::open_export_folder,
            commands::choose_path,
            commands::legacy_api,
        ])
        .setup(move |app| {
            // Tauri panics on setup errors; show a readable reason instead.
            if let Err(error) = start(app.handle()) {
                fail_named(name, &error.to_string());
            }
            Ok(())
        })
        .build(context);

    match app {
        Ok(app) => app.run(|app, event| {
            if let RunEvent::Exit = event {
                services::companion::shutdown();
                // Leave a self-contained database file behind on a clean exit.
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.store().checkpoint();
                }
            }
        }),
        Err(error) => fail_named(name, &error.to_string()),
    }
}

/// Opens this app's own data, then creates the window and tray icon.
fn start(app: &AppHandle) -> Result<(), Box<dyn Error>> {
    let identifier = &app.config().identifier;
    if identifier != APP_ID {
        return Err(format!(
            "This build is configured with the identifier {identifier:?} instead of {APP_ID:?}. It refuses to run so it cannot touch another app's data."
        )
        .into());
    }
    let data_dir = app.path().app_data_dir()?;
    let webview_dir = app.path().app_local_data_dir()?.join("webview");

    start_at(app, &data_dir, webview_dir, APP_NAME)
}

/// Private startup wiring. No caller-supplied data path reaches this function
/// from the executable, its CLI, or its IPC.
fn start_at(
    app: &AppHandle,
    data_dir: &Path,
    webview_dir: PathBuf,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let opened = AppPaths::new(data_dir).and_then(|paths| {
        let store = Store::open(&paths.database_path())?;
        Ok((paths, store))
    });
    let (paths, store) =
        opened.map_err(|error| format!("{error}\n\nData folder: {}", data_dir.display()))?;

    app.manage(AppState::new(paths, store, card::renderer()));
    window::create_named(app, webview_dir, name)?;
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        if let Ok(hwnd) = window.hwnd() {
            platform::set_clipboard_owner(hwnd.0 as isize);
        }
    }
    tray::create_named(app, name)?;
    services::desktop::install(app);
    services::companion::initialize(app.clone());
    Ok(())
}

fn fail(reason: &str) -> ! {
    fail_named(APP_NAME, reason)
}

fn fail_named(name: &str, reason: &str) -> ! {
    platform::show_error(name, &format!("{name} could not start.\n\n{reason}"));
    std::process::exit(1);
}
