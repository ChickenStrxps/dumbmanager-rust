//! Opt-in debug desktop for manual UI/IPC testing against a fresh OS-temp root.
//! Uses the production context, commands, frontend, window and renderer. Only
//! process identity, visible title and fixed private storage paths differ.
//! Never invoked by the normal executable; absent unless `desktop-qa` is set.

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use dumbmanager_core::{CoreError, CoreResult};

use crate::{app_context, native_check, platform, run_desktop, start_at, APP_ID};

const QA_APP_ID: &str = "io.dumbmanager.rust.ui-qa";
const QA_NAME: &str = "DumbManager Rust [TEMP UI QA - DEVELOPMENT ONLY]";

struct QaRoot {
    root: PathBuf,
    data: PathBuf,
    webview: PathBuf,
}

impl QaRoot {
    /// No path input: generate a fresh direct OS-temp child, atomically refuse
    /// collisions, and leave it on exit for review. Never adopt or delete data.
    fn create() -> CoreResult<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| CoreError::Io(error.to_string()))?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dumbmanager-rust-ui-qa-{}-{stamp}",
            std::process::id()
        ));
        native_check::create_temporary_root(&root)?;
        let data = root.join("data");
        let webview = root.join("webview");
        fs::create_dir(&data)?;
        fs::create_dir(&webview)?;
        let mut marker = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("TEMP-UI-QA.txt"))?;
        writeln!(marker, "{QA_NAME}\nFresh, initially empty storage for manual synthetic input only.\nData: {}\nExports: {}\nWebView: {}\nNo normal app data is used. This root is left for inspection after exit.", data.display(), data.join("exports").display(), webview.display())?;
        marker.sync_all()?;
        Ok(Self {
            root,
            data,
            webview,
        })
    }
}

/// Starts a real native WebView. A successful launch is not a passed UI test:
/// the reviewer must exercise the actual frontend and inspect the results.
pub fn run() -> Result<(), Box<dyn Error>> {
    if std::env::args_os().nth(1).is_some() {
        return Err("native-ui-qa accepts no arguments or data-directory overrides".into());
    }
    let mut context = app_context();
    if context.config().identifier != APP_ID {
        return Err("UI QA requires the unchanged production AppId as its base context".into());
    }
    // Retain the generated assets, ACL and CSP verbatim. The single-instance
    // plugin derives its mutex from this distinct identifier before setup.
    context.config_mut().identifier = QA_APP_ID.to_owned();
    context.config_mut().product_name = Some(QA_NAME.to_owned());
    platform::set_app_user_model_id(QA_APP_ID);
    run_desktop(context, QA_NAME, |app| {
        if app.config().identifier != QA_APP_ID {
            return Err("UI QA refuses to start under a normal app identity".into());
        }
        // Setup runs after single-instance detection: a second QA launch only
        // focuses the existing QA window, without allocating another root.
        let paths = QaRoot::create()?;
        println!("TEMP UI QA root: {}", paths.root.display());
        start_at(app, &paths.data, paths.webview, QA_NAME)
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_roots_are_fresh_direct_temp_children_and_keep_profiles_separate() {
        let first = QaRoot::create().unwrap();
        let second = QaRoot::create().unwrap();
        assert_ne!(first.root, second.root);
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        for paths in [first, second] {
            let root = fs::canonicalize(&paths.root).unwrap();
            assert_eq!(root.parent(), Some(temp.as_path()));
            assert_eq!(fs::canonicalize(&paths.data).unwrap(), root.join("data"));
            assert_eq!(
                fs::canonicalize(&paths.webview).unwrap(),
                root.join("webview")
            );
            assert_eq!(fs::read_dir(&paths.data).unwrap().count(), 0);
            assert_eq!(fs::read_dir(&paths.webview).unwrap().count(), 0);
            assert!(native_check::create_temporary_root(&paths.root).is_err());
            assert!(paths.root.join("TEMP-UI-QA.txt").is_file());
            // Only these generated, inspected direct children are removed.
            fs::remove_dir(&paths.data).unwrap();
            fs::remove_dir(&paths.webview).unwrap();
            fs::remove_file(paths.root.join("TEMP-UI-QA.txt")).unwrap();
            fs::remove_dir(&paths.root).unwrap();
        }
    }
}
