use super::*;

const FIELDS: [(&str, &str, u64, u64); 5] = [
    ("title", "Title", 1, 240),
    ("environment", "Environment", 2, 1000),
    ("expected", "Expected", 2, 1000),
    ("observed", "Observed", 2, 1500),
    ("steps", "Steps", 2, 1500),
];

pub(super) fn modal(row: &Value) -> Value {
    let controls: Vec<Value> = FIELDS
        .iter()
        .map(|(key, label, style, max)| {
            json!({
                "type":1,"components":[{"type":4,"custom_id":key,"label":label,"style":style,
                "required":false,"max_length":max,"value":val(&row["repro"],key)}]
            })
        })
        .collect();
    json!({"type":9,"data":{"custom_id":format!("db:reproedited:{}:{}",val(row,"id"),row["version"].as_u64().unwrap_or(0)),"title":"Edit repro brief","components":controls}})
}

pub(super) fn edit(row: &mut Value, data: &Value) -> LegacyResult<()> {
    let mut fields = Vec::new();
    for (key, _, _, max) in FIELDS {
        let value = input_value(data, key);
        if value.encode_utf16().count() > max as usize {
            return Err(LegacyError::invalid(
                "Repro field exceeds its limit. Shorten it and retry.",
            ));
        }
        fields.push((key, value));
    }
    if !row["repro"].is_object() {
        row["repro"] = json!({});
    }
    for (key, value) in fields {
        row["repro"][key] = json!(value);
    }
    Ok(())
}

pub(super) fn recover(row: &mut Value, data: &Value) -> LegacyResult<()> {
    let mut buffer = json!({});
    for (key, _, _, max) in FIELDS {
        let value = input_value(data, key);
        if value.encode_utf16().count() > max as usize {
            return Err(LegacyError::invalid(
                "Repro recovery field exceeds its limit.",
            ));
        }
        buffer[key] = json!(value);
    }
    row["repro_recovery"] = buffer;
    Ok(())
}

pub(super) fn recovery_file(row: &Value) -> crate::services::browser::BridgeCard {
    let mut out = String::from("# Repro recovery - not applied\n\nThese submitted edits were not saved to the current repro. Copy desired fields into a fresh edit.\n");
    for (key, label, _, _) in FIELDS {
        out.push_str(&format!(
            "\n## {label}\n\n{}",
            literal(val(&row["repro_recovery"], key))
        ));
    }
    crate::services::browser::BridgeCard {
        bytes: out.into_bytes(),
        mime: "text/markdown; charset=utf-8",
        name: "repro-recovery.md",
    }
}

// A fence longer than any run in the text makes untrusted Markdown literal.
fn literal(text: &str) -> String {
    let longest = text.split(|c| c != '~').map(str::len).max().unwrap_or(0);
    let fence = "~".repeat(longest.max(2) + 1);
    format!("{fence}text\n{text}\n{fence}\n")
}

pub(super) fn file(row: &Value) -> crate::services::browser::BridgeCard {
    let mut out =
        String::from("# Repro brief\n\nUser-edited fields; unknown values were not inferred.\n");
    for (key, label, _, _) in FIELDS {
        let value = val(&row["repro"], key);
        out.push_str(&format!("\n## {label}\n\n"));
        if value.trim().is_empty() {
            out.push_str("Unknown / not supplied.\n");
        } else {
            out.push_str(&literal(value));
        }
    }
    out.push_str("\n## Captured source\n\n");
    if row["repro"]["include_source"] == true {
        out.push_str("Explicitly included captured snapshot and attachment filename, type and size; it may no longer match the source. Text and links may contain private information. Attachment media and links are omitted.\n\n");
        let source = &row["source"];
        let text = val(source, "text");
        if text.is_empty() {
            out.push_str("Message text unavailable or empty.\n");
        } else {
            out.push_str(&literal(text));
        }
        for attachment in source["attachments"].as_array().into_iter().flatten() {
            // Metadata is display-only: strip controls and path separators, never include links or IDs.
            let safe = |text: &str| {
                text.chars()
                    .filter(|c| !c.is_control())
                    .take(220)
                    .collect::<String>()
            };
            out.push_str("\nAttachment metadata (no media):\n");
            out.push_str(&literal(&format!(
                "Filename: {}\nType: {}\nSize: {} bytes",
                safe(
                    val(attachment, "filename")
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("attachment")
                ),
                safe(val(attachment, "content_type")),
                attachment["size"].as_u64().unwrap_or(0)
            )));
        }
        let url = val(source, "url");
        if !url.is_empty() {
            out.push_str("\nSource link (access may have changed):\n");
            out.push_str(&literal(url));
        }
    } else {
        out.push_str("Omitted by default or by choice.\n");
    }
    crate::services::browser::BridgeCard {
        bytes: out.into_bytes(),
        mime: "text/markdown; charset=utf-8",
        name: "repro-brief.md",
    }
}
