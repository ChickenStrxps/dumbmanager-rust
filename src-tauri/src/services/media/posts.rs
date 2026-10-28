//! User-declared links only. No account capture or verification is implied.
use super::*;
const POSTS: &str = "social-post-links.json";

fn canonical(raw: &str) -> LegacyResult<(String, String, String)> {
    if raw.len() > 2048 || raw.chars().any(char::is_control) {
        return Err(LegacyError::invalid("Paste a supported post URL."));
    }
    let u =
        url::Url::parse(raw).map_err(|_| LegacyError::invalid("Paste a supported post URL."))?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return Err(LegacyError::invalid(
            "Use a public HTTPS post URL without credentials.",
        ));
    }
    let host = u
        .host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .trim_start_matches("m.");
    let parts: Vec<_> = u
        .path_segments()
        .map(|p| p.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (platform, id) = match host {
        "x.com" | "twitter.com" if parts.len() == 3 && parts[1] == "status" && digits(parts[2]) => {
            ("x", parts[2].to_string())
        }
        "tiktok.com"
            if parts.len() == 3
                && parts[0].starts_with('@')
                && matches!(parts[1], "video" | "photo")
                && digits(parts[2]) =>
        {
            ("tiktok", parts[2].to_string())
        }
        "youtu.be" if parts.len() == 1 => ("youtube", parts[0].to_string()),
        "youtube.com" if parts.len() == 2 && matches!(parts[0], "shorts" | "live") => {
            ("youtube", parts[1].to_string())
        }
        "youtube.com" if u.path() == "/watch" => (
            "youtube",
            u.query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default(),
        ),
        _ => {
            return Err(LegacyError::invalid(
                "Paste an X, TikTok or YouTube post URL.",
            ))
        }
    };
    if platform == "youtube"
        && (id.len() != 11
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
    {
        return Err(LegacyError::invalid("Invalid YouTube post URL."));
    }
    let mut safe = u;
    safe.set_fragment(None);
    safe.set_query(None);
    // Preserve the video identifier for watch links; strip tracking parameters elsewhere.
    if platform == "youtube" && safe.path() == "/watch" {
        safe.query_pairs_mut().append_pair("v", &id);
    }
    Ok((
        format!("{platform}:{id}"),
        platform.into(),
        safe.to_string(),
    ))
}

pub(super) fn for_itch(state: &AppState, id: &str) -> LegacyResult<Value> {
    Ok(state.runtime.read_document(POSTS)?.get(id).cloned().unwrap_or_else(|| json!({"itch_id":id,"state":"unknown","posts":[],"candidates":[],"candidate_count":0,"coverage":null,"note":"No external account check has been performed."})))
}
pub(super) fn link(state: &AppState, args: &Value) -> LegacyResult<Value> {
    if args["confirm"] != true {
        return Err(LegacyError::conflict(
            "Confirm that this URL is your post before linking it.",
        ));
    }
    let id = text(args, "itch_id", 100)?;
    let source = itch(state, id)?;
    let (key, platform, url) = canonical(text(args, "url", 2048)?)?;
    let stamp = time::now_iso();
    let doc = state.runtime.update_document(POSTS, |doc| {
        let map = doc.as_object_mut().ok_or_else(|| LegacyError::invalid("Unreadable post links."))?;
        let row = map.entry(id.to_string()).or_insert_with(|| json!({"itch_id":id,"posts":[]}));
        let posts = row["posts"].as_array_mut().ok_or_else(|| LegacyError::invalid("Unreadable post links."))?;
        posts.retain(|p| p["key"] != key);
        posts.push(json!({"key":key,"url":url,"platform":platform,"account":"User-confirmed; not verified","source":"user_confirmed_url","basis":"user_confirmed_url","matched_at":stamp,"verified":false}));
        row["project"] = source["project"].clone(); row["state"] = json!("posted"); row["updated_at"] = json!(stamp); row["candidates"] = json!([]); row["candidate_count"] = json!(0); row["coverage"] = Value::Null;
        row["note"] = json!("You linked this URL; ownership and posting were not independently verified.");
        Ok(())
    })?;
    Ok(json!({"items":[doc[id].clone()],"note":"User-confirmed URL; not independently verified."}))
}
pub(super) fn unlink(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = text(args, "itch_id", 100)?;
    itch(state, id)?;
    let stamp = time::now_iso();
    let doc = state.runtime.update_document(POSTS, |doc| {
        let map = doc.as_object_mut().ok_or_else(|| LegacyError::invalid("Unreadable post links."))?;
        map.insert(id.to_string(),json!({"itch_id":id,"state":"unconfirmed","posts":[],"candidates":[],"candidate_count":0,"coverage":null,"updated_at":stamp,"unlinked_at":stamp,"note":"Links removed. No external account check has been performed."})); Ok(())
    })?;
    Ok(doc[id].clone())
}
pub(super) fn check(state: &AppState, args: &Value) -> LegacyResult<Value> {
    let id = args["itch_id"].as_str().unwrap_or("");
    if !id.is_empty() {
        itch(state, id)?;
        return Ok(
            json!({"items":[for_itch(state,id)?],"coverage":null,"note":"No account capture or external verification is configured."}),
        );
    }
    let doc = state.runtime.read_document(POSTS)?;
    let items: Vec<Value> = doc
        .as_object()
        .into_iter()
        .flat_map(|m| m.values().cloned())
        .collect();
    Ok(
        json!({"items":items,"coverage":null,"note":"No account capture or external verification is configured."}),
    )
}
