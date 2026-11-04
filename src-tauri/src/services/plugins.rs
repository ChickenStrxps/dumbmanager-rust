//! App-owned catalog and package shelf. Never edits another MCP client's config.
use crate::state::AppState;
use dumbmanager_core::{
    legacy::{self, LegacyError, LegacyResult},
    time,
};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path, sync::Mutex, time::Duration};

static INSTALL: Mutex<()> = Mutex::new(());
const SHELF: &str = "plugins-shelf.json";

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn catalog() -> Vec<Value> {
    serde_json::from_str(include_str!("../../../web/plugin-catalog.json"))
        .expect("bundled plugin catalog")
}

pub fn handle(state: &AppState, payload: &Value) -> LegacyResult<Value> {
    let (name, args) = legacy::call_envelope(payload)?;
    match name {
        "plugins.requirements" => Ok(requirements(&args)),
        "plugins.list" => Ok(
            json!({"items":catalog(),"shelf":state.runtime.read_document(SHELF)?,"context":context(state,text(&args,"project"))}),
        ),
        "plugins.search" => search(state, &args),
        "plugins.add" => {
            let item = args
                .get("item")
                .ok_or_else(|| LegacyError::invalid("Choose a tool."))?;
            let kind = text(item, "kind");
            if !["mcp", "library", "api"].contains(&kind)
                || text(item, "id").is_empty()
                || item.to_string().len() > 16000
            {
                return Err(LegacyError::invalid("Choose a catalog tool."));
            }
            let url = url::Url::parse(text(item, "url"))
                .map_err(|_| LegacyError::invalid("Tool needs a source URL."))?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(LegacyError::invalid("Tool needs a public HTTPS source."));
            }
            state.runtime.update_document(SHELF, |doc| {
                if !doc["items"].is_object() {
                    doc["items"] = json!({});
                }
                let mut saved = item.clone();
                saved["added_at"] = json!(time::now_iso());
                // A shelf reference is not an installed package or a running MCP server.
                saved["state"] = json!("added");
                doc["items"][text(item, "id")] = saved;
                Ok(())
            })
        }
        "plugins.remove" => state.runtime.update_document(SHELF, |doc| {
            if let Some(items) = doc["items"].as_object_mut() {
                items.remove(text(&args, "id"));
            }
            Ok(())
        }),
        "plugins.install" => install(state, &args),
        "plugins.folder" => {
            let id = text(&args, "id");
            if !["dumbvoice", "dumb-lighting"].contains(&id) {
                return Err(LegacyError::invalid("Unknown app."));
            }
            let doc = state.runtime.read_document(SHELF)?;
            if doc["items"][id]["state"] != "installed" {
                return Err(LegacyError::not_found("Install this app first."));
            }
            let path = doc["items"][id]["path"]
                .as_str()
                .ok_or_else(|| LegacyError::not_found("Package unavailable."))?;
            let package = fs::canonicalize(path)
                .map_err(|_| LegacyError::not_found("Package unavailable."))?;
            let parent = fs::canonicalize(state.runtime.root())
                .map_err(|_| LegacyError::not_found("Package storage unavailable."))?;
            if package.parent() != Some(parent.as_path()) {
                return Err(LegacyError::invalid("Package is outside this app."));
            }
            crate::platform::open_folder(&package)?;
            Ok(json!({"opened":true}))
        }
        _ => Err(LegacyError::not_found("Unknown plugin action.")),
    }
}

fn requirements(args: &Value) -> Value {
    let id = text(args, "id");
    let mut rows = Vec::new();
    let command_found = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|p| p.join(name).is_file())
    };
    if id == "dumb-lighting" {
        let mut found =
            Path::new(text(args, "signalrgb_path")).is_file() || command_found("SignalRgb.exe");
        #[cfg(windows)]
        if !found {
            for root in [
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
                r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall",
                r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            ] {
                let args = vec![
                    "query".into(),
                    root.into(),
                    "/s".into(),
                    "/f".into(),
                    "SignalRGB".into(),
                    "/d".into(),
                ];
                if let Ok(out) = super::process::run(
                    std::ffi::OsStr::new("reg.exe"),
                    &args,
                    &std::env::temp_dir(),
                    Duration::from_secs(6),
                ) {
                    if out.code == 0 {
                        found = true;
                        break;
                    }
                }
            }
        }
        rows.push(json!({"name":"SignalRGB","state":if found{"found"}else{"not_found"},"separate_app":true,"detail":if found{"Windows app registration or command found."}else{"Not found in Windows app registrations or PATH. Install SignalRGB separately, or locate an existing portable install."}}));
    } else if id == "dumbvoice" {
        rows.push(json!({"name":"Windows x64","state":if cfg!(all(windows,target_arch="x86_64")){"found"}else{"not_found"},"detail":"Required for the native package."}));
        rows.push(json!({"name":"Speech engine and English model","state":if Path::new(text(args,"engine_path")).is_file() && Path::new(text(args,"model_path")).is_file(){"found"}else{"choose"},"detail":"Choose your whisper.cpp executable and model in DumbVoice settings. Custom paths are supported; no drive scan or model download runs here."}));
        rows.push(json!({"name":"Python (optional engines only)","state":if command_found("py.exe")||command_found("python.exe"){"found"}else{"optional"},"detail":"The native whisper.cpp path does not require Python. Python engine packages can be installed by the setup thread."}));
    }
    if id == "dumbvoice" {
        let found = if cfg!(windows) {
            super::process::run(
                std::ffi::OsStr::new("reg.exe"),
                &[
                    "query".into(),
                    r"HKLM\SOFTWARE\Microsoft\NET Framework Setup\NDP\v4\Full".into(),
                    "/v".into(),
                    "Release".into(),
                ],
                &std::env::temp_dir(),
                Duration::from_secs(6),
            )
            .is_ok_and(|out| out.code == 0)
        } else {
            false
        };
        rows.push(json!({"name":".NET Framework 4.x","state":if found{"found"}else{"not_found"},"detail":"Windows runtime registration check. Source builds also need the Framework C# compiler; the T3 setup request verifies build tools before installing."}));
    }
    json!({"items":rows,"checked_at":time::now_iso()})
}

fn bounded_file(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let m = fs::symlink_metadata(path).ok()?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

fn context(state: &AppState, id: &str) -> Value {
    let mut signals: Vec<&str> = Vec::new();
    let root = match super::git::project(state, id) {
        Ok(p) => p,
        Err(_) => {
            return json!({"signals":[],"note":"Choose a registered project to find tools for its stack."})
        }
    };
    // Only bounded root manifests; no source tree, account data or environment files.
    if bounded_file(&root.join("Cargo.toml"), 65536).is_some() {
        signals.push("rust");
    }
    if bounded_file(&root.join("pyproject.toml"), 65536).is_some()
        || bounded_file(&root.join("requirements.txt"), 65536).is_some()
    {
        signals.push("python");
    }
    if bounded_file(&root.join("default.project.json"), 65536).is_some() {
        signals.extend(["roblox", "luau"]);
    }
    if bounded_file(&root.join("go.mod"), 65536).is_some() {
        signals.push("golang");
    }
    if let Some(raw) = bounded_file(&root.join("package.json"), 65536) {
        signals.push("javascript");
        if let Ok(p) = serde_json::from_slice::<Value>(&raw) {
            for (dep, tag) in [
                ("react", "react"),
                ("next", "nextjs"),
                ("typescript", "typescript"),
                ("discord.js", "discord"),
                ("@tauri-apps/api", "tauri"),
            ] {
                if p["dependencies"].get(dep).is_some() || p["devDependencies"].get(dep).is_some() {
                    signals.push(tag);
                }
            }
        }
    }
    signals.sort();
    signals.dedup();
    json!({"signals":signals,"note":if signals.is_empty(){"No recognized root manifest yet. Search by the tool or language you need."}else{"Matched from this project’s root manifests. Only stack keywords are used in discovery."}})
}

fn get(url: &str) -> LegacyResult<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("DumbManager-Plugins/0.1")
        .build()
        .map_err(|_| LegacyError::unavailable("Discovery could not start."))?;
    let response = client
        .get(url)
        .send()
        .map_err(|_| LegacyError::unavailable("Source unavailable. Try again shortly."))?;
    if !response.status().is_success() {
        return Err(LegacyError::unavailable(
            "Source unavailable or rate limited. Your saved tools are still here.",
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LegacyError::unavailable("Could not read source."))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(LegacyError::unavailable("Source response is too large."));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| LegacyError::unavailable("Source returned an unreadable response."))
}

fn search(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let query = text(args, "query").trim();
    if query.chars().count() > 120 || query.chars().any(char::is_control) {
        return Err(LegacyError::invalid("Search with up to 120 characters."));
    }
    let ctx = context(state, text(args, "project"));
    let signals: Vec<&str> = ctx["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let term = if query.is_empty() {
        signals.last().copied().unwrap_or("developer")
    } else {
        query
    };
    let kind = text(args, "kind");
    let mut items = Vec::new();
    let mut sources = Vec::new();
    if kind.is_empty() || kind == "all" || kind == "mcp" {
        let mut url =
            url::Url::parse("https://registry.modelcontextprotocol.io/v0.1/servers").unwrap();
        url.query_pairs_mut()
            .append_pair(
                "search",
                if term.chars().count() < 3 {
                    "mcp"
                } else {
                    term
                },
            )
            .append_pair("version", "latest")
            .append_pair("limit", "40");
        match get(url.as_str()) {
            Ok(data) => {
                for entry in data["servers"].as_array().into_iter().flatten().take(40) {
                    let s = &entry["server"];
                    let name = text(s, "name");
                    if name.is_empty() {
                        continue;
                    }
                    if entry["_meta"]["io.modelcontextprotocol.registry/official"]["status"]
                        .as_str()
                        .is_some_and(|s| s != "active")
                    {
                        continue;
                    }
                    items.push(json!({"id":format!("mcp:{name}"),"name":s["title"].as_str().unwrap_or(name),"kind":"mcp","description":s["description"],"url":format!("https://registry.modelcontextprotocol.io/v0.1/servers/{}/versions/latest",url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>()),"source":"MCP Registry","version":s["version"],"tags":[term],"manifest":s,"setup":"Review the publisher’s package or remote endpoint below. Add it to your DumbManager shelf to keep its configuration here. No other MCP client is changed."}));
                }
                sources.push(json!({"name":"MCP Registry","ok":true}));
            }
            Err(e) => sources.push(json!({"name":"MCP Registry","ok":false,"error":e.error})),
        }
    }
    if kind.is_empty() || kind == "all" || kind == "library" {
        let mut url = url::Url::parse("https://api.github.com/search/repositories").unwrap();
        // Treat the search as words, never GitHub query operators supplied by a page.
        let words = term
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .filter(|s| !s.is_empty())
            .take(8)
            .collect::<Vec<_>>()
            .join(" ");
        url.query_pairs_mut()
            .append_pair("q", &format!("{words} library archived:false"))
            .append_pair("sort", "stars")
            .append_pair("per_page", "24");
        match get(url.as_str()) {
            Ok(data) => {
                for r in data["items"].as_array().into_iter().flatten().take(24) {
                    items.push(json!({"id":format!("github:{}",text(r,"full_name")),"name":r["full_name"],"kind":"library","description":r["description"],"url":r["html_url"],"source":"GitHub","stars":r["stargazers_count"],"updated_at":r["pushed_at"],"tags":r["topics"],"setup":"Read the repository’s installation and compatibility notes. Add this reference to your shelf; install the package using your project’s package manager."}));
                }
                sources.push(json!({"name":"GitHub","ok":true}));
            }
            Err(e) => sources.push(json!({"name":"GitHub","ok":false,"error":e.error})),
        }
    }
    Ok(
        json!({"items":items,"sources":sources,"context":ctx,"term":term,"checked_at":time::now_iso(),"note":"Popularity uses GitHub stars where available. MCP Registry does not provide install counts. Results are a bounded source page."}),
    )
}

fn install(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let _lock = INSTALL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.runtime.read_document(SHELF)?;
    let id = text(args, "id");
    let files: Vec<&str> = match id {
        "dumbvoice" => vec!["DumbVoice.exe", "transcribe.py"],
        "dumb-lighting" => vec![
            "Dumb Cinema.html",
            "Dumb Coding.html",
            "Dumb FPS.html",
            "Dumb Horror.html",
            "Dumb Music.html",
            "Dumb Simulator.html",
        ],
        _ => {
            return Err(LegacyError::invalid(
                "This app has no installable package yet.",
            ))
        }
    };
    let source = fs::canonicalize(text(args, "source"))
        .map_err(|_| LegacyError::invalid("Choose the built app or effects folder."))?;
    let mut payload = Vec::new();
    for name in files {
        let bytes = bounded_file(&source.join(name), 32 * 1024 * 1024).ok_or_else(|| {
            LegacyError::invalid(&format!(
                "Package needs an ordinary {name} file (up to 32 MiB)."
            ))
        })?;
        if name.ends_with(".exe") && !bytes.starts_with(b"MZ") {
            return Err(LegacyError::invalid("Choose a built Windows app."));
        }
        payload.push((name, bytes));
    }
    // Unique immutable directories: no overwrites, running-process replacement, or recursive removal.
    let dir = state
        .runtime
        .root()
        .join(format!("plugin-{id}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(state.runtime.root())
        .map_err(|_| LegacyError::unavailable("Could not create plugin storage."))?;
    fs::create_dir(&dir)
        .map_err(|_| LegacyError::unavailable("Could not create package folder."))?;
    for (name, bytes) in payload {
        fs::write(dir.join(name), bytes)
            .map_err(|_| LegacyError::unavailable("Package copy failed. Try again."))?;
    }
    state.runtime.update_document(SHELF, |doc| {
        if !doc["items"].is_object() {
            doc["items"] = json!({});
        }
        let mut item = catalog().into_iter().find(|v| v["id"] == id).unwrap();
        item["state"] = json!("installed");
        item["path"] = json!(dir);
        item["added_at"] = json!(time::now_iso());
        doc["items"][id] = item;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dumbmanager_core::{AppPaths, Store};

    fn fixture() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("io.dumbmanager.rust")).unwrap();
        let store = Store::open(&paths.database_path()).unwrap();
        (dir, AppState::new(paths, store, None))
    }

    #[test]
    fn installs_only_complete_packages_inside_own_storage() {
        let (temp, state) = fixture();
        let source = temp.path().join("custom-package");
        fs::create_dir(&source).unwrap();
        let args = json!({"id":"dumbvoice","source":source});
        fs::write(
            source.join("DumbVoice.exe"),
            b"MZ synthetic, never executed",
        )
        .unwrap();
        assert!(install(&state, &args).is_err());
        fs::write(source.join("transcribe.py"), b"# synthetic").unwrap();
        let saved = install(&state, &args).unwrap();
        let path = Path::new(saved["items"]["dumbvoice"]["path"].as_str().unwrap());
        assert!(path.starts_with(state.runtime.root()));
        assert!(path.join("transcribe.py").is_file());
        assert!(source.join("DumbVoice.exe").is_file());
        assert!(install(&state, &json!({"id":"dumbeffects","source":source})).is_err());
        let removed = handle(
            &state,
            &json!({"name":"plugins.remove","args":{"id":"dumbvoice"}}),
        )
        .unwrap();
        assert!(removed["items"].get("dumbvoice").is_none());
        assert!(path.join("DumbVoice.exe").is_file());
    }

    #[test]
    fn context_uses_only_registered_root_manifests() {
        let (temp, state) = fixture();
        let root = temp.path().join("unusual-project-location");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("Cargo.toml"), b"[package]").unwrap();
        fs::write(
            root.join("package.json"),
            br#"{"dependencies":{"react":"1"}}"#,
        )
        .unwrap();
        state
            .runtime
            .write_document(
                "native-projects.json",
                &json!({"items":[{"id":"demo","path":root}]}),
            )
            .unwrap();
        assert_eq!(
            context(&state, "demo")["signals"],
            json!(["javascript", "react", "rust"])
        );
        assert_eq!(context(&state, "not-registered")["signals"], json!([]));
    }

    #[test]
    fn supports_custom_requirement_locations_without_executing() {
        let (temp, _state) = fixture();
        let engine = temp.path().join("custom-engine.exe");
        let model = temp.path().join("english.bin");
        fs::write(&engine, b"not executed").unwrap();
        fs::write(&model, b"not a real model").unwrap();
        let result =
            requirements(&json!({"id":"dumbvoice","engine_path":engine,"model_path":model}));
        assert_eq!(result["items"][1]["state"], "found");
        assert_eq!(
            requirements(&json!({"id":"dumbvoice"}))["items"][1]["state"],
            "choose"
        );
    }

    #[test]
    fn added_reference_is_not_an_installed_or_running_tool() {
        let (_temp, state) = fixture();
        let item = json!({"id":"mcp:test","name":"Test tool","kind":"mcp","url":"https://example.invalid/docs","state":"installed"});
        let saved = handle(&state, &json!({"name":"plugins.add","args":{"item":item}})).unwrap();
        assert_eq!(saved["items"]["mcp:test"]["state"], "added");
        let bad = json!({"id":"bad","kind":"mcp","url":"file:///secret"});
        assert!(handle(&state, &json!({"name":"plugins.add","args":{"item":bad}})).is_err());
    }
}
