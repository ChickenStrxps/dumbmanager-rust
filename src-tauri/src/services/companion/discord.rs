//! Signed Discord intake. The ledger and drafts contain no interaction tokens.
mod workflows;
use super::*;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

const LEDGER: &str = "bridge-discord-events.json";
static LOCK: Mutex<()> = Mutex::new(());
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
const MAX_ATTACHMENTS: usize = 4;
const MAX_BYTES: u64 = 20 * 1024 * 1024;

pub(super) struct Job {
    id: String,
    app: String,
    token: String,
    attachments: Vec<Value>,
    action: String,
    args: Value,
    nonce: String,
    config_version: u64,
}
impl Drop for Job {
    fn drop(&mut self) {
        ACTIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.nonce);
        WORKERS.fetch_sub(1, Ordering::Release);
        use zeroize::Zeroize;
        self.token.zeroize();
    }
}
fn reserve_job(id: &str, app: &str, token: &str, attachments: Vec<Value>) -> LegacyResult<Job> {
    if token.is_empty()
        || token.len() > 256
        || !token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err(LegacyError::invalid("Invalid interaction continuation."));
    }
    // Keep compatibility with Rust 1.88, before AtomicUsize::try_update.
    let mut workers = WORKERS.load(Ordering::Acquire);
    loop {
        if workers >= 4 {
            return Err(LegacyError::unavailable(
                "Discord attachment workers are busy; try again.",
            ));
        }
        match WORKERS.compare_exchange_weak(
            workers,
            workers + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => workers = current,
        }
    }
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    ACTIVE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(nonce.clone());
    Ok(Job {
        id: id.into(),
        app: app.into(),
        token: token.into(),
        attachments,
        action: "import".into(),
        args: json!({}),
        nonce,
        config_version: 0,
    })
}
fn process_session() -> &'static str {
    static SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SESSION.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}
fn recover_row(row: &mut Value) {
    let active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
    let interrupted = row["pending"].is_object()
        && (row["pending"]["session"] != process_session()
            || !active.contains(val(&row["pending"], "nonce")));
    let intake = row["intake_pending"] == true
        && (row["intake_session"] != process_session()
            || !active.contains(val(row, "intake_nonce")));
    if interrupted {
        row["pending"] = Value::Null;
        row["notice"] = json!("The earlier operation was interrupted. Recover same send preserves its frozen identity; no success is assumed.");
    }
    if intake {
        row["intake_pending"] = json!(false);
        row["intake_failed"] = json!(true);
        row["notice"] = json!("Reopen the original message to finish importing its evidence.");
    }
    if interrupted || intake {
        row["version"] = json!(row["version"].as_u64().unwrap_or(0) + 1);
    }
}

fn attachment_metadata(a: &Value) -> LegacyResult<Value> {
    let id = val(a, "id");
    let size = a["size"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= MAX_BYTES)
        .ok_or_else(|| LegacyError::invalid("Attachment exceeds the 20 MiB intake limit."))?;
    let name = val(a, "filename");
    let mime = val(a, "content_type").split(';').next().unwrap_or("");
    let image = matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    );
    let video = mime.strip_prefix("video/").is_some_and(|v| {
        !v.is_empty()
            && v.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".+-_".contains(&c))
    });
    if !snow(id)
        || name.is_empty()
        || name.len() > 220
        || a["description"].as_str().is_some_and(|d| d.len() > 256)
        || (!image && !video)
        || (image && size > 10 * 1024 * 1024)
    {
        return Err(LegacyError::invalid(
            "Only bounded PNG, JPEG, WebP, GIF or video attachments are supported.",
        ));
    }
    let url = url::Url::parse(val(a, "url"))
        .map_err(|_| LegacyError::invalid("Invalid Discord attachment URL."))?;
    if url.scheme() != "https"
        || url.host_str() != Some("cdn.discordapp.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/attachments/")
        || url.fragment().is_some()
    {
        return Err(LegacyError::invalid(
            "Attachment must come from Discord's CDN.",
        ));
    }
    Ok(
        json!({"id":id,"filename":name,"size":size,"content_type":mime,"description":a["description"],"ephemeral":a["ephemeral"]}),
    )
}

fn imported(state: &AppState, job: &Job, row: &mut Value) -> LegacyResult<()> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| LegacyError::unavailable("Could not initialize attachment download."))?;
    let mut saved = row["attachments"].as_array().cloned().unwrap_or_default();
    for attachment in &job.attachments {
        let metadata = attachment_metadata(attachment)?;
        if saved.iter().any(|old| old["discord"] == metadata) {
            continue;
        }
        let size = metadata["size"].as_u64().unwrap();
        let mut response = client.get(val(attachment, "url")).send().map_err(|_| {
            LegacyError::unavailable(
                "Discord attachment download failed. Reopen the original message to retry.",
            )
        })?;
        if !response.status().is_success() || response.content_length().is_some_and(|n| n != size) {
            return Err(LegacyError::unavailable(
                "Discord attachment was unavailable or changed.",
            ));
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(size + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| LegacyError::unavailable("Discord attachment download failed."))?;
        if bytes.len() as u64 != size {
            return Err(LegacyError::unavailable(
                "Discord attachment was incomplete.",
            ));
        }
        let uploaded=state.legacy_api("/api/itch-upload",&json!({"base64":STANDARD.encode(bytes),"filename":metadata["filename"],"mime_type":metadata["content_type"],"video_mode":"original"}))?;
        saved.push(json!({"discord":metadata,"evidence":uploaded["attachment"]}));
        row["attachments"] = json!(saved);
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut ledger = state.runtime.read_document(LEDGER)?;
        ledger["drafts"][&job.id] = row.clone();
        state.runtime.write_document(LEDGER, &ledger)?;
    }
    row["intake_failed"] = json!(false);
    Ok(())
}
pub(super) fn run_job(state: &AppState, job: Job) {
    let initial = (|| -> LegacyResult<Value> {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let config = state.runtime.read_document(BRIDGE)?;
        if config["enabled"] != true
            || config["application_id"] != job.app
            || config["version"].as_u64().unwrap_or(0) != job.config_version
        {
            return Err(denied());
        }
        let ledger = state.runtime.read_document(LEDGER)?;
        let row = ledger["drafts"][&job.id].clone();
        if !row.is_object() {
            return Err(denied());
        }
        Ok(row)
    })();
    let Ok(mut row) = initial else {
        return;
    };
    let result = if job.action == "import" {
        imported(state, &job, &mut row).map(|_| None)
    } else {
        workflows::perform(state, &job, &mut row)
    };
    let mut file = None;
    match result {
        Ok(attachment) => file = attachment,
        Err(error) => {
            if error.code == "bridge_preview_changed" {
                row["frozen"]["status"] = json!("previewed");
            }
            row["notice"] = json!(format!(
                "{} Saved draft: {}. No success is claimed for this operation.",
                error.error.chars().take(500).collect::<String>(),
                job.id
            ));
            if job.action == "import" {
                row["intake_failed"] = json!(true);
            }
        }
    }
    row["pending"] = Value::Null;
    row["intake_pending"] = json!(false);
    row["version"] = json!(row["version"].as_u64().unwrap_or(0) + 1);
    let saved = (|| -> LegacyResult<()> {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut ledger = state.runtime.read_document(LEDGER)?;
        ledger["drafts"][&job.id] = row.clone();
        state.runtime.write_document(LEDGER, &ledger)
    })();
    if saved.is_err() {
        return;
    }
    let reply = workflows::reply(&row);
    // The token is memory-only and zeroized when Job drops. A lost callback never re-dispatches T3.
    if let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        let request = client.patch(format!(
            "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
            job.app, job.token
        ));
        if let Some(file) = file {
            let mut reply = reply;
            reply["attachments"] = json!([{"id":0,"filename":file.name}]);
            if let Ok(part) = reqwest::blocking::multipart::Part::bytes(file.bytes)
                .file_name(file.name)
                .mime_str(file.mime)
            {
                let _ = request
                    .multipart(
                        reqwest::blocking::multipart::Form::new()
                            .text("payload_json", reply.to_string())
                            .part("files[0]", part),
                    )
                    .send();
            }
        } else {
            let _ = request.json(&reply).send();
        }
    }
}
fn hex(s: &str, len: usize) -> Option<Vec<u8>> {
    if s.len() != len * 2 || !s.is_ascii() {
        return None;
    }
    (0..len)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}
fn denied() -> LegacyError {
    LegacyError {
        error: "Invalid or unauthorized Discord interaction.".into(),
        code: "discord_access".into(),
        status: 401,
    }
}
fn snow(s: &str) -> bool {
    !s.is_empty() && s.len() <= 24 && s.bytes().all(|b| b.is_ascii_digit())
}
fn allowed(config: &Value, name: &str, value: &str) -> bool {
    config[name]
        .as_array()
        .is_some_and(|a| a.is_empty() || a.iter().any(|v| v.as_str() == Some(value)))
}
fn panel(row: &Value, kind: u8) -> Value {
    workflows::panel(row, kind)
}
fn val<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn input_value(data: &Value, key: &str) -> String {
    fn walk(v: &Value, key: &str) -> Option<String> {
        if v["custom_id"].as_str() == Some(key) {
            return v["value"].as_str().map(str::to_owned);
        }
        if let Some(a) = v.as_array() {
            for x in a {
                if let Some(s) = walk(x, key) {
                    return Some(s);
                }
            }
        }
        if let Some(o) = v.as_object() {
            for x in o.values() {
                if let Some(s) = walk(x, key) {
                    return Some(s);
                }
            }
        }
        None
    }
    walk(data, key).unwrap_or_default()
}
pub(super) fn receive(
    state: &AppState,
    signature: &str,
    timestamp: &str,
    raw: &[u8],
) -> LegacyResult<(Value, Option<Job>)> {
    let config = state.runtime.read_document(BRIDGE)?;
    if config["enabled"] != true {
        return Err(denied());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| denied())?
        .as_secs();
    let stamp = timestamp.parse::<u64>().map_err(|_| denied())?;
    if timestamp.len() > 12
        || !timestamp.bytes().all(|b| b.is_ascii_digit())
        || now.abs_diff(stamp) > 300
    {
        return Err(denied());
    }
    let key: [u8; 32] = hex(val(&config, "public_key"), 32)
        .ok_or_else(denied)?
        .try_into()
        .map_err(|_| denied())?;
    let sig: [u8; 64] = hex(signature, 64)
        .ok_or_else(denied)?
        .try_into()
        .map_err(|_| denied())?;
    let mut signed = timestamp.as_bytes().to_vec();
    signed.extend_from_slice(raw);
    VerifyingKey::from_bytes(&key)
        .map_err(|_| denied())?
        .verify_strict(&signed, &Signature::from_bytes(&sig))
        .map_err(|_| denied())?;
    let v: Value = serde_json::from_slice(raw)
        .map_err(|_| LegacyError::invalid("Invalid interaction JSON."))?;
    if val(&v, "application_id") != val(&config, "application_id") {
        return Err(denied());
    }
    // Only a verified request for the configured application counts as a signed check.
    state.runtime.write_document("bridge-discord-verified.json", &json!({"application_id":config["application_id"],"public_key":config["public_key"],"at":dumbmanager_core::time::now_iso()}))?;
    if v["type"] == 1 {
        return Ok((json!({"type":1}), None));
    }
    let actor = v["member"]["user"]["id"]
        .as_str()
        .or_else(|| v["user"]["id"].as_str())
        .unwrap_or("");
    let channel = val(&v, "channel_id");
    let guild = val(&v, "guild_id");
    let event = val(&v, "id");
    if !snow(actor)
        || !snow(channel)
        || !snow(event)
        || !config["users"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(actor)))
        || !allowed(&config, "channels", channel)
        || !allowed(&config, "guilds", guild)
    {
        return Err(denied());
    }
    let digest = format!("{:x}", Sha256::digest(raw));
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ledger = state.runtime.read_document(LEDGER)?;
    if !ledger["events"].is_object() {
        ledger = json!({"events":{},"drafts":{}});
    }
    if let Some(old) = ledger["events"].get(event) {
        if old["digest"] != digest {
            return Err(LegacyError::conflict(
                "Interaction ID reused with different content.",
            ));
        }
        return Ok((old["response"].clone(), None));
    }
    ledger["events"].as_object_mut().unwrap().retain(|_, e| {
        e["at"]
            .as_u64()
            .is_some_and(|at| now.saturating_sub(at) <= 600)
    });
    if ledger["events"]
        .as_object()
        .is_some_and(|o| o.len() >= 2000)
    {
        return Err(LegacyError::unavailable(
            "Discord event ledger is full; retry shortly.",
        ));
    }
    let data = &v["data"];
    if v["type"] == 2 && data["name"] == "dumbbridge" {
        if let Some(id) = data["options"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|o| o["name"] == "draft")
            .and_then(|o| o["value"].as_str())
        {
            let row = &mut ledger["drafts"][id];
            if val(row, "actor") != actor
                || val(row, "channel") != channel
                || row["app"] != config["application_id"]
                || row["source"]["guild_id"] != guild
            {
                return Err(denied());
            }
            recover_row(row);
            let response = panel(row, 4);
            ledger["events"][event] = json!({"digest":digest,"response":response,"at":now});
            state.runtime.write_document(LEDGER, &ledger)?;
            return Ok((response, None));
        }
    }
    let mut job = None;
    let response = if v["type"] == 2 {
        let (text, source, attachments) = if data["name"] == "dumbbridge" && data["type"] == 1 {
            let options = data["options"].as_array().cloned().unwrap_or_default();
            let attachments =
                if let Some(option) = options.iter().find(|x| x["name"] == "attachment") {
                    let key = val(option, "value");
                    let a = data["resolved"]["attachments"][key].clone();
                    if key.is_empty() || val(&a, "id") != key {
                        return Err(LegacyError::invalid(
                            "Discord did not supply the attachment.",
                        ));
                    }
                    vec![a]
                } else {
                    vec![]
                };
            let text = options
                .iter()
                .find(|x| x["name"] == "message")
                .and_then(|x| x["value"].as_str())
                .unwrap_or("");
            (
                text.to_owned(),
                json!({"platform":"discord","actor":actor,"channel_id":channel,"guild_id":guild,"text":text}),
                attachments,
            )
        } else if data["name"] == "DumbBridge" && data["type"] == 3 {
            let message = &data["resolved"]["messages"][val(data, "target_id")];
            let attachments = message["attachments"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let text = val(message, "content");
            let mid = val(message, "id");
            if !snow(mid) {
                return Err(LegacyError::invalid(
                    "Discord did not supply the selected message.",
                ));
            }
            (
                text.to_owned(),
                json!({"platform":"discord","actor":actor,"channel_id":channel,"guild_id":guild,"message_id":mid,"author_id":message["author"]["id"],"author":{"id":message["author"]["id"],"username":message["author"]["username"],"global_name":message["author"]["global_name"],"discriminator":message["author"]["discriminator"],"avatar":message["author"]["avatar"],"bot":message["author"]["bot"]},"timestamp":message["timestamp"],"edited_timestamp":message["edited_timestamp"],"message_type":message["type"],"message_flags":message["flags"],"url":format!("https://discord.com/channels/{}/{channel}/{mid}",if guild.is_empty(){"@me"}else{guild}),"text":text}),
                attachments,
            )
        } else {
            return Err(LegacyError::invalid("Unknown command."));
        };
        if (text.trim().is_empty() && attachments.is_empty()) || text.len() > 6000 {
            return Err(LegacyError::invalid(
                "A message or attachment is required; text is limited to 6000 bytes.",
            ));
        }
        let source_key = format!(
            "{:x}",
            Sha256::digest(
                json!([
                    config["application_id"],
                    actor,
                    channel,
                    guild,
                    [
                        source["message_id"].clone(),
                        source["author_id"].clone(),
                        json!(text)
                    ],
                    attachments
                        .iter()
                        .map(|a| a["id"].clone())
                        .collect::<Vec<_>>()
                ])
                .to_string()
                .as_bytes()
            )
        );
        let previous = ledger["drafts"]
            .as_object()
            .into_iter()
            .flat_map(|o| o.values())
            .find(|row| row["source_key"] == source_key)
            .cloned();
        if let Some(previous) = previous.as_ref().filter(|r| {
            (r["intake_pending"] != true && r["intake_failed"] != true)
                || (r["intake_pending"] == true
                    && ACTIVE
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains(val(r, "intake_nonce")))
        }) {
            let response = panel(previous, 4);
            ledger["events"][event] = json!({"digest":digest,"response":response,"at":now});
            state.runtime.write_document(LEDGER, &ledger)?;
            return Ok((response, None));
        }
        if previous.is_none() && ledger["drafts"].as_object().is_some_and(|o| o.len() >= 500) {
            return Err(LegacyError::unavailable("Discord draft storage is full."));
        }
        let id = previous
            .as_ref()
            .and_then(|r| r["id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let title = if text.trim().is_empty() {
            "Discord attachment".to_owned()
        } else {
            text.lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>()
        };
        let row = json!({"id":id,"version":1,"actor":actor,"channel":channel,"source":source,"title":title,"observation":text.chars().take(1500).collect::<String>(),"goal":"","why":"","questions":"","itch_id":"","project":"unsorted","thread_id":"","target":"","app":v["application_id"]});
        if attachments.len() > MAX_ATTACHMENTS
            || attachments
                .iter()
                .map(|a| a["size"].as_u64().unwrap_or(u64::MAX))
                .try_fold(0u64, |n, size| n.checked_add(size))
                .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(LegacyError::invalid(
                "Use at most four attachments totaling 20 MiB.",
            ));
        }
        let metadata: Vec<Value> = attachments
            .iter()
            .map(attachment_metadata)
            .collect::<LegacyResult<_>>()?;
        let mut row = previous.unwrap_or(row);
        row["source_key"] = json!(source_key);
        row["source"]["attachments"] = json!(metadata);
        if row["source"].to_string().len() > 9000 {
            return Err(LegacyError::invalid(
                "Discord source metadata is too large.",
            ));
        }
        let response = if attachments.is_empty() {
            panel(&row, 4)
        } else {
            job = Some(reserve_job(
                &id,
                val(&v, "application_id"),
                val(&v, "token"),
                attachments,
            )?);
            let task = job.as_mut().unwrap();
            task.config_version = config["version"].as_u64().unwrap_or(0);
            row["intake_pending"] = json!(true);
            row["intake_session"] = json!(process_session());
            row["intake_nonce"] = json!(task.nonce);
            json!({"type":5,"data":{"flags":64}})
        };
        ledger["drafts"][&id] = row;
        response
    } else if v["type"] == 3 || v["type"] == 5 {
        workflows::receive(state, &v, &config, &mut ledger, actor, channel, &mut job)?
    } else {
        return Err(LegacyError::invalid("Unsupported interaction."));
    };
    ledger["events"][event] = json!({"digest":digest,"response":response,"at":now});
    state.runtime.write_document(LEDGER, &ledger)?;
    Ok((response, job))
}
