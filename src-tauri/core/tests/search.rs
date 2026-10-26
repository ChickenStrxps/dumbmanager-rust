mod common;

use common::{edit_input, new_input, Fixture};
use dumbmanager_core::{ItchStatus, ListFilter};

fn titles(fx: &Fixture, query: &str, status: &str, project: &str) -> Vec<String> {
    let filter = ListFilter::parse(Some(query), Some(status), Some(project)).unwrap();
    fx.store
        .list_itches(&filter)
        .unwrap()
        .into_iter()
        .map(|itch| itch.title)
        .collect()
}

#[test]
fn search_is_case_insensitive_and_requires_every_term() {
    let mut fx = Fixture::new();
    let mut input = new_input("Login button ignores Enter", "bug", "web");
    input.description = Some("Happens on the PASSWORD field only".to_owned());
    fx.store.save_itch(&input).unwrap();
    fx.add("Login page copy", "improvement", "web");
    fx.add("Export cards as GIF", "idea", "renderer");

    assert_eq!(titles(&fx, "login", "all", "all").len(), 2);
    assert_eq!(
        titles(&fx, "LOGIN password", "all", "all"),
        vec!["Login button ignores Enter"]
    );
    assert_eq!(titles(&fx, "login gif", "all", "all"), Vec::<String>::new());
    // Project and kind are searchable too.
    assert_eq!(
        titles(&fx, "renderer", "all", "all"),
        vec!["Export cards as GIF"]
    );
    assert_eq!(
        titles(&fx, "improvement", "all", "all"),
        vec!["Login page copy"]
    );
    // Blank query lists everything.
    assert_eq!(titles(&fx, "   ", "all", "all").len(), 3);
}

#[test]
fn search_handles_unicode_case() {
    let mut fx = Fixture::new();
    fx.add("Überschrift fehlt im Export", "bug", "ÄÖÜ-Projekt");
    fx.add("Ελληνικά τίτλος", "idea", "intl");
    assert_eq!(titles(&fx, "ÜBERSCHRIFT", "all", "all").len(), 1);
    assert_eq!(titles(&fx, "äöü-projekt", "all", "all").len(), 1);
    assert_eq!(titles(&fx, "ΕΛΛΗΝΙΚΆ", "all", "all").len(), 1);
}

#[test]
fn search_treats_sql_wildcards_literally() {
    let mut fx = Fixture::new();
    fx.add("100% CPU on idle", "bug", "p");
    fx.add("Plain title", "bug", "p");
    fx.add("snake_case names", "idea", "p");
    assert_eq!(titles(&fx, "%", "all", "all"), vec!["100% CPU on idle"]);
    assert_eq!(titles(&fx, "_", "all", "all"), vec!["snake_case names"]);
    assert_eq!(
        titles(&fx, "' OR 1=1 --", "all", "all"),
        Vec::<String>::new()
    );
}

#[test]
fn status_and_project_filters_combine_with_search() {
    let mut fx = Fixture::new();
    let a = fx.add("Crash on save", "bug", "alpha");
    let b = fx.add("Crash on load", "bug", "beta");
    fx.add("Crash on exit", "bug", "alpha");
    fx.store.transition_itch(&a.id, "stashed").unwrap();
    fx.store.transition_itch(&b.id, "stashed").unwrap();

    assert_eq!(
        titles(&fx, "crash", "stashed", "all"),
        vec!["Crash on load", "Crash on save"]
    );
    assert_eq!(
        titles(&fx, "crash", "stashed", "alpha"),
        vec!["Crash on save"]
    );
    assert_eq!(titles(&fx, "", "open", "alpha"), vec!["Crash on exit"]);
    assert_eq!(titles(&fx, "", "resolved", "all"), Vec::<String>::new());
    // Project filter is exact, not a substring.
    assert_eq!(titles(&fx, "", "all", "alp"), Vec::<String>::new());
}

#[test]
fn newest_update_first_including_edits_and_transitions() {
    let mut fx = Fixture::new();
    let first = fx.add("first", "bug", "p");
    let second = fx.add("second", "bug", "p");
    let third = fx.add("third", "bug", "p");
    assert_eq!(
        titles(&fx, "", "all", "all"),
        vec!["third", "second", "first"]
    );

    // Editing moves an itch to the top.
    let mut edit = edit_input(&first);
    edit.description = Some("now with details".to_owned());
    fx.store.save_itch(&edit).unwrap();
    assert_eq!(
        titles(&fx, "", "all", "all"),
        vec!["first", "third", "second"]
    );

    // A status change also counts as an update.
    fx.store.transition_itch(&second.id, "ready").unwrap();
    assert_eq!(
        titles(&fx, "", "all", "all"),
        vec!["second", "first", "third"]
    );

    // No-op saves and same-status transitions do not reorder anything.
    fx.store.save_itch(&edit_input(&third)).unwrap();
    fx.store.transition_itch(&first.id, "open").unwrap();
    assert_eq!(
        titles(&fx, "", "all", "all"),
        vec!["second", "first", "third"]
    );
    assert_eq!(fx.store.get_itch(&third.id).unwrap(), third);
}

#[test]
fn many_itches_keep_order_within_the_same_millisecond() {
    let mut fx = Fixture::new();
    for n in 0..200 {
        fx.add(&format!("itch {n:03}"), "idea", "bulk");
    }
    let listed = titles(&fx, "", "all", "bulk");
    let expected: Vec<String> = (0..200).rev().map(|n| format!("itch {n:03}")).collect();
    assert_eq!(listed, expected);
    let filter = ListFilter::parse(Some("itch 19"), Some("open"), Some("bulk")).unwrap();
    let hits = fx.store.list_itches(&filter).unwrap();
    // Terms are ANDed: "itch" and "19" -> 019, 119 and 190..=199.
    assert_eq!(hits.len(), 12);
    assert!(hits.iter().all(|itch| itch.status == ItchStatus::Open));
}
