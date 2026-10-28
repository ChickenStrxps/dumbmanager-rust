//! Signed Discord intake. The ledger and drafts contain no interaction tokens.
use super::*;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const LEDGER: &str = "bridge-discord-events.json";
static LOCK: Mutex<()> = Mutex::new(());
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
    let id = row["id"].as_str().unwrap_or("");
    let version = row["version"].as_u64().unwrap_or(0);
    let title = row["title"].as_str().unwrap_or("");
    let observation = row["observation"].as_str().unwrap_or("");
    let content = format!("**Draft**\n{}\n\n{}\n\nSource retained locally. ASCII card unavailable; this text is the draft. Edit or explicitly stash it in DumbManager.", title, observation);
    let mut data = json!({"content":content.chars().take(1900).collect::<String>(),"allowed_mentions":{"parse":[]},"components":[{"type":1,"components":[{"type":2,"style":2,"label":"Edit","custom_id":format!("db:edit:{id}:{version}")},{"type":2,"style":1,"label":"Stash","custom_id":format!("db:save:{id}:{version}")}]}]});
    if kind == 4 {
        data["flags"] = json!(64);
    }
    json!({"type":kind,"data":data})
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
) -> LegacyResult<Value> {
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
    if v["type"] == 1 {
        return Ok(json!({"type":1}));
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
        return Ok(old["response"].clone());
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
    let response = if v["type"] == 2 {
        let (text, source) = if data["name"] == "dumbbridge" && data["type"] == 1 {
            let options = data["options"].as_array().cloned().unwrap_or_default();
            if options.iter().any(|x| x["name"] == "attachment") {
                return Err(LegacyError::unavailable(
                    "Discord attachments are not yet supported; use a text-only draft.",
                ));
            }
            let text = options
                .iter()
                .find(|x| x["name"] == "message")
                .and_then(|x| x["value"].as_str())
                .unwrap_or("");
            (
                text.to_owned(),
                json!({"platform":"discord","actor":actor,"channel_id":channel,"guild_id":guild,"text":text}),
            )
        } else if data["name"] == "DumbBridge" && data["type"] == 3 {
            let message = &data["resolved"]["messages"][val(data, "target_id")];
            if !message["attachments"]
                .as_array()
                .is_none_or(|a| a.is_empty())
            {
                return Err(LegacyError::unavailable(
                    "Discord attachments are not yet supported; use a text-only draft.",
                ));
            }
            let text = val(message, "content");
            let mid = val(message, "id");
            if !snow(mid) {
                return Err(LegacyError::invalid(
                    "Discord did not supply the selected message.",
                ));
            }
            (
                text.to_owned(),
                json!({"platform":"discord","actor":actor,"channel_id":channel,"guild_id":guild,"message_id":mid,"author_id":message["author"]["id"],"timestamp":message["timestamp"],"url":format!("https://discord.com/channels/{}/{channel}/{mid}",if guild.is_empty(){"@me"}else{guild}),"text":text}),
            )
        } else {
            return Err(LegacyError::invalid("Unknown command."));
        };
        if text.trim().is_empty() || text.len() > 6000 {
            return Err(LegacyError::invalid(
                "A message of up to 6000 bytes is required.",
            ));
        }
        if ledger["drafts"].as_object().is_some_and(|o| o.len() >= 500) {
            return Err(LegacyError::unavailable("Discord draft storage is full."));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let row = json!({"id":id,"version":1,"actor":actor,"channel":channel,"source":source,"title":text.lines().next().unwrap_or("").chars().take(120).collect::<String>(),"observation":text.chars().take(1500).collect::<String>(),"goal":"","why":"","questions":"","itch_id":""});
        let response = panel(&row, 4);
        ledger["drafts"][&id] = row;
        response
    } else if v["type"] == 3 || v["type"] == 5 {
        let parts: Vec<&str> = val(data, "custom_id").split(':').collect();
        if parts.len() != 4
            || parts[0] != "db"
            || parts[2].len() != 32
            || !parts[2].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(denied());
        }
        let id = parts[2];
        let version = parts[3].parse::<u64>().map_err(|_| denied())?;
        let mut row = ledger["drafts"][id].clone();
        if val(&row, "actor") != actor || val(&row, "channel") != channel {
            return Err(denied());
        }
        if row["version"].as_u64() != Some(version) {
            panel(&row, 7)
        } else if parts[1] == "edit"
            && v["type"] == 3
            && val(&row, "itch_id").is_empty()
            && row["stash_pending"] != true
        {
            let fields = [
                ("title", "Title", 1, 240),
                ("observation", "Observation", 2, 1500),
                ("goal", "Goal", 2, 1500),
                ("why", "Why", 2, 1500),
                ("questions", "Questions", 2, 1500),
            ];
            let components: Vec<Value> = fields.iter().map(|(id,label,style,max)| json!({"type":1,"components":[{"type":4,"custom_id":id,"label":label,"style":style,"required":*id=="title","max_length":max,"value":val(&row,id)}]})).collect();
            json!({"type":9,"data":{"custom_id":format!("db:edited:{id}:{version}"),"title":"Edit draft","components":components}})
        } else if parts[1] == "edited"
            && v["type"] == 5
            && val(&row, "itch_id").is_empty()
            && row["stash_pending"] != true
        {
            for (key, limit) in [
                ("title", 240),
                ("observation", 1500),
                ("goal", 1500),
                ("why", 1500),
                ("questions", 1500),
            ] {
                let value = input_value(data, key);
                if value.len() > limit || (key == "title" && value.trim().is_empty()) {
                    return Err(LegacyError::invalid("Invalid draft field."));
                }
                row[key] = json!(value);
            }
            row["version"] = json!(version + 1);
            ledger["drafts"][id] = row.clone();
            panel(&row, 7)
        } else if parts[1] == "save" && v["type"] == 3 && val(&row, "itch_id").is_empty() {
            row["stash_pending"] = json!(true);
            ledger["drafts"][id] = row.clone();
            state.runtime.write_document(LEDGER, &ledger)?;
            let source = &row["source"];
            let text = val(source, "text");
            let end = text
                .char_indices()
                .map(|(i, c)| i + c.len_utf8())
                .take_while(|i| *i <= 1750)
                .last()
                .unwrap_or(0);
            let original = format!("{}\nSource: {}", &text[..end], val(source, "url"));
            let capture=state.legacy_api("/api/call",&json!({"name":"manager.capture","args":{"title":row["title"],"project":"unsorted","original":original,"use_active":false,"request_key":format!("bridge:{id}")}}))?;
            let evidence = json!([{"type":"bridge_intake","details":{"bridge_id":id,"source":source,"observation":row["observation"],"why":row["why"],"questions":row["questions"]}}]);
            state.legacy_api("/api/call",&json!({"name":"manager.update","args":{"id":capture["id"],"expected_version":capture["version"],"changes":{"desired_outcome":row["goal"],"evidence":evidence},"request_key":format!("bridge-update:{id}")}}))?;
            row["itch_id"] = capture["id"].clone();
            row["version"] = json!(version + 1);
            ledger["drafts"][id] = row;
            json!({"type":7,"data":{"content":"Stashed in DumbManager. The source and edited details are retained with the itch.","allowed_mentions":{"parse":[]},"components":[]}})
        } else {
            return Err(LegacyError::invalid("This action is unavailable."));
        }
    } else {
        return Err(LegacyError::invalid("Unsupported interaction."));
    };
    ledger["events"][event] = json!({"digest":digest,"response":response,"at":now});
    state.runtime.write_document(LEDGER, &ledger)?;
    Ok(response)
}
