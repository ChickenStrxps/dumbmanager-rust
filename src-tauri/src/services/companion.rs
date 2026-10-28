//! Opt-in companion settings and listener. Nothing starts at application launch.
mod discord;
mod server;
use crate::state::AppState;
use dumbmanager_core::legacy::{self, LegacyError, LegacyResult};
use serde_json::{json, Value};
use std::net::Ipv4Addr;
use std::sync::OnceLock;
use tauri::AppHandle;

const DOC: &str = "companion.json";
const BRIDGE: &str = "bridge-discord.json";
const BOT_SECRET: &str = "bridge-discord-bot";
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Called by the parent during setup; this does not start or bind anything.
pub fn initialize(app: AppHandle) {
    let _ = APP.set(app);
}
/// Called by the parent on application exit.
pub fn shutdown() {
    server::stop();
}
fn text<'a>(v: &'a Value, key: &str, max: usize) -> LegacyResult<&'a str> {
    let s = v.get(key).and_then(Value::as_str).unwrap_or("");
    if s.len() > max || s.contains('\0') {
        return Err(LegacyError::invalid("Invalid settings field."));
    }
    Ok(s)
}
fn config(state: &AppState) -> LegacyResult<Value> {
    let d = state.runtime.read_document(DOC)?;
    Ok(
        json!({"bind":d["bind"].as_str().unwrap_or("127.0.0.1"),"port":d["port"].as_u64().unwrap_or(8877),"proxy_origin":d["proxy_origin"].as_str().unwrap_or("")}),
    )
}
fn validate_config(v: &Value) -> LegacyResult<Value> {
    let bind: Ipv4Addr = text(v, "bind", 15)?
        .parse()
        .map_err(|_| LegacyError::invalid("Use a specific loopback or private IPv4 address."))?;
    if !(bind.is_loopback() || bind.is_private()) {
        return Err(LegacyError::invalid(
            "Bind address must be loopback or private IPv4.",
        ));
    }
    let port = v["port"]
        .as_u64()
        .filter(|p| (1..=65535).contains(p))
        .ok_or_else(|| LegacyError::invalid("Choose a port from 1 to 65535."))?;
    let proxy = text(v, "proxy_origin", 512)?;
    if !proxy.is_empty() {
        let u = url::Url::parse(proxy)
            .map_err(|_| LegacyError::invalid("Enter a valid HTTPS proxy origin."))?;
        if u.scheme() != "https"
            || u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !bind.is_loopback()
        {
            return Err(LegacyError::invalid(
                "An HTTPS proxy origin requires a loopback bind and no path or credentials.",
            ));
        }
        return Ok(
            json!({"bind":bind.to_string(),"port":port,"proxy_origin":u.origin().ascii_serialization()}),
        );
    }
    Ok(json!({"bind":bind.to_string(),"port":port,"proxy_origin":""}))
}
fn status(state: &AppState) -> LegacyResult<Value> {
    let mut c = config(state)?;
    c["running"] = json!(server::running());
    c["pairing_active"] = json!(server::pairing_active());
    Ok(c)
}
fn bridge_status(state: &AppState) -> LegacyResult<Value> {
    let d = state.runtime.read_document(BRIDGE)?;
    let id = d["application_id"].as_str().unwrap_or("");
    let key = d["public_key"].as_str().unwrap_or("");
    let verified = state
        .runtime
        .read_document("bridge-discord-verified.json")?;
    let last_verified = if verified["application_id"] == id
        && verified["public_key"] == key
        && !id.is_empty()
        && !key.is_empty()
    {
        verified["at"].clone()
    } else {
        Value::Null
    };
    Ok(
        json!({"version":d["version"].as_u64().unwrap_or(0),"enabled":d["enabled"].as_bool().unwrap_or(false),"configured":!id.is_empty() && key.len()==64 && d["users"].as_array().is_some_and(|x|!x.is_empty()),"application_id":id,"public_key":key,"users":d["users"].as_array().cloned().unwrap_or_default(),"channels":d["channels"].as_array().cloned().unwrap_or_default(),"guilds":d["guilds"].as_array().cloned().unwrap_or_default(),"animated":d["animated"].as_bool().unwrap_or(true),"flashes":d["flashes"].as_bool().unwrap_or(false),"theme":d["theme"].as_str().unwrap_or(""),"bot_key_saved":state.runtime.secret(BOT_SECRET)?.is_some(),"signature_verifier":true,"last_verified_interaction":last_verified,"interaction_available":server::running()}),
    )
}
fn ids(v: &Value, key: &str) -> LegacyResult<Vec<String>> {
    let val = &v[key];
    let items: Vec<String> = if let Some(s) = val.as_str() {
        s.split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    } else if let Some(a) = val.as_array() {
        a.iter()
            .map(|x| x.as_str().unwrap_or("").to_owned())
            .collect()
    } else if val.is_null() {
        vec![]
    } else {
        return Err(LegacyError::invalid("Expected ID list."));
    };
    if items.len() > 100
        || items
            .iter()
            .any(|x| x.is_empty() || x.len() > 24 || !x.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(LegacyError::invalid(
            "Discord IDs must contain only digits.",
        ));
    }
    Ok(items)
}
fn bridge_configure(state: &AppState, v: &Value) -> LegacyResult<Value> {
    let current = bridge_status(state)?;
    if v["expected_version"].as_u64() != current["version"].as_u64() {
        return Err(LegacyError::conflict(
            "Bridge settings changed; reload before saving.",
        ));
    }
    let id = text(v, "application_id", 24)?;
    if !id.is_empty() && !id.bytes().all(|c| c.is_ascii_digit()) {
        return Err(LegacyError::invalid("Invalid application ID."));
    }
    let key = text(v, "public_key", 64)?;
    if !key.is_empty() && (key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit())) {
        return Err(LegacyError::invalid(
            "The Discord public key must be 64 hexadecimal digits.",
        ));
    }
    let users = ids(v, "users")?;
    let channels = ids(v, "channels")?;
    let guilds = ids(v, "guilds")?;
    let enabled = v["enabled"].as_bool().unwrap_or(false);
    if enabled && (id.is_empty() || key.is_empty() || users.is_empty()) {
        return Err(LegacyError::invalid(
            "Enable requires an application ID, public key and at least one allowed user.",
        ));
    }
    let token = text(v, "bot_token", 500)?;
    if token.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(LegacyError::invalid("Invalid bot token."));
    }
    let theme = text(v, "theme", 40)?;
    let next = json!({"version":current["version"].as_u64().unwrap_or(0)+1,"enabled":enabled,"application_id":id,"public_key":key,"users":users,"channels":channels,"guilds":guilds,"animated":v["animated"].as_bool().unwrap_or(true),"flashes":v["flashes"].as_bool().unwrap_or(false),"theme":theme});
    if v["forget_token"] == true {
        state.runtime.remove_secret(BOT_SECRET)?;
    } else if !token.is_empty() {
        state.runtime.set_secret(BOT_SECRET, token)?;
    }
    state.runtime.write_document(BRIDGE, &next)?;
    bridge_status(state)
}
fn commands() -> Value {
    json!([
        {"name":"DumbBridge","type":3,"integration_types":[0,1],"contexts":[0,1,2]},
        {"name":"dumbbridge","description":"Shape a message into an itch, research or T3 conversation.","type":1,"integration_types":[0,1],"contexts":[0,1,2],"options":[{"name":"message","description":"Idea, research question or message to T3","type":3,"required":false,"max_length":2000},{"name":"attachment","description":"Optional image or video evidence","type":11,"required":false},{"name":"draft","description":"Resume your saved draft ID in this channel","type":3,"required":false,"max_length":32}]}
    ])
}
fn register(state: &AppState, v: &Value) -> LegacyResult<Value> {
    let d = bridge_status(state)?;
    let app = d["application_id"].as_str().unwrap_or("");
    if app.is_empty() {
        return Err(LegacyError::invalid("Save a Discord application ID first."));
    }
    let install=format!("https://discord.com/oauth2/authorize?client_id={app}&scope=applications.commands&integration_type=1");
    let guild_install=format!("https://discord.com/oauth2/authorize?client_id={app}&scope=applications.commands&integration_type=0");
    if v["confirm"] == true {
        let token = state
            .runtime
            .secret(BOT_SECRET)?
            .ok_or_else(|| LegacyError::invalid("Save a bot token first."))?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| LegacyError::unavailable("Could not initialize Discord client."))?;
        for command in commands().as_array().unwrap() {
            let response = client
                .post(format!(
                    "https://discord.com/api/v10/applications/{app}/commands"
                ))
                .header("Authorization", format!("Bot {token}"))
                .json(command)
                .send()
                .map_err(|_| {
                    LegacyError::unavailable(
                        "Discord did not confirm all commands; retry registration.",
                    )
                })?;
            if !response.status().is_success() {
                return Err(LegacyError::unavailable("Discord did not confirm all commands; check the application and token, then retry."));
            }
        }
        return Ok(
            json!({"registered":true,"commands":commands(),"install_url":install,"guild_install_url":guild_install}),
        );
    }
    Ok(
        json!({"registered":false,"commands":commands(),"install_url":install,"guild_install_url":guild_install}),
    )
}
pub fn handle(state: &AppState, path: &str, payload: &Value) -> Option<LegacyResult<Value>> {
    if path != "/api/call" {
        return None;
    }
    let (name, args) = match legacy::call_envelope(payload) {
        Ok(x) => x,
        Err(_) => return None,
    };
    Some(match name {
        "companion.status" => status(state),
        "companion.configure" => (|| {
            if server::running() {
                return Err(LegacyError::conflict(
                    "Stop the listener before changing its address.",
                ));
            }
            let c = validate_config(&args)?;
            state.runtime.write_document(DOC, &c)?;
            status(state)
        })(),
        "companion.start" => (|| {
            let app = APP.get().ok_or_else(|| {
                LegacyError::unavailable("Companion lifecycle has not been initialized.")
            })?;
            server::start(app.clone(), &config(state)?)?;
            status(state)
        })(),
        "companion.stop" => {
            server::stop();
            status(state)
        }
        "companion.invite" => (|| {
            let c = config(state)?;
            server::invite(&c)
        })(),
        "companion.revoke" => {
            server::revoke();
            status(state)
        }
        "bridge.status" => bridge_status(state),
        "bridge.configure" => bridge_configure(state, &args),
        "bridge.discord.commands" | "bridge.discord.register" => register(state, &args),
        _ => return None,
    })
}
