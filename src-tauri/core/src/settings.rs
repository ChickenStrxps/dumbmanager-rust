//! App settings. Stored in SQLite next to the itches and saved atomically.
//! No secrets live here, so the whole record can be returned to the UI.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::integrations::IntegrationStatus;

/// Animation preference. Serialized lowercase: `full|reduced|off`.
/// The UI should still honour the OS reduced-motion preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    #[default]
    Full,
    Reduced,
    Off,
}

impl Motion {
    pub const ALL: [Motion; 3] = [Motion::Full, Motion::Reduced, Motion::Off];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reduced => "reduced",
            Self::Off => "off",
        }
    }

    pub fn parse(raw: &str) -> CoreResult<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.as_str() == raw.trim())
            .ok_or_else(|| CoreError::invalid("Motion must be full, reduced or off."))
    }
}

/// Colour theme. Only `midnight` exists in the first milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Midnight,
}

impl Theme {
    pub const ALL: [Theme; 1] = [Theme::Midnight];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Midnight => "midnight",
        }
    }

    pub fn parse(raw: &str) -> CoreResult<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == raw.trim())
            .ok_or_else(|| CoreError::invalid("Theme must be midnight (the only theme so far)."))
    }
}

/// Stored settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub motion: Motion,
    pub theme: Theme,
    /// Empty means "no default"; new itches without a project then use `unsorted`.
    pub default_project: String,
    /// When settings were last changed; `None` until the first save.
    pub updated_at: Option<String>,
}

/// `settings_save` input. Unknown fields are rejected; omitted fields keep
/// their current values. The whole save is validated before anything is written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsInput {
    #[serde(default)]
    pub motion: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub default_project: Option<String>,
}

/// What `settings_get` / `settings_save` return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub motion: Motion,
    pub theme: Theme,
    pub default_project: String,
    pub integration_status: Vec<IntegrationStatus>,
    pub motion_options: Vec<Motion>,
    pub theme_options: Vec<Theme>,
    /// The only folder exports are written to (and the only one the app opens).
    pub exports_dir: String,
    pub updated_at: Option<String>,
}

impl SettingsView {
    pub fn new(
        settings: Settings,
        integrations: Vec<IntegrationStatus>,
        exports_dir: &Path,
    ) -> Self {
        Self {
            motion: settings.motion,
            theme: settings.theme,
            default_project: settings.default_project,
            integration_status: integrations,
            motion_options: Motion::ALL.to_vec(),
            theme_options: Theme::ALL.to_vec(),
            exports_dir: exports_dir.display().to_string(),
            updated_at: settings.updated_at,
        }
    }
}
