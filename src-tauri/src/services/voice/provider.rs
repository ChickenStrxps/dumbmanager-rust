//! Provider provisioning; endpoints are selected in code, never supplied by clients.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use std::{
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(super) const OPENAI: &str = "https://api.openai.com/v1/realtime/client_secrets";
pub(super) const GEMINI: &str = "https://generativelanguage.googleapis.com/v1beta/auth_tokens";
pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
// Gregorian UTC conversion, independent of local timezone.
fn rfc3339(t: u64) -> String {
    let days = (t / 86400) as i64 + 719468;
    let era = days / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    let s = t % 86400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        (s % 3600) / 60,
        s % 60
    )
}
fn err(message: &str, code: &str, status: u16) -> LegacyError {
    LegacyError {
        error: message.into(),
        code: code.into(),
        status,
    }
}
fn post(url: &str, provider: &str, key: &str, body: &Value) -> LegacyResult<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| err("Provider client unavailable.", "provider_unavailable", 503))?;
    let mut req = client.post(url).json(body);
    req = if provider == "openai_realtime" {
        req.bearer_auth(key)
    } else {
        req.header("x-goog-api-key", key)
    };
    let response = req.send().map_err(|_| {
        err(
            "The provider could not be reached. Check the connection and try again.",
            "provider_unavailable",
            503,
        )
    })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let msg = match status {
            401 => "The provider rejected the credential.",
            403 => "This account does not have access.",
            404 => "The configured model or API is unavailable.",
            429 => "The provider is rate limited.",
            _ => "The provider could not provision a session.",
        };
        return Err(err(
            msg,
            if status == 429 {
                "provider_rate_limited"
            } else {
                "provider_unavailable"
            },
            if status == 429 { 429 } else { 503 },
        ));
    }
    let mut raw = Vec::new();
    response.take(131073).read_to_end(&mut raw).map_err(|_| {
        err(
            "The provider returned an unexpected session response.",
            "provider_protocol",
            502,
        )
    })?;
    if raw.len() > 131072 {
        return Err(err(
            "The provider returned an unexpected session response.",
            "provider_protocol",
            502,
        ));
    }
    let data: Value = serde_json::from_slice(&raw).map_err(|_| {
        err(
            "The provider returned an unexpected session response.",
            "provider_protocol",
            502,
        )
    })?;
    if !data.is_object() {
        return Err(err(
            "The provider returned an unexpected session response.",
            "provider_protocol",
            502,
        ));
    }
    Ok(data)
}
pub(super) fn bootstrap(
    provider: &str,
    key: &str,
    saved: &Value,
    tools: &Value,
    instructions: &str,
) -> LegacyResult<Value> {
    bootstrap_at(
        provider,
        key,
        saved,
        tools,
        instructions,
        if provider == "openai_realtime" {
            OPENAI
        } else {
            GEMINI
        },
    )
}
fn bootstrap_at(
    provider: &str,
    key: &str,
    saved: &Value,
    tools: &Value,
    instructions: &str,
    endpoint: &str,
) -> LegacyResult<Value> {
    let model = saved["model"].as_str().unwrap_or("");
    let settings = &saved["settings"];
    if provider == "openai_realtime" {
        let funcs=tools.as_array().unwrap().iter().map(|t|json!({"type":"function","name":t["name"],"description":t["description"],"parameters":t["parameters"]})).collect::<Vec<_>>();
        let body = json!({"expires_after":{"anchor":"created_at","seconds":60},"session":{"type":"realtime","model":model,"instructions":instructions,"audio":{"input":{"turn_detection":{"type":"server_vad","interrupt_response":true}},"output":{"voice":settings["voice"]}},"tools":funcs,"tool_choice":"auto"}});
        let data = post(endpoint, provider, key, &body)?;
        let secret = data["value"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or_else(|| {
                err(
                    "OpenAI did not return a client secret.",
                    "provider_protocol",
                    502,
                )
            })?;
        let expires_at = data["expires_at"]
            .as_u64()
            .filter(|t| *t > now())
            .ok_or_else(|| {
                err(
                    "OpenAI did not return a usable client-secret expiry.",
                    "provider_protocol",
                    502,
                )
            })?;
        Ok(
            json!({"transport":"webrtc","client_secret":secret,"expires_at":expires_at,"endpoint":"https://api.openai.com/v1/realtime/calls"}),
        )
    } else {
        let funcs=tools.as_array().unwrap().iter().map(|t|json!({"name":t["name"],"description":t["description"],"parametersJsonSchema":t["parameters"],"behavior":"NON_BLOCKING"})).collect::<Vec<_>>();
        let setup = json!({"model":format!("models/{}",model.strip_prefix("models/").unwrap_or(model)),"generationConfig":{"responseModalities":["AUDIO"],"speechConfig":{"voiceConfig":{"prebuiltVoiceConfig":{"voiceName":settings["voice"]}}}},"systemInstruction":{"parts":[{"text":instructions}]},"tools":[{"functionDeclarations":funcs}],"inputAudioTranscription":{},"outputAudioTranscription":{},"realtimeInputConfig":{"automaticActivityDetection":{},"activityHandling":"START_OF_ACTIVITY_INTERRUPTS"}});
        let t = now();
        let expiry = rfc3339(t + settings["minutes"].as_u64().unwrap_or(15) * 60);
        let body = json!({"uses":1,"expireTime":expiry,"newSessionExpireTime":rfc3339(t+60),"bidiGenerateContentSetup":setup});
        let data = post(endpoint, provider, key, &body)?;
        let secret = data["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or_else(|| {
                err(
                    "Gemini did not return an ephemeral token.",
                    "provider_protocol",
                    502,
                )
            })?;
        Ok(
            json!({"transport":"websocket","client_secret":secret,"expires_at":expiry,"setup":setup,"endpoint":"wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained"}),
        )
    }
}
