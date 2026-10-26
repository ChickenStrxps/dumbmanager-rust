//! Itch records, inputs, statuses and list filters (IPC contract shapes).

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::validate;

/// What kind of itch this is. Serialized lowercase: `bug|improvement|idea`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItchKind {
    Bug,
    Improvement,
    Idea,
}

impl ItchKind {
    pub const ALL: [ItchKind; 3] = [ItchKind::Bug, ItchKind::Improvement, ItchKind::Idea];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Improvement => "improvement",
            Self::Idea => "idea",
        }
    }

    /// Exact lowercase match only; anything else is a validation error.
    pub fn parse(raw: &str) -> CoreResult<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == raw.trim())
            .ok_or_else(|| CoreError::invalid("Kind must be bug, improvement or idea."))
    }
}

/// Where an itch is in its life. Serialized lowercase: `open|stashed|ready|resolved`.
///
/// `resolved` means the user explicitly accepted the result. The store only
/// reaches it through an explicit transition from `ready`, never as a side
/// effect of saving or editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItchStatus {
    Open,
    Stashed,
    Ready,
    Resolved,
}

impl ItchStatus {
    pub const ALL: [ItchStatus; 4] = [
        ItchStatus::Open,
        ItchStatus::Stashed,
        ItchStatus::Ready,
        ItchStatus::Resolved,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Stashed => "stashed",
            Self::Ready => "ready",
            Self::Resolved => "resolved",
        }
    }

    /// Exact lowercase match only; anything else is a validation error.
    pub fn parse(raw: &str) -> CoreResult<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == raw.trim())
            .ok_or_else(|| CoreError::invalid("Status must be open, stashed, ready or resolved."))
    }
}

/// Allowed status changes.
///
/// - `open`, `stashed` and `ready` move freely between each other.
/// - `resolved` is reachable only from `ready` (the user's acceptance step).
/// - a `resolved` itch can only be reopened to `open`.
/// - "changing" to the current status is a no-op, so retries are safe.
pub fn check_transition(from: ItchStatus, to: ItchStatus) -> CoreResult<()> {
    use ItchStatus::{Open, Ready, Resolved};
    match (from, to) {
        (a, b) if a == b => Ok(()),
        (Ready, Resolved) => Ok(()),
        (_, Resolved) => Err(CoreError::invalid(
            "Only an itch that is ready for review can be accepted as resolved. Move it to Ready first.",
        )),
        (Resolved, Open) => Ok(()),
        (Resolved, _) => Err(CoreError::invalid(
            "A resolved itch can only be reopened (moved back to Open).",
        )),
        _ => Ok(()),
    }
}

/// An itch as returned over IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Itch {
    pub id: String,
    pub title: String,
    pub description: String,
    pub project: String,
    pub kind: ItchKind,
    pub status: ItchStatus,
    pub created_at: String,
    pub updated_at: String,
}

/// `itch_save` input. Unknown fields are rejected.
///
/// - No `id` (or an empty one) creates a new `open` itch; `title` and `kind`
///   are required, `description` defaults to empty and `project` falls back to
///   the default project setting, then to `unsorted`.
/// - With an `id`, the existing itch is edited. `title` is still required;
///   omitted (`null`) `description`/`project`/`kind` keep their current values.
///   Editing never changes the status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItchInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// One entry of an itch's local audit trail (not part of the first IPC milestone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchEvent {
    pub at: String,
    /// `created`, `edited` or `status`.
    pub action: String,
    pub from_status: Option<ItchStatus>,
    pub to_status: Option<ItchStatus>,
}

/// Parsed `itch_list` arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    /// Lowercased search terms; every term must appear (AND).
    pub terms: Vec<String>,
    /// `None` = all statuses.
    pub status: Option<ItchStatus>,
    /// `None` = all projects. Exact match otherwise.
    pub project: Option<String>,
}

impl ListFilter {
    /// Everything, newest update first.
    pub fn all() -> Self {
        Self::default()
    }

    /// Parses `{query, status, project}`. Missing values mean "" / "all" / "all".
    pub fn parse(
        query: Option<&str>,
        status: Option<&str>,
        project: Option<&str>,
    ) -> CoreResult<Self> {
        let query = validate::query(query.unwrap_or(""))?;
        let terms = query.split_whitespace().map(str::to_lowercase).collect();

        let status = match status.map(str::trim).unwrap_or("") {
            "" | "all" => None,
            other => Some(ItchStatus::parse(other).map_err(|_| {
                CoreError::invalid("Status filter must be all, open, stashed, ready or resolved.")
            })?),
        };

        let project = match project.map(str::trim).unwrap_or("") {
            "" => None,
            p if p.eq_ignore_ascii_case("all") => None,
            p => Some(validate::project(p)?),
        };

        Ok(Self {
            terms,
            status,
            project,
        })
    }

    /// Case-insensitive match of every term against title, description,
    /// project and kind.
    pub fn matches_text(&self, itch: &Itch) -> bool {
        if self.terms.is_empty() {
            return true;
        }
        let haystack = format!(
            "{}\n{}\n{}\n{}",
            itch.title,
            itch.description,
            itch.project,
            itch.kind.as_str()
        )
        .to_lowercase();
        self.terms
            .iter()
            .all(|term| haystack.contains(term.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ItchStatus::*;

    #[test]
    fn transition_table() {
        for status in ItchStatus::ALL {
            assert!(check_transition(status, status).is_ok(), "{status:?} no-op");
        }
        for (from, to) in [
            (Open, Stashed),
            (Open, Ready),
            (Stashed, Open),
            (Stashed, Ready),
            (Ready, Open),
            (Ready, Stashed),
            (Ready, Resolved),
            (Resolved, Open),
        ] {
            assert!(check_transition(from, to).is_ok(), "{from:?} -> {to:?}");
        }
        for (from, to) in [
            (Open, Resolved),
            (Stashed, Resolved),
            (Resolved, Stashed),
            (Resolved, Ready),
        ] {
            assert!(check_transition(from, to).is_err(), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn strict_enum_parsing() {
        assert_eq!(ItchKind::parse("bug").unwrap(), ItchKind::Bug);
        assert!(ItchKind::parse("Bug").is_err());
        assert!(ItchKind::parse("feature").is_err());
        assert_eq!(ItchStatus::parse(" ready ").unwrap(), Ready);
        assert!(ItchStatus::parse("accepted").is_err());
        assert!(ItchStatus::parse("all").is_err());
    }

    #[test]
    fn filter_defaults_and_sentinels() {
        let f = ListFilter::parse(None, None, None).unwrap();
        assert_eq!(f, ListFilter::all());
        let f = ListFilter::parse(Some(""), Some("all"), Some("ALL")).unwrap();
        assert_eq!(f, ListFilter::all());
        let f = ListFilter::parse(Some("  Login   BUG "), Some("open"), Some("Dome X3")).unwrap();
        assert_eq!(f.terms, vec!["login", "bug"]);
        assert_eq!(f.status, Some(Open));
        assert_eq!(f.project.as_deref(), Some("Dome X3"));
        assert!(ListFilter::parse(None, Some("done"), None).is_err());
        assert!(ListFilter::parse(Some("a\nb"), None, None).is_err());
    }
}
