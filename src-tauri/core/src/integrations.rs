//! Honest integration states for the snapshot and settings screens.
//!
//! Nothing here connects to anything. Legacy DumbManager integrations are
//! listed as `unavailable` until they are actually implemented and tested in
//! this app; no token or configuration is read from another installation.

use serde::Serialize;

/// `ready` works now; `unconfigured` is implemented but needs user setup;
/// `unavailable` cannot be used in this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationState {
    Ready,
    Unconfigured,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    pub id: String,
    pub name: String,
    pub state: IntegrationState,
    pub detail: String,
}

fn status(id: &str, name: &str, state: IntegrationState, detail: &str) -> IntegrationStatus {
    IntegrationStatus {
        id: id.to_owned(),
        name: name.to_owned(),
        state,
        detail: detail.to_owned(),
    }
}

const NOT_MIGRATED: &str = "Not migrated to DumbManager Rust yet. Nothing connects, listens or sends, and no credentials are stored or read.";

/// Current integration list. `card_renderer_ready` reflects whether the
/// desktop shell was built with a real card renderer plugged in.
pub fn integration_statuses(card_renderer_ready: bool) -> Vec<IntegrationStatus> {
    use IntegrationState::{Ready, Unavailable};
    let card = if card_renderer_ready {
        status(
            "card-renderer",
            "Card renderer",
            Ready,
            "Exports GIF and PNG cards into this app's own exports folder. Nothing is posted or uploaded.",
        )
    } else {
        status(
            "card-renderer",
            "Card renderer",
            Unavailable,
            "Not integrated in this build yet. Card export returns an explicit unavailable error; nothing is written.",
        )
    };
    vec![
        card,
        status("t3-code", "T3 Code threads", Unavailable, NOT_MIGRATED),
        status("git", "Git and pull requests", Unavailable, NOT_MIGRATED),
        status("browser", "Browser automation", Unavailable, NOT_MIGRATED),
        status("scout", "Scout discovery", Unavailable, NOT_MIGRATED),
        status("phone", "Phone companion", Unavailable, NOT_MIGRATED),
        status("voice", "Voice input", Unavailable, NOT_MIGRATED),
        status(
            "webhook-posting",
            "Webhook posting",
            Unavailable,
            NOT_MIGRATED,
        ),
        status("jev", "Jev (TypeSafe)", Unavailable, NOT_MIGRATED),
        status("mercury", "Mercury (Inception)", Unavailable, NOT_MIGRATED),
        status(
            "dumbbridge",
            "DumbBridge (Discord)",
            Unavailable,
            NOT_MIGRATED,
        ),
        status(
            "analytics-one",
            "AnalyticsOne stats",
            Unavailable,
            NOT_MIGRATED,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_card_renderer_can_be_ready() {
        let off = integration_statuses(false);
        assert!(off.iter().all(|s| s.state == IntegrationState::Unavailable));
        let on = integration_statuses(true);
        let ready: Vec<_> = on
            .iter()
            .filter(|s| s.state == IntegrationState::Ready)
            .collect();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "card-renderer");
    }

    #[test]
    fn ids_are_unique() {
        let list = integration_statuses(false);
        let mut ids: Vec<_> = list.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), list.len());
    }
}
