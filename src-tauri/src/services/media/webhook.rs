//! Explicit, idempotent webhook delivery. URLs are kept in DPAPI slots.
use super::*;
use std::{
    io::Read,
    sync::{Mutex, PoisonError},
    time::Duration,
};
const SETTINGS: &str = "social-webhook.json";
const RECEIPTS: &str = "social-webhook-receipts.json";
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

fn defaults() -> Value {
    json!({"version":0,"format":"discord","layout":"embed","bullets":true,"markdown":true,"configured":false,"destination":"","slot":""})
}
fn settings(state: &AppState) -> LegacyResult<Value> {
    let mut v = state.runtime.read_document(SETTINGS)?;
    if v.get("version").is_none() {
        v = defaults();
    }
    Ok(v)
}
fn public(mut v: Value) -> Value {
    if let Some(m) = v.as_object_mut() {
        m.remove("slot");
        m.remove("fingerprint");
    }
    v
}
fn validate_url(raw: &str) -> LegacyResult<url::Url> {
    if raw.len() > 2000 || !raw.is_ascii() || raw.bytes().any(|c| c <= 32 || c == 127) {
        return Err(LegacyError::invalid("Use an HTTPS webhook URL."));
    }
    let url =
        url::Url::parse(raw).map_err(|_| LegacyError::invalid("Use an HTTPS webhook URL."))?;
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    let test_loopback = cfg!(test) && url.scheme() == "http" && loopback;
    if (url.scheme() != "https" && !test_loopback)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
        || (!cfg!(test) && loopback)
    {
        return Err(LegacyError::invalid(
            "Use an HTTPS webhook URL without userinfo or a fragment.",
        ));
    }
    Ok(url)
}
fn slot(v: &Value) -> Option<&'static str> {
    match v["slot"].as_str() {
        Some("a") => Some("social-webhook-a"),
        Some("b") => Some("social-webhook-b"),
        _ => None,
    }
}
fn configure(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let old = settings(state)?;
    if old["version"].as_u64() != Some(version(args)?) {
        return Err(LegacyError::conflict(
            "Webhook settings changed in another window. Reload settings.",
        ));
    }
    let raw = args.get("url").and_then(Value::as_str).unwrap_or("").trim();
    let clear = args
        .get("clear_url")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if clear && !raw.is_empty() {
        return Err(LegacyError::invalid(
            "Choose a URL or forget the saved URL.",
        ));
    }
    let format = args
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("discord");
    let layout = args
        .get("layout")
        .and_then(Value::as_str)
        .unwrap_or("embed");
    if !matches!(format, "discord" | "generic") || !matches!(layout, "embed" | "plain") {
        return Err(LegacyError::invalid("Invalid webhook format."));
    }
    let mut next = old.clone();
    next["version"] = json!(old["version"].as_u64().unwrap_or(0) + 1);
    next["format"] = json!(format);
    next["layout"] = json!(layout);
    for key in ["bullets", "markdown"] {
        next[key] = json!(args.get(key).and_then(Value::as_bool).unwrap_or(true));
    }
    let previous = slot(&old);
    if !raw.is_empty() {
        let parsed = validate_url(raw)?;
        let new = if old["slot"] == "a" { "b" } else { "a" };
        let secret = if new == "a" {
            "social-webhook-a"
        } else {
            "social-webhook-b"
        };
        state.runtime.set_secret(secret, raw)?;
        next["slot"] = json!(new);
        next["configured"] = json!(true);
        next["destination"] = json!(match parsed.port() {
            Some(p) => format!("{}:{p}", parsed.host_str().unwrap_or("")),
            None => parsed.host_str().unwrap_or("").to_string(),
        });
    } else if clear {
        next["slot"] = json!("");
        next["configured"] = json!(false);
        next["destination"] = json!("");
    }
    state.runtime.write_document(SETTINGS, &next)?;
    if (clear || !raw.is_empty()) && previous != slot(&next) {
        if let Some(secret) = previous {
            let _ = state.runtime.remove_secret(secret);
        }
    }
    Ok(public(next))
}
fn current(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "id", 100)?;
    let (row, source) = get(state, id)?;
    let row = refresh(state, &row, &source)?;
    if row["version"].as_u64() != Some(version(args)?)
        || row["source_changed"] == true
        || stage(&source) == "changed"
    {
        return Err(LegacyError::conflict(
            "The itch or draft changed. Update the preview before sending.",
        ));
    }
    Ok(row)
}
fn strip_markdown(input: &str) -> String {
    let mut s = input.to_string();
    // Links retain their destinations in plain text.
    while let Some(open) = s.find('[') {
        let Some(close_rel) = s[open..].find("](") else {
            break;
        };
        let close = open + close_rel;
        let Some(end_rel) = s[close + 2..].find(')') else {
            break;
        };
        let end = close + 2 + end_rel;
        let start = if open > 0 && s.as_bytes()[open - 1] == b'!' {
            open - 1
        } else {
            open
        };
        let replacement = format!("{} ({})", &s[open + 1..close], &s[close + 2..end]);
        s.replace_range(start..=end, &replacement);
    }
    let mut result = Vec::new();
    for line in s.lines() {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        let line = if (1..=6).contains(&hashes)
            && trimmed
                .as_bytes()
                .get(hashes)
                .is_some_and(u8::is_ascii_whitespace)
        {
            trimmed[hashes..].trim_start()
        } else if let Some(rest) = trimmed.strip_prefix('>') {
            rest.trim_start()
        } else {
            line
        };
        let mut text = line
            .replace("```", "")
            .replace("**", "")
            .replace("__", "")
            .replace("~~", "")
            .replace('`', "");
        for marker in ['*', '_'] {
            let mut output = String::new();
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == marker && (i == 0 || !chars[i - 1].is_alphanumeric()) {
                    if let Some(end) = (i + 1..chars.len()).find(|&j| {
                        chars[j] == marker
                            && (j + 1 == chars.len() || !chars[j + 1].is_alphanumeric())
                    }) {
                        output.extend(&chars[i + 1..end]);
                        i = end + 1;
                        continue;
                    }
                }
                output.push(chars[i]);
                i += 1;
            }
            text = output;
        }
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\'
                && chars
                    .peek()
                    .is_some_and(|c| "\\`*_{}[]()#+.!>~|-".contains(*c))
            {
                out.push(chars.next().unwrap());
            } else {
                out.push(c);
            }
        }
        result.push(out);
    }
    result.join("\n")
}
fn list_body(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let mut chars = t.char_indices();
    let (_, first) = chars.next()?;
    let after = if "\u{2022}\u{25cf}\u{25aa}\u{25e6}*+-".contains(first) {
        first.len_utf8()
    } else if first.is_ascii_digit() {
        let end = t.bytes().take_while(u8::is_ascii_digit).count();
        if !matches!(t.as_bytes().get(end), Some(b'.' | b')')) {
            return None;
        }
        end + 1
    } else {
        return None;
    };
    if !t[after..].starts_with(char::is_whitespace) {
        return None;
    }
    Some(t[after..].trim())
}
fn formatted(line: &str, bullets: bool, markdown: bool) -> String {
    let body = list_body(line).unwrap_or(line.trim());
    let clean = if markdown {
        body.to_string()
    } else {
        strip_markdown(body)
    };
    if clean.is_empty() {
        String::new()
    } else {
        format!(
            "{}{}",
            if bullets {
                if markdown {
                    "- "
                } else {
                    "\u{2022} "
                }
            } else {
                ""
            },
            clean
        )
    }
}
fn message(row: &Value, opts: &Value, filename: &str) -> Value {
    let markdown = opts["markdown"] == true;
    let bullets = opts["bullets"] == true;
    let clean = |s: &str| {
        if markdown {
            s.to_string()
        } else {
            strip_markdown(s)
        }
    };
    let headline = clean(row["headline"].as_str().unwrap_or(""));
    let changes = row["summary"]
        .as_str()
        .unwrap_or("")
        .lines()
        .map(|line| formatted(line, bullets, markdown))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let caption = row["caption"]
        .as_str()
        .unwrap_or("")
        .lines()
        .map(|line| {
            if list_body(line).is_some() {
                formatted(line, bullets, markdown)
            } else {
                clean(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let embed = json!({"title":headline,"description":changes,"footer":{"text":if row["stage"]=="accepted" {"Build update"}else{"Ready to test"}},"image":{"url":format!("attachment://{filename}")},"color":16735436});
    if opts["format"] == "discord" {
        let mut payload =
            json!({"allowed_mentions":{"parse":[]},"attachments":[{"id":0,"filename":filename}]});
        if opts["layout"] == "embed" {
            payload["embeds"] = json!([embed]);
        } else {
            payload["content"] = json!(caption);
        }
        payload
    } else {
        let mut payload = json!({"event":"dumbmanager.completion","draft_id":row["id"],"itch_id":row["itch_id"],"stage":row["stage"],"headline":headline,"text":caption,"changes":changes,"format":{"layout":opts["layout"],"bullets":opts["bullets"],"markdown":opts["markdown"]},"filename":filename});
        if opts["layout"] == "embed" {
            payload["embed"] = embed;
        }
        payload
    }
}
fn filename(row: &Value) -> String {
    format!(
        "dumbmanager-{}.gif",
        row["itch_id"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(8)
            .collect::<String>()
    )
}
fn preview(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let row = current(state, args)?;
    let opts = settings(state)?;
    Ok(json!({"settings":public(opts.clone()),"payload":message(&row,&opts,&filename(&row))}))
}
fn key(request: &str) -> String {
    format!("webhook-{:x}", Sha256::digest(request.as_bytes()))
}
fn status(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let doc = state.runtime.read_document(RECEIPTS)?;
    let request = args
        .get("request_key")
        .and_then(Value::as_str)
        .unwrap_or("");
    let id = args.get("id").and_then(Value::as_str).unwrap_or("");
    let found = if !request.is_empty() {
        doc.get(key(request))
    } else {
        doc.as_object().and_then(|m| {
            m.values()
                .filter(|v| v["draft_id"] == id)
                .max_by_key(|v| v["created_at"].as_str().unwrap_or("").to_owned())
        })
    };
    Ok(found
        .cloned()
        .map(public)
        .unwrap_or(json!({"state":"not_sent"})))
}
fn reserve(
    state: &AppState,
    receipt_key: &str,
    fingerprint: &str,
    settings_version: u64,
    receipt: &Value,
) -> LegacyResult<(bool, Value)> {
    let mut first = false;
    let reserved = state.runtime.update_document(RECEIPTS, |doc| {
        let map = doc
            .as_object_mut()
            .ok_or_else(|| LegacyError::invalid("Unreadable webhook receipts."))?;
        if let Some(prior) = map.get(receipt_key) {
            if prior["fingerprint"] != fingerprint {
                return Err(LegacyError::conflict(
                    "This delivery key belongs to another request.",
                ));
            }
        } else {
            let cooldown = &map.get("_cooldown").cloned().unwrap_or(Value::Null);
            if cooldown["version"].as_u64() == Some(settings_version)
                && cooldown["until"].as_u64().unwrap_or(0) > unix_seconds()
            {
                return Err(LegacyError {
                    error: "Webhook is rate limited. Wait before trying again.".into(),
                    code: "webhook_rate_limited".into(),
                    status: 429,
                });
            }
            map.insert(receipt_key.to_string(), receipt.clone());
            first = true;
        }
        Ok(())
    })?;
    Ok((first, reserved))
}
fn send(state: &AppState, args: &Value) -> LegacyResult<Value> {
    if args["confirm"] != true {
        return Err(LegacyError::conflict(
            "Choose Send to webhook to deliver this GIF.",
        ));
    }
    let request = text(args, "request_key", 100)?;
    let sha = text(args, "sha256", 64)?;
    if request.len() < 12
        || !request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LegacyError::invalid(
            "Use a delivery key and exact GIF SHA-256.",
        ));
    }
    let id = text(args, "id", 100)?;
    let expected = version(args)?;
    let settings_version = args["settings_version"]
        .as_u64()
        .ok_or_else(|| LegacyError::invalid("Expected settings version."))?;
    let vibe = args["vibe"].as_u64().unwrap_or(0);
    let theme = args["theme"].as_str().unwrap_or("");
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            json!([id, expected, settings_version, sha, vibe, theme])
                .to_string()
                .as_bytes()
        )
    );
    let receipt_key = key(request);
    let doc = state.runtime.read_document(RECEIPTS)?;
    if let Some(prior) = doc.get(&receipt_key) {
        if prior["fingerprint"] != fingerprint {
            return Err(LegacyError::conflict(
                "This delivery key belongs to another request.",
            ));
        }
        return Ok(public(prior.clone()));
    }
    let row = current(state, args)?;
    let (opts, endpoint) = {
        let _guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let opts = settings(state)?;
        if opts["version"].as_u64() != Some(settings_version) {
            return Err(LegacyError::conflict(
                "Webhook settings changed. Review the preview again.",
            ));
        }
        let name = slot(&opts)
            .ok_or_else(|| LegacyError::invalid("Add a webhook URL in Settings first."))?;
        let endpoint = state
            .runtime
            .secret(name)?
            .ok_or_else(|| LegacyError::invalid("Set the webhook URL again."))?;
        (opts, validate_url(&endpoint)?)
    };
    let rendered = browser_gif::cached(
        state,
        &json!({"id":id,"vibe":vibe,"theme":theme}),
        Some(sha),
    )?;
    if rendered["sha256"] != sha {
        return Err(LegacyError::conflict(
            "The animated card changed. Refresh the preview.",
        ));
    }
    let data = STANDARD
        .decode(rendered["gif"].as_str().unwrap_or(""))
        .map_err(|_| LegacyError::unavailable("Could not read the rendered GIF."))?;
    let reservation_guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    current(state, args)?;
    if settings(state)?["version"].as_u64() != Some(settings_version) {
        return Err(LegacyError::conflict(
            "Webhook settings changed. Review the preview again.",
        ));
    }
    let stamp = time::now_iso();
    let receipt = json!({"id":receipt_key,"draft_id":id,"project":row["project"],"version":expected,"source_version":row["source_version"],"settings_version":settings_version,"sha256":sha,"vibe":vibe,"theme":theme,"destination":opts["destination"],"format":opts["format"],"fingerprint":fingerprint,"state":"sending","created_at":stamp,"updated_at":stamp,"note":"Sending. If interrupted, check the destination before sending another copy."});
    let (first, reserved) = reserve(
        state,
        &receipt_key,
        &fingerprint,
        settings_version,
        &receipt,
    )?;
    drop(reservation_guard);
    if !first {
        return Ok(public(reserved[&receipt_key].clone()));
    }
    let answer = deliver(
        endpoint,
        &opts,
        &message(&row, &opts, &filename(&row)),
        &filename(&row),
        data,
        request,
    );
    let updated = state.runtime.update_document(RECEIPTS, |doc| {
        let entry = doc
            .get_mut(&receipt_key)
            .ok_or_else(|| LegacyError::not_found("Delivery receipt is missing."))?;
        for (k, v) in answer.as_object().unwrap() {
            entry[k] = v.clone();
        }
        entry["updated_at"] = json!(time::now_iso());
        if let Some(delay) = answer["retry_after"].as_u64() {
            doc["_cooldown"] =
                json!({"version":settings_version,"until":unix_seconds().saturating_add(delay)});
        }
        Ok(())
    })?;
    Ok(public(updated[&receipt_key].clone()))
}
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn retry_seconds(header: Option<&str>, body: &[u8]) -> u64 {
    let from_body = || {
        serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|v| v["retry_after"].as_f64())
    };
    let seconds = header
        .and_then(|s| s.trim().parse::<f64>().ok())
        .or_else(from_body)
        .unwrap_or(60.0);
    if !seconds.is_finite() {
        60
    } else {
        seconds.ceil().clamp(1.0, 86400.0) as u64
    }
}
fn deliver(
    mut endpoint: url::Url,
    opts: &Value,
    payload: &Value,
    filename: &str,
    data: Vec<u8>,
    request: &str,
) -> Value {
    let discord = opts["format"] == "discord";
    if discord {
        let pairs: Vec<_> = endpoint
            .query_pairs()
            .filter(|(k, _)| k != "wait")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        endpoint.set_query(None);
        {
            let mut q = endpoint.query_pairs_mut();
            for (k, v) in pairs {
                q.append_pair(&k, &v);
            }
            q.append_pair("wait", "true");
        }
    }
    // Resolve once and pin the result so a DNS change cannot redirect the
    // connection to loopback after validation. The caller never sees resolver
    // errors, which may contain the secret URL.
    let host = endpoint
        .host_str()
        .unwrap_or("")
        .trim_matches(['[', ']'])
        .to_owned();
    let port = endpoint.port_or_known_default().unwrap_or(443);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let lookup = host.clone();
    std::thread::spawn(move || {
        use std::net::ToSocketAddrs;
        let found = (lookup.as_str(), port)
            .to_socket_addrs()
            .map(|a| a.collect::<Vec<_>>());
        let _ = sender.send(found);
    });
    let addresses = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(a)) if !a.is_empty() => a,
        _ => return json!({"state":"failed","note":"Could not resolve the webhook destination."}),
    };
    if !cfg!(test) && addresses.iter().any(|a| a.ip().is_loopback()) {
        return json!({"state":"failed","note":"Webhook destinations cannot resolve to loopback."});
    }
    let client = match reqwest::blocking::Client::builder()
        .no_proxy()
        .resolve_to_addrs(&host, &addresses)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return json!({"state":"failed","note":"Could not start webhook transport."}),
    };
    let part = match reqwest::blocking::multipart::Part::bytes(data)
        .file_name(filename.to_owned())
        .mime_str("image/gif")
    {
        Ok(p) => p,
        Err(_) => return json!({"state":"failed","note":"Could not prepare GIF attachment."}),
    };
    let form = reqwest::blocking::multipart::Form::new()
        .text("payload_json", payload.to_string())
        .part(if discord { "files[0]" } else { "file" }, part);
    let response = client
        .post(endpoint)
        .header("Idempotency-Key", request)
        .multipart(form)
        .send();
    let mut response = match response {
        Ok(r) => r,
        Err(_) => {
            return json!({"state":"unknown","note":"No delivery receipt came back. Check the destination before sending again."})
        }
    };
    let code = response.status().as_u16();
    if code == 429 {
        let retry = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut body = Vec::new();
        let _ = response.by_ref().take(4096).read_to_end(&mut body);
        return json!({"state":"failed","http_status":429,"retry_after":retry_seconds(retry.as_deref(), &body),"note":"Webhook is rate limited. Wait before trying again."});
    }
    if !(200..300).contains(&code) {
        return json!({"state":if code>=500 {"unknown"}else{"failed"},"http_status":code,"note":if code>=500 {"Webhook response was uncertain. Check the destination before sending again."}else{"Webhook rejected the request. Check settings before trying again."}});
    }
    if !discord {
        return json!({"state":"sent","http_status":code,"note":"Webhook accepted the GIF and text."});
    }
    let mut raw = Vec::new();
    if response.by_ref().take(65537).read_to_end(&mut raw).is_err() || raw.len() > 65536 {
        return json!({"state":"unknown","http_status":code,"note":"The receiver did not return a usable receipt. Check the destination."});
    }
    let body: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let message = body["id"].as_str().unwrap_or("");
    if message.is_empty() || !message.bytes().all(|b| b.is_ascii_digit()) {
        return json!({"state":"unknown","http_status":code,"note":"The receiver did not return a message receipt. Check the destination."});
    }
    let mut result = json!({"state":"sent","http_status":code,"message_id":message,"note":"Webhook accepted the GIF and text."});
    let channel = body["channel_id"].as_str().unwrap_or("");
    let guild = body["guild_id"].as_str().unwrap_or("");
    if !channel.is_empty() && channel.bytes().all(|b| b.is_ascii_digit()) {
        result["message_url"] = json!(format!(
            "https://discord.com/channels/{}/{channel}/{message}",
            if !guild.is_empty() && guild.bytes().all(|b| b.is_ascii_digit()) {
                guild
            } else {
                "@me"
            }
        ));
    }
    result
}
pub(super) fn call(state: &AppState, name: &str, args: &Value) -> LegacyResult<Value> {
    match name {
        "manager.social.webhook_settings" => Ok(public(settings(state)?)),
        "manager.social.webhook_configure" => configure(state, args),
        "manager.social.webhook_preview" => preview(state, args),
        "manager.social.webhook_status" => status(state, args),
        "manager.social.webhook_send" => send(state, args),
        _ => Err(LegacyError::not_found("Unknown webhook operation.")),
    }
}
