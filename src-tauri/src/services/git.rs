//! Explicit project-scoped Git operations with stale-view and durable-retry guards.
use super::process;
use crate::state::AppState;
use dumbmanager_core::{
    legacy::{self, LegacyError, LegacyResult},
    time,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, UNIX_EPOCH},
};

static WRITER: Mutex<()> = Mutex::new(());

pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    if path != "/api/call" {
        return None;
    }
    let name = payload.get("name")?.as_str()?;
    if !name.starts_with("git.") {
        return None;
    }
    Some((|| {
        let (name, args) = legacy::call_envelope(payload)?;
        if name == "git.settings" {
            let saved = state.runtime.read_document("native-git.json")?;
            return Ok(json!({"clone_root":saved["clone_root"].as_str().unwrap_or("")}));
        }
        if name == "git.projects" {
            let mut items = projects(state)?;
            for item in &mut items {
                item["repository"] = json!(item["path"]
                    .as_str()
                    .is_some_and(|p| Path::new(p).join(".git").exists()));
            }
            return Ok(json!({"items":items,"time":time::now_iso()}));
        }
        if name == "git.configure" {
            let root = text(&args, "clone_root", 2000)?;
            let root = fs::canonicalize(root).map_err(|_| {
                LegacyError::invalid("Choose an existing clone destination folder.")
            })?;
            if !root.is_dir() {
                return Err(LegacyError::invalid("Choose a folder."));
            }
            state.runtime.update_document("native-git.json", |doc| {
                doc["clone_root"] = json!(root);
                Ok(())
            })?;
            return Ok(json!({"clone_root":root}));
        }
        if name == "git.clone" {
            return clone_project(state, &args);
        }
        let id = text(&args, "project", 100)?;
        let root = project(state, id)?;
        match name {
            "git.status" => status(
                &root,
                id,
                number(&args, "limit", 50, 1, 100)?,
                number(&args, "offset", 0, 0, 100000)?,
            ),
            "git.diff" => diff(&root, &args),
            "git.history" => history(&root, &args),
            "git.action" => action(state, &root, &args),
            _ => Err(LegacyError::not_found("Unknown Git operation.")),
        }
    })())
}

fn text<'a>(args: &'a Value, key: &str, max: usize) -> LegacyResult<&'a str> {
    let s = match args.get(key) {
        None => "",
        Some(Value::String(s)) => s,
        _ => return Err(LegacyError::invalid("Expected text arguments.")),
    };
    if s.len() > max || s.contains('\0') {
        return Err(LegacyError::invalid("Invalid argument length or content."));
    }
    Ok(s)
}
fn number(args: &Value, key: &str, default: usize, min: usize, max: usize) -> LegacyResult<usize> {
    let n = match args.get(key) {
        None => default as u64,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| LegacyError::invalid("Expected a whole number."))?,
    };
    if n < min as u64 || n > max as u64 {
        return Err(LegacyError::invalid(
            "Number is outside the supported range.",
        ));
    }
    Ok(n as usize)
}
pub fn projects(state: &AppState) -> LegacyResult<Vec<Value>> {
    let doc = state.runtime.read_document("native-projects.json")?;
    Ok(doc["items"].as_array().cloned().unwrap_or_default())
}
pub fn project(state: &AppState, id: &str) -> LegacyResult<PathBuf> {
    let items = projects(state)?;
    let path = items
        .iter()
        .find(|i| i["id"] == id)
        .and_then(|i| i["path"].as_str())
        .ok_or_else(|| LegacyError::not_found("Choose a registered project folder in Settings."))?;
    let root = fs::canonicalize(path)
        .map_err(|_| LegacyError::not_found("The project folder is unavailable."))?;
    if !root.is_dir() {
        return Err(LegacyError::invalid("The project is not a folder."));
    }
    Ok(root)
}
fn command(root: &Path, args: &[String], okay: &[i32]) -> LegacyResult<Vec<u8>> {
    let mut fixed = vec![
        "--no-pager".into(),
        "-c".into(),
        "color.ui=false".into(),
        "-c".into(),
        "core.quotepath=false".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        "protocol.ext.allow=never".into(),
        "-c".into(),
        "maintenance.auto=false".into(),
    ];
    if args
        .first()
        .is_some_and(|s| ["add", "reset", "rm", "diff", "show", "ls-files"].contains(&s.as_str()))
    {
        fixed.push("--literal-pathspecs".into());
    }
    fixed.extend_from_slice(args);
    let result = process::run(OsStr::new("git"), &fixed, root, Duration::from_secs(45))?;
    if !okay.contains(&result.code) {
        return Err(LegacyError{error:format!("Git could not complete this action (exit {}). Refresh status and check Git's configuration.",result.code),code:"git_failed".into(),status:409});
    }
    Ok(result.stdout)
}
fn run(root: &Path, args: &[&str]) -> LegacyResult<Vec<u8>> {
    command(
        root,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &[0],
    )
}
fn string(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim().to_string()
}
fn repository(root: &Path) -> LegacyResult<bool> {
    if !root.join(".git").exists() {
        return Ok(false);
    }
    let top = string(&run(root, &["rev-parse", "--show-toplevel"])?);
    if fs::canonicalize(top)
        .map_err(|_| LegacyError::invalid("Git's working root is unavailable."))?
        != fs::canonicalize(root)
            .map_err(|_| LegacyError::invalid("The project root is unavailable."))?
    {
        return Err(LegacyError::invalid(
            "Choose the repository root, not a subfolder.",
        ));
    }
    Ok(true)
}
fn sensitive(path: &str) -> bool {
    path.replace('\\', "/").split('/').any(|part| {
        let p = part.to_ascii_lowercase();
        p == ".git"
            || p == ".codex"
            || p == ".claude"
            || p == ".t3"
            || p.starts_with(".env")
            || p.contains("credential")
            || p.contains("secret")
            || p.contains("token")
            || p.ends_with(".key")
            || p.ends_with(".pem")
            || p.ends_with(".pfx")
            || p == "id_rsa"
            || p == "id_ed25519"
            || p == "auth.json"
    })
}
fn file(root: &Path, path: &str) -> LegacyResult<PathBuf> {
    if path.is_empty() || path.len() > 2000 || path.contains(['\0', ':', '\\']) || sensitive(path) {
        return Err(LegacyError::invalid(
            "Choose a non-credential file inside this project.",
        ));
    }
    let rel = Path::new(path);
    if rel.components().any(|p| !matches!(p, Component::Normal(_))) {
        return Err(LegacyError::invalid("Choose an individual relative file."));
    }
    let mut target = root.to_path_buf();
    for part in rel.components() {
        target.push(part);
        if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(LegacyError::invalid(
                "Linked project files are not accepted.",
            ));
        }
    }
    Ok(target)
}
fn snapshot(root: &Path) -> LegacyResult<Value> {
    if !repository(root)? {
        return Ok(
            json!({"repository":false,"snapshot":format!("{:x}",Sha256::digest(root.as_os_str().to_string_lossy().as_bytes())),"files":[],"branch":null,"head":null,"upstream":null,"ahead":0,"behind":0,"stashes":[]}),
        );
    }
    let raw = run(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    let decoded = String::from_utf8_lossy(&raw);
    let parts = decoded.split('\0').collect::<Vec<_>>();
    let mut hash = Sha256::new();
    hash.update(&raw);
    let mut info = serde_json::Map::new();
    let mut files = vec![];
    let mut i = 0;
    while i < parts.len() {
        let entry = parts[i];
        i += 1;
        if let Some(header) = entry.strip_prefix("# ") {
            if let Some((k, v)) = header.split_once(' ') {
                info.insert(k.into(), json!(v));
            }
            continue;
        }
        let (xy, path, old) = if entry.starts_with("1 ") {
            let f = entry.splitn(9, ' ').collect::<Vec<_>>();
            if f.len() != 9 {
                continue;
            }
            (f[1], f[8], None)
        } else if entry.starts_with("2 ") {
            let f = entry.splitn(10, ' ').collect::<Vec<_>>();
            if f.len() != 10 || i >= parts.len() {
                continue;
            }
            let old = parts[i];
            i += 1;
            (f[1], f[9], Some(old))
        } else if entry.starts_with("u ") {
            let f = entry.splitn(11, ' ').collect::<Vec<_>>();
            if f.len() != 11 {
                continue;
            }
            (f[1], f[10], None)
        } else if let Some(p) = entry.strip_prefix("? ") {
            ("??", p, None)
        } else {
            continue;
        };
        if files.len() >= 10000 {
            return Err(LegacyError::invalid(
                "More than 10,000 changed files; narrow the project first.",
            ));
        }
        let x = xy.as_bytes();
        let conflict = ["DD", "AU", "UD", "UA", "DU", "AA", "UU"].contains(&xy);
        files.push(json!({"path":path,"original_path":old,"status":xy,"staged":!matches!(x.first(),Some(b'.'|b'?')),"unstaged":x.get(1)!=Some(&b'.'),"untracked":xy=="??","conflict":conflict,"sensitive":sensitive(path)||old.is_some_and(sensitive)}));
        for rel in std::iter::once(path).chain(old) {
            hash.update(rel.as_bytes());
            if let Ok(meta) = fs::symlink_metadata(root.join(rel)) {
                hash.update(meta.len().to_le_bytes());
                if let Ok(at) = meta
                    .modified()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).map_err(std::io::Error::other))
                {
                    hash.update(at.as_nanos().to_le_bytes());
                }
            } else {
                hash.update(b"deleted");
            }
        }
    }
    let index = string(&run(root, &["rev-parse", "--git-path", "index"])?);
    let index = Path::new(&index);
    let index = if index.is_absolute() {
        index.to_path_buf()
    } else {
        root.join(index)
    };
    if let Ok(mut input) = fs::File::open(index) {
        let mut block = [0; 65536];
        loop {
            let n = input
                .read(&mut block)
                .map_err(|_| LegacyError::unavailable("Could not read Git's index."))?;
            if n == 0 {
                break;
            }
            hash.update(&block[..n]);
        }
    }
    let raw_stashes = run(root, &["stash", "list", "--format=%H%x00%gs%x00"])?;
    hash.update(&raw_stashes);
    let stash_text = String::from_utf8_lossy(&raw_stashes);
    let fields = stash_text.split('\0').collect::<Vec<_>>();
    let stashes = fields
        .chunks(2)
        .filter(|f| f.len() == 2 && !f[0].trim().is_empty())
        .enumerate()
        .map(|(i, f)| json!({"id":f[0].trim(),"message":f[1],"index":i}))
        .collect::<Vec<_>>();
    hash.update(run(
        root,
        &["for-each-ref", "--format=%(refname)%00%(objectname)"],
    )?);
    hash.update(command(
        root,
        &[
            "config".into(),
            "--null".into(),
            "--get-regexp".into(),
            "^remote\\.".into(),
        ],
        &[0, 1],
    )?);
    let tracking = info
        .get("branch.ab")
        .and_then(Value::as_str)
        .unwrap_or("+0 -0")
        .split_whitespace()
        .collect::<Vec<_>>();
    let count = |i: usize| {
        tracking
            .get(i)
            .and_then(|x| x.get(1..))
            .and_then(|x| x.parse::<usize>().ok())
            .unwrap_or(0)
    };
    Ok(
        json!({"repository":true,"branch":info.get("branch.head"),"head":info.get("branch.oid"),"upstream":info.get("branch.upstream"),"ahead":count(0),"behind":count(1),"files":files,"stashes":stashes,"snapshot":format!("{:x}",hash.finalize())}),
    )
}
fn status(root: &Path, id: &str, limit: usize, offset: usize) -> LegacyResult<Value> {
    let mut value = snapshot(root)?;
    let files = value["files"].as_array().cloned().unwrap_or_default();
    value["project"] = json!(id);
    value["total"] = json!(files.len());
    value["offset"] = json!(offset);
    value["next_offset"] = json!(if offset + limit < files.len() {
        Some(offset + limit)
    } else {
        None
    });
    value["staged_count"] = json!(files.iter().filter(|v| v["staged"] == true).count());
    value["changed_count"] = json!(files.iter().filter(|v| v["unstaged"] == true).count());
    value["conflict_count"] = json!(files.iter().filter(|v| v["conflict"] == true).count());
    value["files"] = json!(files
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>());
    value["time"] = json!(time::now_iso());
    if value["repository"] != true {
        return Ok(value);
    }
    let raw = run(
        root,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(objectname)%00%(upstream:short)",
            "refs/heads/",
        ],
    )?;
    let decoded = String::from_utf8_lossy(&raw);
    let branches = decoded
        .lines()
        .filter_map(|line| {
            let f = line.split('\0').collect::<Vec<_>>();
            (f.len() == 3).then(|| json!({"name":f[0],"head":f[1],"upstream":f[2]}))
        })
        .collect::<Vec<_>>();
    value["branches_total"] = json!(branches.len());
    value["branches"] = json!(branches.into_iter().take(300).collect::<Vec<_>>());
    let names = string(&run(root, &["remote"])?);
    let mut remotes = vec![];
    for name in names.lines().take(30) {
        remotes.push(json!({"name":name,"fetch":redact_url(&string(&run(root,&["remote","get-url","--",name])?)),"push":redact_url(&string(&run(root,&["remote","get-url","--push","--",name])?))}));
    }
    value["remotes"] = json!(remotes);
    Ok(value)
}
fn redact_url(value: &str) -> String {
    if let Ok(mut url) = url::Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        return url.to_string();
    }
    value.into()
}
fn revision(value: &str) -> LegacyResult<&str> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(LegacyError::invalid("Choose a full commit or stash ID."));
    }
    Ok(value)
}
fn diff(root: &Path, args: &Value) -> LegacyResult<Value> {
    let path = text(args, "path", 2000)?;
    let target = file(root, path)?;
    let staged = args.get("staged").and_then(Value::as_bool).unwrap_or(false);
    let rev = text(args, "revision", 64)?;
    let mut cmd = if rev.is_empty() {
        vec!["diff", "--no-ext-diff", "--no-textconv"]
    } else {
        vec![
            "show",
            "--format=",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            revision(rev)?,
        ]
    };
    if staged && rev.is_empty() {
        cmd.push("--cached");
    }
    cmd.extend(["--", path]);
    let mut raw = run(root, &cmd)?;
    if raw.is_empty() && !staged && rev.is_empty() {
        let tracked = command(
            root,
            &[
                "ls-files".into(),
                "--error-unmatch".into(),
                "--".into(),
                path.into(),
            ],
            &[0, 1],
        )?;
        if tracked.is_empty() && target.is_file() {
            fs::File::open(target)
                .and_then(|f| f.take(160001).read_to_end(&mut raw))
                .map_err(|_| LegacyError::not_found("The file could not be read."))?;
        }
    }
    let binary = raw.iter().take(8192).any(|c| *c == 0);
    let truncated = raw.len() > 160000;
    let content = if binary {
        "Binary file; no text preview.".into()
    } else {
        String::from_utf8_lossy(&raw[..raw.len().min(160000)]).to_string()
    };
    Ok(json!({"path":path,"staged":staged,"revision":rev,"text":content,"truncated":truncated}))
}
fn history(root: &Path, args: &Value) -> LegacyResult<Value> {
    if !repository(root)? {
        return Ok(json!({"items":[],"next_offset":null}));
    }
    let rev = text(args, "revision", 64)?;
    if !rev.is_empty() {
        let raw = run(
            root,
            &[
                "show",
                "--format=",
                "--name-only",
                "--no-renames",
                "-z",
                revision(rev)?,
                "--",
            ],
        )?;
        return Ok(
            json!({"revision":rev,"files":String::from_utf8_lossy(&raw).split('\0').filter(|p|!p.is_empty()).map(|p|json!({"path":p,"sensitive":sensitive(p)})).collect::<Vec<_>>()}),
        );
    }
    if command(
        root,
        &["rev-parse".into(), "--verify".into(), "HEAD".into()],
        &[0, 128],
    )?
    .is_empty()
    {
        return Ok(json!({"items":[],"next_offset":null}));
    }
    let limit = number(args, "limit", 15, 1, 50)?;
    let offset = number(args, "offset", 0, 0, 100000)?;
    let raw = command(
        root,
        &[
            "log".into(),
            "--no-show-signature".into(),
            format!("--max-count={}", limit + 1),
            format!("--skip={offset}"),
            "--format=%H%x00%an%x00%aI%x00%s%x00".into(),
            "--".into(),
        ],
        &[0],
    )?;
    let decoded = String::from_utf8_lossy(&raw);
    let fields = decoded.split('\0').collect::<Vec<_>>();
    let rows = fields
        .chunks(4)
        .filter(|f| f.len() == 4)
        .map(|f| json!({"id":f[0].trim(),"author":f[1],"at":f[2],"subject":f[3]}))
        .collect::<Vec<_>>();
    let next = if rows.len() > limit {
        Some(offset + limit)
    } else {
        None
    };
    Ok(
        json!({"items":rows.into_iter().take(limit).collect::<Vec<_>>(),"offset":offset,"next_offset":next}),
    )
}
fn branch<'a>(root: &Path, value: &'a str) -> LegacyResult<&'a str> {
    if value.is_empty()
        || value.starts_with('-')
        || ["HEAD", "@"].contains(&value)
        || value.contains("@{")
        || value.contains(['\r', '\n'])
    {
        return Err(LegacyError::invalid("Use a valid branch name."));
    }
    run(root, &["check-ref-format", "--branch", value])?;
    Ok(value)
}
fn remote(value: &str) -> LegacyResult<&str> {
    if value.is_empty()
        || value.len() > 80
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(LegacyError::invalid("Use a simple remote name."));
    }
    Ok(value)
}
fn address(value: &str) -> LegacyResult<&str> {
    if value.is_empty()
        || value.starts_with('-')
        || value.contains(['\r', '\n', '\0'])
        || value.contains("::")
    {
        return Err(LegacyError::invalid(
            "Use an HTTPS, SSH or local repository address.",
        ));
    }
    if let Ok(url) = url::Url::parse(value) {
        if url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
        {
            return Ok(value);
        }
        if url.scheme() == "ssh"
            && url.host_str().is_some()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
        {
            return Ok(value);
        }
    }
    if Path::new(value).is_absolute() && Path::new(value).is_dir() {
        return Ok(value);
    }
    if let Some((host, path)) = value.split_once(':') {
        if host.contains('@')
            && !host.contains(['/', '\\', ' '])
            && !path.is_empty()
            && !path.contains(' ')
        {
            return Ok(value);
        }
    }
    Err(LegacyError::invalid(
        "Use a credential-free HTTPS, SSH or existing absolute local address.",
    ))
}
fn receipt<F>(state: &AppState, args: &Value, work: F) -> LegacyResult<Value>
where
    F: FnOnce() -> LegacyResult<Value>,
{
    let key = text(args, "request_key", 128)?;
    if key.len() < 8
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(LegacyError::invalid(
            "Use a stable request key for this Git action.",
        ));
    }
    let fingerprint = format!("{:x}", Sha256::digest(args.to_string()));
    let mut replay = None;
    state.runtime.update_document("git-receipts.json",|doc| {
        if let Some(old)=doc.get(key) {
            if old["fingerprint"]!=fingerprint{return Err(LegacyError::conflict("This retry key belongs to another action."));}
            if old["state"]=="done" {let mut result=old["result"].clone();result["replayed"]=json!(true);replay=Some(result);return Ok(());}
            return Err(LegacyError::conflict("The previous action is pending or failed. Refresh status before starting a new action."));
        }
        if doc.as_object().map_or(0,|o|o.len())>=2000{return Err(LegacyError::invalid("Git receipt storage is full."));}
        doc[key]=json!({"fingerprint":fingerprint,"state":"pending","at":time::now_iso()});Ok(())
    })?;
    if let Some(result) = replay {
        return Ok(result);
    }
    let result = work();
    state.runtime.update_document("git-receipts.json", |doc| {
        match &result {
            Ok(value) => {
                doc[key]["state"] = json!("done");
                doc[key]["result"] = value.clone();
            }
            Err(error) => {
                doc[key]["state"] = json!("failed_or_uncertain");
                doc[key]["code"] = json!(error.code);
            }
        }
        Ok(())
    })?;
    result
}
fn action(state: &AppState, root: &Path, args: &Value) -> LegacyResult<Value> {
    let _guard = WRITER.lock().unwrap_or_else(PoisonError::into_inner);
    receipt(state, args, || {
        let current = snapshot(root)?;
        if current["snapshot"] != text(args, "snapshot", 128)? {
            return Err(LegacyError::conflict(
                "This repository changed. Refresh and review it before acting.",
            ));
        }
        let operation = text(args, "operation", 40)?;
        let branch_name = text(args, "branch", 300)?;
        let remote_name = text(args, "remote", 80)?;
        let message = text(args, "message", 4000)?;
        let files = current["files"].as_array().cloned().unwrap_or_default();
        let mut cmd: Vec<String> = vec![];
        if operation == "init" {
            if current["repository"] == true {
                return Err(LegacyError::invalid("This project already has Git."));
            }
            let b = if branch_name.is_empty() {
                "main"
            } else {
                branch_name
            };
            branch(root, b)?;
            cmd.extend(["init".into(), format!("--initial-branch={b}")]);
        } else {
            if current["repository"] != true {
                return Err(LegacyError::invalid("Initialize this project first."));
            }
            match operation {
                "stage" | "unstage" => {
                    let selected: Value = if args["paths"].is_string() {
                        serde_json::from_str(args["paths"].as_str().unwrap_or("")).map_err(
                            |_| LegacyError::invalid("Select a JSON list of file paths."),
                        )?
                    } else {
                        args["paths"].clone()
                    };
                    let selected = selected
                        .as_array()
                        .filter(|s| !s.is_empty() && s.len() <= 200)
                        .ok_or_else(|| LegacyError::invalid("Select 1–200 changed files."))?;
                    let mut paths = vec![];
                    for value in selected {
                        let path = value
                            .as_str()
                            .ok_or_else(|| LegacyError::invalid("Expected file paths."))?;
                        file(root, path)?;
                        let item = files.iter().find(|f| f["path"] == path).ok_or_else(|| {
                            LegacyError::conflict("A selected file changed. Refresh first.")
                        })?;
                        paths.push(path.to_string());
                        if let Some(old) = item["original_path"].as_str() {
                            file(root, old)?;
                            paths.push(old.into());
                        }
                    }
                    if operation == "stage" {
                        cmd.extend(["add", "--all", "--"].map(String::from));
                    } else if current["head"] == "(initial)" {
                        cmd.extend(
                            ["rm", "--cached", "--force", "--ignore-unmatch", "--"]
                                .map(String::from),
                        );
                    } else {
                        cmd.extend(["reset", "--quiet", "HEAD", "--"].map(String::from));
                    }
                    cmd.extend(paths);
                }
                "commit" => {
                    if message.trim().is_empty() || !files.iter().any(|f| f["staged"] == true) {
                        return Err(LegacyError::invalid(
                            "Stage changes and add a commit message first.",
                        ));
                    }
                    if files.iter().any(|f| {
                        f["conflict"] == true || f["staged"] == true && f["sensitive"] == true
                    }) {
                        return Err(LegacyError::invalid("Resolve conflicts and unstage credential/session files before committing."));
                    }
                    cmd.extend(["commit", "-m", message.trim()].map(String::from));
                }
                "branch_create" | "branch_switch" | "branch_delete" | "merge_ff" => {
                    branch(root, branch_name)?;
                    if ["branch_switch", "merge_ff"].contains(&operation) && !files.is_empty() {
                        return Err(LegacyError::invalid(
                            "Commit or stash current changes first.",
                        ));
                    }
                    cmd = match operation {
                        "branch_create" => vec!["switch", "-c", branch_name],
                        "branch_switch" => vec!["switch", "--", branch_name],
                        "branch_delete" => vec!["branch", "-d", "--", branch_name],
                        _ => vec!["merge", "--ff-only", "--", branch_name],
                    }
                    .into_iter()
                    .map(String::from)
                    .collect();
                }
                "fetch" | "pull" | "push" => {
                    remote(remote_name)?;
                    let mut query = vec!["remote", "get-url", "--all"];
                    if operation == "push" {
                        query.push("--push");
                    }
                    query.extend(["--", remote_name]);
                    let destinations = string(&run(root, &query)?);
                    if destinations.lines().count() != 1 {
                        return Err(LegacyError::invalid("Use a remote with one destination."));
                    }
                    address(&destinations)?;
                    if operation == "fetch" {
                        cmd.extend(
                            ["fetch", "--no-tags", "--no-recurse-submodules", remote_name]
                                .map(String::from),
                        );
                    } else {
                        branch(root, branch_name)?;
                        if current["branch"] != branch_name {
                            return Err(LegacyError::conflict(
                                "Choose the currently checked-out branch.",
                            ));
                        }
                        if operation == "pull" {
                            if !files.is_empty() {
                                return Err(LegacyError::invalid(
                                    "Commit or stash current changes before pulling.",
                                ));
                            }
                            cmd.extend(
                                [
                                    "pull",
                                    "--ff-only",
                                    "--no-rebase",
                                    "--no-tags",
                                    "--no-recurse-submodules",
                                    remote_name,
                                    branch_name,
                                ]
                                .map(String::from),
                            );
                        } else {
                            cmd.extend(
                                [
                                    "push",
                                    "--no-follow-tags",
                                    "--recurse-submodules=no",
                                    "--set-upstream",
                                    remote_name,
                                ]
                                .map(String::from),
                            );
                            cmd.push(format!("HEAD:refs/heads/{branch_name}"));
                        }
                    }
                }
                "remote_add" => {
                    cmd.extend(
                        [
                            "remote",
                            "add",
                            "--",
                            remote(remote_name)?,
                            address(text(args, "url", 2000)?)?,
                        ]
                        .map(String::from),
                    );
                }
                "remote_remove" => {
                    cmd.extend(["remote", "remove", "--", remote(remote_name)?].map(String::from));
                }
                "stash_save" => {
                    if files.is_empty() || files.iter().any(|f| f["sensitive"] == true) {
                        return Err(LegacyError::invalid("No safe changed files to stash. Handle credential/session files outside this view."));
                    }
                    cmd.extend(
                        [
                            "stash",
                            "push",
                            "--include-untracked",
                            "-m",
                            if message.trim().is_empty() {
                                "DumbManager Rust stash"
                            } else {
                                message.trim()
                            },
                        ]
                        .map(String::from),
                    );
                }
                "stash_apply" | "stash_drop" => {
                    let oid = revision(text(args, "stash_id", 64)?)?;
                    let stash = current["stashes"]
                        .as_array()
                        .and_then(|s| s.iter().find(|s| s["id"] == oid))
                        .ok_or_else(|| {
                            LegacyError::conflict("This stash changed. Refresh first.")
                        })?;
                    if operation == "stash_apply" {
                        if !files.is_empty() {
                            return Err(LegacyError::invalid(
                                "Commit or stash current changes first.",
                            ));
                        }
                        cmd.extend(["stash", "apply", "--index", oid].map(String::from));
                    } else {
                        cmd.extend([
                            "stash".into(),
                            "drop".into(),
                            format!("stash@{{{}}}", stash["index"].as_u64().unwrap_or(0)),
                        ]);
                    }
                }
                _ => return Err(LegacyError::invalid("Unknown or unsupported Git action.")),
            }
        }
        let output = command(root, &cmd, &[0])?;
        Ok(
            json!({"operation":operation,"project":args["project"],"request_key":args["request_key"],"output":String::from_utf8_lossy(&output),"note":"","time":time::now_iso()}),
        )
    })
}
fn clone_project(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let _guard = WRITER.lock().unwrap_or_else(PoisonError::into_inner);
    let name = text(args, "name", 80)?;
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err(LegacyError::invalid("Choose a simple new folder name."));
    }
    let url = address(text(args, "url", 2000)?)?;
    let config = state.runtime.read_document("native-git.json")?;
    let parent = config["clone_root"].as_str().ok_or_else(|| {
        LegacyError::invalid("Choose the clone destination folder in Settings first.")
    })?;
    let parent = fs::canonicalize(parent)
        .map_err(|_| LegacyError::invalid("The clone destination is unavailable."))?;
    receipt(state, args, || {
        let destination = parent.join(name);
        if destination.exists() {
            return Err(LegacyError::conflict(
                "That destination already exists. Nothing was replaced.",
            ));
        }
        let output = command(
            &parent,
            &[
                "clone".into(),
                "--no-recurse-submodules".into(),
                "--".into(),
                url.into(),
                destination.to_string_lossy().into(),
            ],
            &[0],
        )?;
        let id = name.to_ascii_lowercase();
        state.runtime.update_document("native-projects.json",|doc| {if !doc["items"].is_array(){doc["items"]=json!([]);}let items=doc["items"].as_array_mut().unwrap();if items.iter().any(|p|p["id"]==id){return Err(LegacyError::conflict("The clone succeeded but its project ID is already registered. Add the folder manually."));}items.push(json!({"id":id,"name":name,"path":destination}));Ok(())})?;
        Ok(
            json!({"operation":"clone","project":id,"name":name,"output":String::from_utf8_lossy(&output),"time":time::now_iso()}),
        )
    })
}
