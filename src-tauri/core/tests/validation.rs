mod common;

use common::{edit_input, new_input, Fixture};
use dumbmanager_core::{CoreError, ItchInput, ListFilter, SettingsInput};

fn invalid(result: Result<impl std::fmt::Debug, CoreError>) -> String {
    match result {
        Err(CoreError::Invalid(message)) => message,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn title_is_required_and_bounded() {
    let mut fx = Fixture::new();
    for title in [None, Some(""), Some("   ")] {
        let input = ItchInput {
            title: title.map(str::to_owned),
            kind: Some("bug".into()),
            ..Default::default()
        };
        assert!(invalid(fx.store.save_itch(&input)).contains("Title is required"));
    }
    assert!(
        invalid(fx.store.save_itch(&new_input(&"x".repeat(201), "bug", "p"))).contains("too long")
    );
    assert!(
        invalid(fx.store.save_itch(&new_input("two\nlines", "bug", "p"))).contains("single line")
    );
    assert!(invalid(
        fx.store
            .save_itch(&new_input("rtl\u{202E}gnp.exe", "bug", "p"))
    )
    .contains("single line"));
    assert!(fx.store.list_itches(&ListFilter::all()).unwrap().is_empty());
}

#[test]
fn kind_is_required_for_new_itches_and_strict() {
    let mut fx = Fixture::new();
    let input = ItchInput {
        title: Some("t".into()),
        ..Default::default()
    };
    assert!(invalid(fx.store.save_itch(&input)).contains("Choose a kind"));
    for kind in ["Bug", "feature", "", " idea2"] {
        let message = invalid(fx.store.save_itch(&new_input("t", kind, "p")));
        assert!(message.contains("Kind must be"), "{kind:?}: {message}");
    }
}

#[test]
fn description_and_project_rules() {
    let mut fx = Fixture::new();
    let mut input = new_input("t", "bug", "p");
    input.description = Some("x".repeat(8_001));
    assert!(invalid(fx.store.save_itch(&input)).contains("Description is too long"));
    input.description = Some("bell\u{7}".into());
    assert!(invalid(fx.store.save_itch(&input)).contains("control characters"));

    assert!(invalid(fx.store.save_itch(&new_input("t", "bug", "all"))).contains("reserved"));
    assert!(invalid(fx.store.save_itch(&new_input("t", "bug", "All"))).contains("reserved"));
    assert!(
        invalid(fx.store.save_itch(&new_input("t", "bug", &"p".repeat(65)))).contains("too long")
    );
    assert!(invalid(fx.store.save_itch(&new_input("t", "bug", "a\tb"))).contains("single line"));
}

#[test]
fn ids_are_validated_and_must_exist() {
    let mut fx = Fixture::new();
    let mut input = new_input("t", "bug", "p");
    input.id = Some("../../other-app/manager.sqlite".into());
    assert!(invalid(fx.store.save_itch(&input)).contains("not a valid itch id"));

    input.id = Some("0f8fad5b-d9cb-469f-a165-70867728950e".into());
    assert!(matches!(
        fx.store.save_itch(&input),
        Err(CoreError::NotFound(_))
    ));

    // Empty id means "create".
    input.id = Some("  ".into());
    let created = fx.store.save_itch(&input).unwrap();

    // Any UUID spelling finds the same itch.
    let upper = created.id.to_uppercase().replace('-', "");
    assert_eq!(fx.store.get_itch(&upper).unwrap(), created);
}

#[test]
fn edits_keep_omitted_fields() {
    let mut fx = Fixture::new();
    let mut input = new_input("t", "bug", "alpha");
    input.description = Some("keep this".into());
    let itch = fx.store.save_itch(&input).unwrap();
    let edited = fx
        .store
        .save_itch(&ItchInput {
            id: Some(itch.id.clone()),
            title: Some("t2".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(edited.title, "t2");
    assert_eq!(edited.description, "keep this");
    assert_eq!(edited.project, "alpha");
    assert_eq!(edited.kind, itch.kind);
    // Title stays required on edit.
    let mut no_title = edit_input(&itch);
    no_title.title = None;
    assert!(invalid(fx.store.save_itch(&no_title)).contains("Title is required"));
}

#[test]
fn unknown_fields_are_rejected_at_the_boundary() {
    let ok: ItchInput =
        serde_json::from_str(r#"{"title":"t","description":"","project":"p","kind":"bug"}"#)
            .unwrap();
    assert_eq!(ok.title.as_deref(), Some("t"));
    let nulls: ItchInput =
        serde_json::from_str(r#"{"id":null,"title":"t","kind":"idea"}"#).unwrap();
    assert_eq!(nulls.id, None);

    for json in [
        r#"{"title":"t","kind":"bug","status":"resolved"}"#,
        r#"{"title":"t","kind":"bug","createdAt":"2020-01-01T00:00:00.000Z"}"#,
        r#"{"title":"t","kind":"bug","default_project":"x"}"#,
    ] {
        assert!(serde_json::from_str::<ItchInput>(json).is_err(), "{json}");
    }
    assert!(serde_json::from_str::<SettingsInput>(r#"{"motion":"full","apiKey":"nope"}"#).is_err());
    assert!(
        serde_json::from_str::<SettingsInput>(r#"{"motion":"full","default_project":"x"}"#)
            .is_err()
    );
    let settings: SettingsInput =
        serde_json::from_str(r#"{"motion":"off","theme":"midnight","defaultProject":"p"}"#)
            .unwrap();
    assert_eq!(settings.default_project.as_deref(), Some("p"));
}

#[test]
fn settings_are_validated_as_a_whole() {
    let mut fx = Fixture::new();
    let before = fx.store.settings().unwrap();
    for input in [
        SettingsInput {
            motion: Some("slow".into()),
            ..Default::default()
        },
        SettingsInput {
            motion: Some("Full".into()),
            ..Default::default()
        },
        SettingsInput {
            theme: Some("daylight".into()),
            ..Default::default()
        },
        SettingsInput {
            default_project: Some("all".into()),
            ..Default::default()
        },
        SettingsInput {
            default_project: Some("x\ny".into()),
            ..Default::default()
        },
        // Valid motion but invalid theme: nothing may be written.
        SettingsInput {
            motion: Some("off".into()),
            theme: Some("neon".into()),
            ..Default::default()
        },
    ] {
        invalid(fx.store.save_settings(&input));
    }
    assert_eq!(fx.store.settings().unwrap(), before);
}

#[test]
fn list_filter_rejects_bad_arguments() {
    assert!(invalid(ListFilter::parse(None, Some("closed"), None)).contains("Status filter"));
    assert!(invalid(ListFilter::parse(Some(&"q".repeat(201)), None, None)).contains("too long"));
    assert!(invalid(ListFilter::parse(Some("tab\tsearch"), None, None)).contains("single line"));
    assert!(invalid(ListFilter::parse(None, None, Some(&"p".repeat(65)))).contains("too long"));
}
