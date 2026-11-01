//! Bounded, transient inspection of explicitly supplied Unicode text.
use super::*;

// Fixed Unicode format ranges keep results independent of host Unicode tables.
fn kind(n: u32) -> Option<&'static str> {
    match n {
        9 => Some("Tab"),
        13 => Some("CR"),
        10 => Some("LF"),
        0..=0x1f | 0x7f..=0x9f => Some("Control"),
        0xa0 | 0x2007 | 0x202f => Some("Nonbreaking space"),
        0xfe00..=0xfe0f | 0xe0100..=0xe01ef => Some("Variation selector"),
        0xad
        | 0x61c
        | 0x6dd
        | 0x70f
        | 0x8e2
        | 0x180e
        | 0xfeff
        | 0x110bd
        | 0x110cd
        | 0xe0001
        | 0x600..=0x605
        | 0x890..=0x891
        | 0x200b..=0x200f
        | 0x202a..=0x202e
        | 0x2060..=0x2064
        | 0x2066..=0x206f
        | 0xfff9..=0xfffb
        | 0x13430..=0x1343f
        | 0x1bca0..=0x1bca3
        | 0x1d173..=0x1d17a
        | 0xe0020..=0xe007f => Some("Format"),
        _ => None,
    }
}

pub(super) fn inspect(text: &str) -> LegacyResult<Value> {
    if text.is_empty() {
        return Err(LegacyError::invalid("Supply nonempty text to inspect."));
    }
    if text.len() > 16000 || text.encode_utf16().count() > 4000 {
        return Err(LegacyError::invalid(
            "Text exceeds 4000 UTF-16 units or 16000 UTF-8 bytes.",
        ));
    }
    let labels = [
        "Tab",
        "CRLF",
        "CR",
        "LF",
        "Control",
        "Format",
        "Nonbreaking space",
        "Variation selector",
    ];
    let mut counts = [0usize; 8];
    let mut details = Vec::new();
    let (mut total, mut line, mut col) = (0usize, 1usize, 1usize);
    let mut chars = text.chars().peekable();
    let mut previous_cr = false;
    while let Some(c) = chars.next() {
        let next_lf = chars.peek() == Some(&'\n');
        if let Some(k) = kind(c as u32) {
            total += 1;
            if details.len() < 16 {
                details.push(format!(
                    "U+{:04X} {} at line {}, column {}",
                    c as u32, k, line, col
                ));
            }
            match k {
                "CR" => counts[if next_lf { 1 } else { 2 }] += 1,
                "LF" => {
                    if !previous_cr {
                        counts[3] += 1;
                    }
                }
                _ => counts[labels.iter().position(|label| *label == k).unwrap()] += 1,
            }
        }
        match c {
            '\r' if next_lf => col += 1,
            '\r' | '\n' => {
                line += 1;
                col = 1;
            }
            _ => col += 1,
        }
        previous_cr = c == '\r';
    }
    let parts: Vec<String> = labels
        .iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(label, n)| format!("{}: {}", label, n))
        .collect();
    let summary = if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    };
    let content = format!("Invisible character inspection\nDetected: {}.\nOccurrences shown: {} of {} code points. Positions are 1-based Unicode scalar columns, not bytes or display columns. CRLF counts as one line break; its code points are counted separately. No automatic repair.\n{}", summary, details.len(), total, details.join("\n"));
    Ok(
        json!({"type": 4, "data": {"flags": 64, "content": content, "allowed_mentions": {"parse": []}}}),
    )
}
