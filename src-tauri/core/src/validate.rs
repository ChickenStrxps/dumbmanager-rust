//! Strict field validation shared by itches, filters and settings.
//! Every function returns the cleaned value or a user-facing message.

use crate::error::{CoreError, CoreResult};

pub const TITLE_MAX: usize = 200;
pub const DESCRIPTION_MAX: usize = 8_000;
pub const PROJECT_MAX: usize = 64;
pub const QUERY_MAX: usize = 200;

/// Project used when neither the input nor the default project names one.
pub const FALLBACK_PROJECT: &str = "unsorted";

/// Bidirectional embedding/override/isolate characters. They can make text
/// display differently from what it contains, so single-line fields refuse them.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn check_length(label: &str, value: &str, max: usize) -> CoreResult<()> {
    let count = value.chars().count();
    if count > max {
        return Err(CoreError::invalid(format!(
            "{label} is too long ({count} characters; the limit is {max})."
        )));
    }
    Ok(())
}

fn single_line(label: &str, value: &str) -> CoreResult<()> {
    if value.chars().any(|c| c.is_control() || is_bidi_control(c)) {
        return Err(CoreError::invalid(format!(
            "{label} must be a single line without control characters."
        )));
    }
    Ok(())
}

/// Required, trimmed, single line, at most [`TITLE_MAX`] characters.
pub fn title(raw: &str) -> CoreResult<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(CoreError::invalid("Title is required."));
    }
    check_length("Title", value, TITLE_MAX)?;
    single_line("Title", value)?;
    Ok(value.to_owned())
}

/// Optional multi-line text. Line endings become `\n`; tabs and newlines are
/// the only control characters kept.
pub fn description(raw: &str) -> CoreResult<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let value = normalized.trim();
    check_length("Description", value, DESCRIPTION_MAX)?;
    if value
        .chars()
        .any(|c| (c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c))
    {
        return Err(CoreError::invalid(
            "Description contains unsupported control characters.",
        ));
    }
    Ok(value.to_owned())
}

/// Required project label. `all` is reserved for the list filter.
pub fn project(raw: &str) -> CoreResult<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(CoreError::invalid("Project is required."));
    }
    check_length("Project", value, PROJECT_MAX)?;
    single_line("Project", value)?;
    if value.eq_ignore_ascii_case("all") {
        return Err(CoreError::invalid(
            "\"all\" is reserved for the project filter. Choose another project name.",
        ));
    }
    Ok(value.to_owned())
}

/// Like [`project`], but an empty value means "no default project".
pub fn default_project(raw: &str) -> CoreResult<String> {
    if raw.trim().is_empty() {
        return Ok(String::new());
    }
    project(raw)
}

/// Search text: trimmed, single line, at most [`QUERY_MAX`] characters.
pub fn query(raw: &str) -> CoreResult<String> {
    let value = raw.trim();
    check_length("Search", value, QUERY_MAX)?;
    single_line("Search", value)?;
    Ok(value.to_owned())
}

/// Accepts any UUID spelling and returns the canonical lowercase hyphenated form.
pub fn itch_id(raw: &str) -> CoreResult<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(CoreError::invalid("An itch id is required."));
    }
    uuid::Uuid::parse_str(value)
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| CoreError::invalid("That is not a valid itch id."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_rules() {
        assert_eq!(title("  Fix login  ").unwrap(), "Fix login");
        assert!(title("   ").is_err());
        assert!(title("line\nbreak").is_err());
        assert!(title("tab\there").is_err());
        assert!(title("evil\u{202E}txt").is_err());
        assert!(title(&"x".repeat(TITLE_MAX)).is_ok());
        assert!(title(&"x".repeat(TITLE_MAX + 1)).is_err());
        // Multi-byte characters count as characters, not bytes.
        assert!(title(&"é".repeat(TITLE_MAX)).is_ok());
        // Emoji ZWJ sequences stay allowed.
        assert!(title("ship it \u{1F468}\u{200D}\u{1F4BB}").is_ok());
    }

    #[test]
    fn description_rules() {
        assert_eq!(description("a\r\nb\rc").unwrap(), "a\nb\nc");
        assert_eq!(description("\n  keep\tinner  \n").unwrap(), "keep\tinner");
        assert!(description("bell\u{7}").is_err());
        assert!(description("nul\u{0}").is_err());
        assert!(description(&"y".repeat(DESCRIPTION_MAX + 1)).is_err());
        assert_eq!(description("").unwrap(), "");
    }

    #[test]
    fn project_rules() {
        assert_eq!(project(" Dome X3 ").unwrap(), "Dome X3");
        assert!(project("").is_err());
        assert!(project("ALL").is_err());
        assert!(project("a\nb").is_err());
        assert!(project(&"p".repeat(PROJECT_MAX + 1)).is_err());
        assert_eq!(default_project("  ").unwrap(), "");
        assert!(default_project("all").is_err());
    }

    #[test]
    fn id_rules() {
        let canonical = "0f8fad5b-d9cb-469f-a165-70867728950e";
        assert_eq!(itch_id(canonical).unwrap(), canonical);
        assert_eq!(
            itch_id("0F8FAD5BD9CB469FA16570867728950E").unwrap(),
            canonical
        );
        assert!(itch_id("").is_err());
        assert!(itch_id("../../etc/passwd").is_err());
        assert!(itch_id("1; DROP TABLE itches").is_err());
    }
}
