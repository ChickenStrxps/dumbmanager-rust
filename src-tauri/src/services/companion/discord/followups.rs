use super::*;

const PAGE: usize = 10;
const PER_ACTOR: usize = 100;
const TOTAL: usize = 1000;

fn cut(text: &str, limit: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= limit
        })
        .collect()
}
fn response(content: &str, components: Vec<Value>) -> Value {
    json!({"type":4,"data":{"flags":64,"content":cut(content, 1900),"allowed_mentions":{"parse":[]},"components":components}})
}
fn field(id: &str, label: &str, value: &str, style: u8, max: u64) -> Value {
    json!({"type":1,"components":[{"type":4,"custom_id":id,"label":label,"style":style,"required":false,"max_length":max,"value":value}]})
}
fn id(action: &str, key: &str, version: u64) -> String {
    format!("fu:{action}:{key}:{version}")
}
fn button(action: &str, key: &str, version: u64, label: &str, disabled: bool) -> Value {
    json!({"type":2,"style":2,"custom_id":id(action,key,version),"label":label,"disabled":disabled})
}

// Literal fenced text prevents user-controlled Markdown from changing the recovery message.
pub(super) fn recovery(reason: &str, note: &str, snapshot: &str) -> Value {
    fn literal(s: &str) -> String {
        let tildes = s
            .split(|c| c != '~')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1;
        let ticks = s
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1;
        let fence = if tildes <= ticks {
            "~".repeat(tildes)
        } else {
            "`".repeat(ticks)
        };
        format!("{fence}\n{s}\n{fence}")
    }
    let note = cut(note, 200);
    let snapshot = cut(snapshot, 1000);
    let mut fields = vec![json!({"name":"Submitted note", "value":literal(&note)})];
    let mut chunk = String::new();
    for c in snapshot.chars() {
        if chunk.encode_utf16().count() + c.len_utf16() > 500 {
            fields.push(json!({"name":"Submitted snapshot (continued)","value":literal(&chunk)}));
            chunk.clear();
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        fields.push(json!({"name":"Submitted snapshot (continued)","value":literal(&chunk)}));
    }
    json!({"type":4,"data":{"flags":64,"content":reason,"allowed_mentions":{"parse":[]},"embeds":[{"fields":fields}]}})
}

pub(super) fn save_modal(row: &Value) -> Value {
    let snapshot = cut(val(&row["source"], "text"), 1000);
    json!({"type":9,"data":{"custom_id":format!("db:followupcommit:{}:{}",val(row,"id"),row["version"]),"title":"Save private follow-up","components":[
        field("note","Note (optional)","",1,200),
        field("snapshot","Snapshot first 1000 (edit or clear)",&snapshot,2,1000)
    ]}})
}

pub(super) fn save(ledger: &mut Value, draft: &Value, data: &Value) -> LegacyResult<Value> {
    let note = input_value(data, "note");
    let snapshot = input_value(data, "snapshot");
    if note.encode_utf16().count() > 200 || snapshot.encode_utf16().count() > 1000 {
        return Ok(recovery(
            "Fields exceed their limits; shorten them and retry. Nothing was saved.",
            &note,
            &snapshot,
        ));
    }
    let app = val(draft, "app");
    let actor = val(draft, "actor");
    let source = &draft["source"];
    let identity = if !val(source, "message_id").is_empty() {
        json!([
            app,
            actor,
            val(source, "channel_id"),
            val(source, "guild_id"),
            val(source, "message_id")
        ])
    } else {
        json!([app, actor, val(draft, "id")])
    };
    let source_key = format!("{:x}", Sha256::digest(identity.to_string().as_bytes()));
    if let Some(existing) = ledger["followups"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .find(|r| {
            val(r, "app") == app && val(r, "actor") == actor && val(r, "source_key") == source_key
        })
    {
        return Ok(item(
            existing,
            "Already saved; existing note and snapshot were preserved.",
        ));
    }
    let records = ledger["followups"]
        .as_object()
        .ok_or_else(|| LegacyError::unavailable("Follow-up storage is unavailable."))?;
    if records.len() >= TOTAL
        || records
            .values()
            .filter(|v| val(v, "app") == app && val(v, "actor") == actor)
            .count()
            >= PER_ACTOR
    {
        return Ok(recovery("Storage is full (100 per user, 1000 total). Delete a bookmark and retry; nothing was saved or evicted.", &note, &snapshot));
    }
    let key = uuid::Uuid::new_v4().simple().to_string();
    let record = json!({"id":key,"source_key":source_key,"version":1,"app":app,"actor":actor,"note":note,"snapshot":snapshot,
        "guild":val(source,"guild_id"),"channel":val(source,"channel_id"),"message":val(source,"message_id"),
        "captured_at":dumbmanager_core::time::now_iso(),"done":false});
    ledger["followups"][&key] = record.clone();
    Ok(item(&record, "Saved locally. This is a private captured snapshot, not live content. Attachments were not saved."))
}

pub(super) fn list(ledger: &Value, app: &str, actor: &str, requested: usize) -> Value {
    let mut records: Vec<&Value> = ledger["followups"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter(|r| val(r, "app") == app && val(r, "actor") == actor)
        .collect();
    records.sort_by(|a, b| {
        val(b, "captured_at")
            .cmp(val(a, "captured_at"))
            .then_with(|| val(a, "id").cmp(val(b, "id")))
    });
    let pages = records.len().max(1).div_ceil(PAGE);
    let page = requested.min(pages - 1);
    let choices: Vec<Value> = records.iter().skip(page*PAGE).take(PAGE).map(|r| {
        let label = if val(r,"note").trim().is_empty() { "(no note)" } else { val(r,"note") };
        json!({"label":cut(&format!("{} {}",if r["done"] == true {"Done ·"} else {"Open ·"},label),100),"value":val(r,"id")})
    }).collect();
    let mut components = Vec::new();
    if !choices.is_empty() {
        components.push(json!({"type":1,"components":[{"type":3,"custom_id":id("select",&page.to_string(),0),"placeholder":"Open a saved follow-up","min_values":1,"max_values":1,"options":choices}]}));
    }
    components.push(json!({"type":1,"components":[button("page",&page.saturating_sub(1).to_string(),0,"Previous",page==0),button("page",&(page+1).to_string(),0,"Next",page+1>=pages)]}));
    response(&format!("**Private follow-ups** · page {} / {} · {} saved\nStored locally for this app and user. Select one to view its captured snapshot. No notifications or live refresh. The workspace must be reachable.",page+1,pages,records.len()),components)
}

fn item(row: &Value, notice: &str) -> Value {
    let key = val(row, "id");
    let version = row["version"].as_u64().unwrap_or(0);
    let guild = val(row, "guild");
    let channel = val(row, "channel");
    let message = val(row, "message");
    let link = if !message.is_empty() {
        format!(
            "https://discord.com/channels/{}/{channel}/{message}",
            if guild.is_empty() { "@me" } else { guild }
        )
    } else {
        "No message link (slash input).".into()
    };
    let snapshot = if val(row, "snapshot").is_empty() {
        "(empty / omitted)"
    } else {
        val(row, "snapshot")
    };
    let note = if val(row, "note").is_empty() {
        "(no note)"
    } else {
        val(row, "note")
    };
    let content = format!("**Private follow-up · {}**\n{}\nNote: {}\nCaptured at: {}\nSource channel: {} · server: {}\nSource link (may be inaccessible): {}\n\nCaptured snapshot (not live; attachments omitted):\n{}",if row["done"] == true {"done"}else{"open"},notice,note,val(row,"captured_at"),channel,if guild.is_empty(){"DM / unknown"}else{guild},link,snapshot);
    response(
        &content,
        vec![
            json!({"type":1,"components":[button("edit",key,version,"Edit note",false),button("toggle",key,version,if row["done"] == true {"Reopen"}else{"Mark done"},false),button("delete",key,version,"Delete bookmark",false),button("page","0",0,"Inbox",false)]}),
        ],
    )
}

pub(super) fn receive(
    v: &Value,
    config: &Value,
    ledger: &mut Value,
    actor: &str,
) -> LegacyResult<Value> {
    let data = &v["data"];
    let parts: Vec<&str> = val(data, "custom_id").split(':').collect();
    if parts.len() != 4 || parts[0] != "fu" {
        return Err(denied());
    }
    let app = val(config, "application_id");
    if parts[1] == "page" && v["type"] == 3 {
        let page = parts[2].parse::<usize>().map_err(|_| denied())?;
        return Ok(list(ledger, app, actor, page));
    }
    if parts[1] == "select" && v["type"] == 3 {
        let values = data["values"]
            .as_array()
            .filter(|a| a.len() == 1)
            .ok_or_else(denied)?;
        let key = values[0].as_str().ok_or_else(denied)?;
        let row = ledger["followups"].get(key).ok_or_else(denied)?;
        if val(row, "app") != app || val(row, "actor") != actor {
            return Err(denied());
        }
        return Ok(item(row, ""));
    }
    let key = parts[2];
    if key.len() != 32 || !key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(denied());
    }
    let version = parts[3].parse::<u64>().map_err(|_| denied())?;
    if ledger["followups"].get(key).is_none() && parts[1] == "edited" && v["type"] == 5 {
        return Ok(recovery(
            "This bookmark no longer exists; the submitted note was not saved.",
            &input_value(data, "note"),
            "",
        ));
    }
    let row = ledger["followups"].get_mut(key).ok_or_else(denied)?;
    if val(row, "app") != app || val(row, "actor") != actor {
        return Err(denied());
    }
    if row["version"].as_u64() != Some(version) {
        if parts[1] == "edited" && v["type"] == 5 {
            return Ok(recovery("This edit was stale and was not saved. Copy the submitted note and reopen the current bookmark.", &input_value(data,"note"), ""));
        }
        return Ok(item(
            row,
            "This control was stale; the current saved item is shown.",
        ));
    }
    match parts[1] {
        "edit" if v["type"] == 3 => Ok(
            json!({"type":9,"data":{"custom_id":id("edited",key,version),"title":"Edit follow-up note","components":[field("note","Note (optional)",val(row,"note"),1,200)]}}),
        ),
        "edited" if v["type"] == 5 => {
            let note = input_value(data, "note");
            if note.encode_utf16().count() > 200 {
                return Ok(recovery(
                    "Note exceeds its limit and was not saved. Shorten it and retry.",
                    &note,
                    "",
                ));
            }
            row["note"] = json!(note);
            row["version"] = json!(version + 1);
            Ok(item(row, "Note saved."))
        }
        "toggle" if v["type"] == 3 => {
            row["done"] = json!(row["done"] != true);
            row["version"] = json!(version + 1);
            Ok(item(row, "Status saved."))
        }
        "delete" if v["type"] == 3 => {
            ledger["followups"].as_object_mut().unwrap().remove(key);
            Ok(response("Bookmark and its captured snapshot deleted. The original intake draft source, source message and any itch are retained.",vec![json!({"type":1,"components":[button("page","0",0,"Inbox",false)]})]))
        }
        _ => Err(LegacyError::invalid(
            "This follow-up action is unavailable.",
        )),
    }
}
