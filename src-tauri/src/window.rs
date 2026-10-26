//! The main window: bundled UI only, no remote pages, no pop-ups.
//!
//! Remote content is kept out three ways: this navigation policy, the
//! capability file (commands are granted to local pages only) and the CSP in
//! `tauri.conf.json`.

use std::path::PathBuf;

use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::window::Color;
use tauri::{AppHandle, Manager, Url, WebviewUrl};

use crate::{APP_NAME, MAIN_WINDOW};

/// Dark backdrop painted before the page loads, so there is no white flash.
const BACKGROUND: Color = Color(9, 10, 16, 255);

/// Decides which URLs the main window may load.
#[derive(Debug, Clone, Default)]
pub struct NavigationPolicy {
    /// `tauri dev` only: the CLI's local dev server for `../web`.
    dev_server: Option<Url>,
}

impl NavigationPolicy {
    /// Bundled pages only. In development builds the configured dev server
    /// origin is allowed too; release builds ignore it.
    pub fn new(dev_server: Option<Url>) -> Self {
        Self {
            dev_server: dev_server.filter(|_| tauri::is_dev()),
        }
    }

    pub fn allows(&self, url: &Url) -> bool {
        is_bundled_page(url)
            || self
                .dev_server
                .as_ref()
                .is_some_and(|dev| is_http(dev) && is_http(url) && dev.origin() == url.origin())
    }
}

fn is_http(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// True only for the app's own bundled pages: `http(s)://tauri.localhost` on
/// Windows, `tauri://localhost` on other platforms. Everything else (links in
/// itch text, dropped files, `data:`, `javascript:`) is refused.
pub fn is_bundled_page(url: &Url) -> bool {
    let no_credentials = url.username().is_empty() && url.password().is_none();
    let host_is = |expected: &str| url.host_str() == Some(expected) && url.port().is_none();
    no_credentials
        && match url.scheme() {
            "tauri" => host_is("localhost"),
            "http" | "https" => host_is("tauri.localhost"),
            _ => false,
        }
}

pub fn create(app: &AppHandle, webview_data_dir: PathBuf) -> tauri::Result<()> {
    create_named(app, webview_data_dir, APP_NAME)
}

pub(crate) fn create_named(
    app: &AppHandle,
    webview_data_dir: PathBuf,
    name: &str,
) -> tauri::Result<()> {
    let policy = NavigationPolicy::new(app.config().build.dev_url.clone());
    WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title(name)
        .inner_size(1280.0, 820.0)
        .min_inner_size(880.0, 600.0)
        .center()
        .background_color(BACKGROUND)
        // Own WebView2 profile (cookies, storage, cache) inside this app's
        // local data folder, never shared with another app.
        .data_directory(webview_data_dir)
        // The page receives no OS file drops; HTML5 drag and drop keeps working.
        .disable_drag_drop_handler()
        .on_navigation(move |url| policy.allows(url))
        .on_new_window(|_url, _features| NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

/// Brings the main window to the front (tray click, second launch).
pub fn reveal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
