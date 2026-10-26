mod common;

use common::{edit_input, Fixture};
use dumbmanager_core::{CoreError, ItchStatus};

#[test]
fn new_itches_are_open_and_saving_never_changes_status() {
    let mut fx = Fixture::new();
    let itch = fx.add("New", "idea", "p");
    assert_eq!(itch.status, ItchStatus::Open);

    fx.store.transition_itch(&itch.id, "ready").unwrap();
    let resolved = fx.store.transition_itch(&itch.id, "resolved").unwrap();
    let mut edit = edit_input(&resolved);
    edit.title = Some("Renamed after acceptance".to_owned());
    let edited = fx.store.save_itch(&edit).unwrap();
    assert_eq!(
        edited.status,
        ItchStatus::Resolved,
        "editing must not reopen or resolve"
    );
}

#[test]
fn resolving_requires_ready_first() {
    let mut fx = Fixture::new();
    let itch = fx.add("Needs review", "bug", "p");
    let err = fx.store.transition_itch(&itch.id, "resolved").unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)));
    assert!(err.to_string().contains("ready for review"));
    assert_eq!(
        fx.store.get_itch(&itch.id).unwrap(),
        itch,
        "a refused transition writes nothing"
    );

    fx.store.transition_itch(&itch.id, "stashed").unwrap();
    assert!(fx.store.transition_itch(&itch.id, "resolved").is_err());

    fx.store.transition_itch(&itch.id, "ready").unwrap();
    let resolved = fx.store.transition_itch(&itch.id, "resolved").unwrap();
    assert_eq!(resolved.status, ItchStatus::Resolved);
}

#[test]
fn resolved_itches_can_only_reopen() {
    let mut fx = Fixture::new();
    let itch = fx.add("Done", "bug", "p");
    fx.store.transition_itch(&itch.id, "ready").unwrap();
    fx.store.transition_itch(&itch.id, "resolved").unwrap();
    assert!(fx.store.transition_itch(&itch.id, "stashed").is_err());
    assert!(fx.store.transition_itch(&itch.id, "ready").is_err());
    let reopened = fx.store.transition_itch(&itch.id, "open").unwrap();
    assert_eq!(reopened.status, ItchStatus::Open);
}

#[test]
fn same_status_transition_is_a_safe_retry() {
    let mut fx = Fixture::new();
    let itch = fx.add("Retry", "bug", "p");
    let stashed = fx.store.transition_itch(&itch.id, "stashed").unwrap();
    let again = fx.store.transition_itch(&itch.id, "stashed").unwrap();
    assert_eq!(again, stashed);
    let history = fx.store.itch_history(&itch.id).unwrap();
    assert_eq!(
        history.len(),
        2,
        "created + one status change, no duplicate"
    );
}

#[test]
fn history_records_who_moved_what_and_only_explicit_acceptance_resolves() {
    let mut fx = Fixture::new();
    let itch = fx.add("Audit me", "improvement", "p");
    let mut edit = edit_input(&itch);
    edit.description = Some("more detail".to_owned());
    fx.store.save_itch(&edit).unwrap();
    fx.store.transition_itch(&itch.id, "ready").unwrap();
    fx.store.transition_itch(&itch.id, "resolved").unwrap();

    let history = fx.store.itch_history(&itch.id).unwrap();
    let actions: Vec<_> = history.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(actions, vec!["created", "edited", "status", "status"]);
    assert_eq!(history[0].to_status, Some(ItchStatus::Open));
    assert_eq!(history[2].from_status, Some(ItchStatus::Open));
    assert_eq!(history[2].to_status, Some(ItchStatus::Ready));
    assert_eq!(history[3].from_status, Some(ItchStatus::Ready));
    assert_eq!(history[3].to_status, Some(ItchStatus::Resolved));
    let resolutions = history
        .iter()
        .filter(|e| e.to_status == Some(ItchStatus::Resolved))
        .count();
    assert_eq!(resolutions, 1);
}

#[test]
fn transition_input_is_strict() {
    let mut fx = Fixture::new();
    let itch = fx.add("Strict", "bug", "p");
    for bad in [
        "",
        "Resolved",
        "accepted",
        "done",
        "all",
        "open; DROP TABLE itches",
    ] {
        let err = fx.store.transition_itch(&itch.id, bad).unwrap_err();
        assert!(matches!(err, CoreError::Invalid(_)), "{bad:?}");
    }
    let missing = "0f8fad5b-d9cb-469f-a165-70867728950e";
    assert!(matches!(
        fx.store.transition_itch(missing, "ready").unwrap_err(),
        CoreError::NotFound(_)
    ));
    assert!(matches!(
        fx.store.transition_itch("not-an-id", "ready").unwrap_err(),
        CoreError::Invalid(_)
    ));
}
