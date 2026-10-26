//! `app_snapshot` result shape.

use serde::Serialize;

use crate::integrations::IntegrationStatus;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounts {
    pub open: u64,
    pub stashed: u64,
    pub ready: u64,
    pub resolved: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub name: String,
    pub version: String,
    pub data_dir: String,
    pub counts: StatusCounts,
    pub integrations: Vec<IntegrationStatus>,
    pub started_at: String,
}
