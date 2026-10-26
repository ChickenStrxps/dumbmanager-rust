use serde_json::{json, Map, Value};

use super::{LegacyError, LegacyResult};

pub(super) struct Args<'a>(&'a Map<String, Value>);

impl<'a> Args<'a> {
    pub fn new(value: &'a Value, allowed: &[&str]) -> LegacyResult<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| LegacyError::invalid("Expected JSON object arguments."))?;
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(LegacyError::invalid(
                "Unsupported input field. No changes were saved.",
            ));
        }
        Ok(Self(object))
    }

    pub fn text(&self, key: &str, limit: usize) -> LegacyResult<&'a str> {
        let value = match self.0.get(key) {
            None => "",
            Some(Value::String(value)) => value,
            _ => return Err(LegacyError::invalid("Expected text input.")),
        };
        if value.chars().count() > limit || value.contains('\0') {
            return Err(LegacyError::invalid(
                "Text input is too long or contains a null character.",
            ));
        }
        Ok(value.trim())
    }

    pub fn required_text(&self, key: &str, limit: usize) -> LegacyResult<&'a str> {
        let text = self.text(key, limit)?;
        if text.is_empty() {
            return Err(LegacyError::invalid("A required text field is empty."));
        }
        Ok(text)
    }

    pub fn boolean(&self, key: &str, fallback: bool) -> LegacyResult<bool> {
        match self.0.get(key) {
            None => Ok(fallback),
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(LegacyError::invalid("Expected a boolean value.")),
        }
    }

    pub fn integer(&self, key: &str, fallback: i64, min: i64, max: i64) -> LegacyResult<i64> {
        let value = match self.0.get(key) {
            None => fallback,
            Some(v) => v
                .as_i64()
                .ok_or_else(|| LegacyError::invalid("Expected an integer."))?,
        };
        if !(min..=max).contains(&value) {
            return Err(LegacyError::invalid("Integer input is out of range."));
        }
        Ok(value)
    }

    pub fn json(&self, key: &str, fallback: Value, max: usize) -> LegacyResult<Value> {
        let value = match self.0.get(key) {
            None | Some(Value::Null) => fallback,
            Some(Value::String(raw)) if raw.is_empty() => fallback,
            Some(Value::String(raw)) => {
                if raw.len() > max {
                    return Err(LegacyError::invalid("JSON input is too large."));
                }
                serde_json::from_str(raw)
                    .map_err(|_| LegacyError::invalid("Expected valid JSON data."))?
            }
            Some(value) => value.clone(),
        };
        if value.to_string().len() > max {
            return Err(LegacyError::invalid("JSON input is too large."));
        }
        Ok(value)
    }
}

pub(super) fn text_array(
    value: &Value,
    max_items: usize,
    max_chars: usize,
) -> LegacyResult<Vec<String>> {
    let rows = value
        .as_array()
        .ok_or_else(|| LegacyError::invalid("Expected an array of text values."))?;
    if rows.len() > max_items {
        return Err(LegacyError::invalid("Too many text values."));
    }
    rows.iter()
        .map(|v| {
            let entry = json!({"text":v});
            Ok(Args::new(&entry, &["text"])?
                .required_text("text", max_chars)?
                .to_owned())
        })
        .collect()
}
