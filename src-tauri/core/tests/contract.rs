//! JSON shapes promised by docs/ipc-contract.md (camelCase, exact keys).

mod common;

use std::collections::BTreeSet;

use common::Fixture;
use dumbmanager_core::{integration_statuses, AppSnapshot, CardExport, SettingsView, StatusCounts};
use serde_json::Value;

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().expect("object").keys().cloned().collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn itch_json_matches_contract() {
    let mut fx = Fixture::new();
    let itch = fx.add("Contract", "improvement", "p");
    let json = serde_json::to_value(&itch).unwrap();
    assert_eq!(
        keys(&json),
        set(&[
            "id",
            "title",
            "description",
            "project",
            "kind",
            "status",
            "createdAt",
            "updatedAt"
        ])
    );
    assert_eq!(json["kind"], "improvement");
    assert_eq!(json["status"], "open");
    let created = json["createdAt"].as_str().unwrap();
    assert_eq!(created.len(), "2026-10-26T00:00:00.000Z".len());
    assert!(created.ends_with('Z') && created.as_bytes()[10] == b'T');
}

#[test]
fn snapshot_json_matches_contract() {
    let snapshot = AppSnapshot {
        name: "DumbManager Rust".into(),
        version: "0.1.0".into(),
        data_dir: "C:/data".into(),
        counts: StatusCounts {
            open: 1,
            stashed: 0,
            ready: 0,
            resolved: 0,
            total: 1,
        },
        integrations: integration_statuses(false),
        started_at: "2026-10-26T00:00:00.000Z".into(),
    };
    let json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        keys(&json),
        set(&[
            "name",
            "version",
            "dataDir",
            "counts",
            "integrations",
            "startedAt"
        ])
    );
    assert_eq!(
        keys(&json["counts"]),
        set(&["open", "stashed", "ready", "resolved", "total"])
    );
    let first = &json["integrations"][0];
    assert_eq!(keys(first), set(&["id", "name", "state", "detail"]));
    assert_eq!(first["state"], "unavailable");
}

#[test]
fn settings_json_has_contract_fields_and_no_secrets() {
    let fx = Fixture::new();
    let view = SettingsView::new(
        fx.store.settings().unwrap(),
        integration_statuses(false),
        &fx.paths.exports_dir(),
    );
    let json = serde_json::to_value(&view).unwrap();
    for key in ["motion", "theme", "defaultProject", "integrationStatus"] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
    assert_eq!(json["motion"], "full");
    assert_eq!(json["theme"], "midnight");
    let text = json.to_string().to_lowercase();
    for word in [
        "token",
        "secret",
        "password",
        "apikey",
        "api_key",
        "webhookurl",
    ] {
        assert!(!text.contains(word), "settings must not expose {word}");
    }
}

#[test]
fn card_export_json_matches_contract() {
    let export = CardExport {
        path: "x.png".into(),
        width: 1,
        height: 2,
        frames: 1,
        bytes: 3,
    };
    let json = serde_json::to_value(&export).unwrap();
    assert_eq!(
        keys(&json),
        set(&["path", "width", "height", "frames", "bytes"])
    );
}
