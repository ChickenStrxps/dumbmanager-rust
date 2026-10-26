//! Real original frontend shapes, exercised against independent temp state.
mod common;

use common::{edit_input, Fixture};
use dumbmanager_core::{legacy::LegacyError, ItchStatus, Motion, SettingsInput, Store};
use serde_json::{json, Value};

fn call(store: &mut Store, name: &str, args: Value) -> Value {
    store
        .legacy_api(
            "/api/call",
            &json!({"name":name,"args":args,"t3_target":""}),
        )
        .unwrap()
}

fn fail(store: &mut Store, name: &str, args: Value) -> LegacyError {
    store
        .legacy_api("/api/call", &json!({"name":name,"args":args}))
        .unwrap_err()
}

fn capture(store: &mut Store, title: &str) -> Value {
    call(
        store,
        "manager.capture",
        json!({"title":title,"moment":false,
        "attachments":"[]","project":"sample","context_id":"",
        "context_snapshot":"{}","use_active":false,
        "event_at":"2026-10-25T18:04:17.123Z","request_key":uuid::Uuid::new_v4().to_string()}),
    )
}

#[test]
fn original_init_has_honest_independent_empty_shapes() {
    let mut fx = Fixture::new();
    let state = call(&mut fx.store, "manager.state", json!({}));
    assert_eq!(state["itches"], json!([]));
    assert_eq!(state["contexts"], json!([]));
    assert!(state["active_context"].is_null());
    assert_eq!(state["preferences"], json!({"motion":true}));
    assert_eq!(state["backend"], "Rust");
    assert_eq!(state["typing_generator"]["configured"], false);
    assert_eq!(state["typing_generator"]["key_saved"], false);
    let projects = call(&mut fx.store, "projects.list", json!({"limit":50}));
    assert_eq!(projects["items"], json!([]));
    assert_eq!(projects["connected"], false);
    let threads = call(&mut fx.store, "t3.threads", json!({"query":"","limit":200}));
    assert_eq!(threads["items"], json!([]));
    assert_eq!(threads["connected"], false);
    let distributions = call(&mut fx.store, "t3.distributions", json!({}));
    assert_eq!(distributions["items"], json!([]));
    assert_eq!(distributions["selected"], "");
    for (path, payload, field) in [
        ("/api/native", json!({"action":"status"}), "running"),
        ("/api/replay", json!({"action":"status"}), "armed"),
    ] {
        let status = fx.store.legacy_api(path, &payload).unwrap();
        assert_eq!(status[field], false);
        assert_eq!(status["available"], false);
    }
}

#[test]
fn original_capture_shape_survives_reload_and_means_stashed() {
    let mut fx = Fixture::new();
    let row = capture(&mut fx.store, "Keep my selected page on reload");
    assert_eq!(row["state"], "open");
    assert_eq!(row["version"], 1);
    assert_eq!(row["original"], row["title"]);
    assert_eq!(row["run_state"], "not_sent");
    assert_eq!(row["event_at"], "2026-10-25T18:04:17.123Z");
    assert_eq!(
        row["occurrences"],
        json!([{"at":"2026-10-25T18:04:17.123Z","note":row["title"]}])
    );
    for key in ["attachments", "plan", "outcomes", "feedback", "evidence"] {
        assert_eq!(row[key], json!([]));
    }
    for key in ["next_step", "desired_outcome", "test_url", "context_id"] {
        assert_eq!(row[key], "");
    }
    assert!(row["context"].is_null());
    assert!(row["recheck"].is_null());
    let id = row["id"].as_str().unwrap();
    assert_eq!(fx.store.get_itch(id).unwrap().status, ItchStatus::Stashed);
    let mut fx = fx.reopen();
    assert_eq!(call(&mut fx.store, "manager.get", json!({"id":id})), row);
    assert_eq!(
        call(&mut fx.store, "manager.state", json!({}))["itches"],
        json!([row])
    );
}

#[test]
fn moments_require_words_before_acceptance_and_edit_keeps_observation() {
    let mut fx = Fixture::new();
    let moment = call(
        &mut fx.store,
        "manager.capture",
        json!({"moment":true,"original":"I saw it happen","attachments":"[]"}),
    );
    assert_eq!(moment["state"], "needs_description");
    assert_eq!(moment["title"], "That moment…");
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.complete",
            json!({"id":moment["id"],"summary":"Done","accepted":true})
        )
        .status,
        400
    );
    let described = call(
        &mut fx.store,
        "manager.update",
        json!({"id":moment["id"],"changes":"{\"title\":\"The button ignored Enter\"}","expected_version":1,"request_key":"describe-moment"}),
    );
    assert_eq!(described["state"], "open");
    assert_eq!(described["version"], 2);
    assert_eq!(described["original"], "I saw it happen");
    assert_eq!(described["event_at"], moment["event_at"]);
    assert_eq!(described["occurrences"], moment["occurrences"]);
}

#[test]
fn retry_keys_are_durable_atomic_and_bind_to_identical_input() {
    let mut fx = Fixture::new();
    let args = json!({"title":"Keep this once","request_key":"offline-retry"});
    let first = call(&mut fx.store, "manager.capture", args.clone());
    let mut fx = fx.reopen();
    assert_eq!(call(&mut fx.store, "manager.capture", args.clone()), first);
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.capture",
            json!({"title":"Changed input","request_key":"offline-retry"})
        )
        .code,
        "conflict"
    );
    assert_eq!(fx.store.counts().unwrap().total, 1);
    let update = json!({"id":first["id"],"changes":"{\"desired_outcome\":\"One save\"}","expected_version":1,"request_key":"edit-once"});
    let edited = call(&mut fx.store, "manager.update", update.clone());
    assert_eq!(call(&mut fx.store, "manager.update", update), edited);
    assert_eq!(
        call(&mut fx.store, "manager.history", json!({"id":first["id"]}))["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn original_full_updates_round_trip_without_truncating_or_accepting() {
    let mut fx = Fixture::new();
    let row = capture(&mut fx.store, &"a".repeat(500));
    let changes = json!({"title":format!("First line\n{}","x".repeat(1800)),"desired_outcome":"Keep the page selected",
        "next_step":"Try another reload","test_url":"https://example.invalid/qa",
        "state":"ready","run_state":"agent_reported_done",
        "plan":[{"id":"page","text":"Preserve selected page","done":true}],
        "feedback":[{"at":"2026-10-25T18:05:00Z","text":"Still jumps on mobile","extra":{"kept":true}}],
        "evidence":[{"commands":["cargo test"],"note":"Saved user report, not independently verified"}],
        "recheck":{"status":"still_relevant","note":"Observed after reload","extra":[1,2]}});
    let edited = call(
        &mut fx.store,
        "manager.update",
        json!({"id":row["id"],"changes":changes.to_string(),"expected_version":1,"request_key":"full-update"}),
    );
    for (key, value) in changes.as_object().unwrap() {
        assert_eq!(&edited[key], value, "{key}");
    }
    assert_eq!(edited["version"], 2);
    assert_eq!(edited["outcomes"], json!([]));
    assert_eq!(
        fx.store
            .get_itch(row["id"].as_str().unwrap())
            .unwrap()
            .title,
        changes["title"].as_str().unwrap()
    );
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.update",
            json!({"id":row["id"],"changes":"{\"title\":\"Stale edit\"}","expected_version":1})
        )
        .status,
        409
    );
    let mut fx = fx.reopen();
    assert_eq!(
        call(&mut fx.store, "manager.get", json!({"id":row["id"]})),
        edited
    );
}

#[test]
fn invalid_changes_and_unsupported_attachments_do_not_save_partial_work() {
    let mut fx = Fixture::new();
    let row = capture(&mut fx.store, "Keep original");
    for changes in [
        json!({"title":"Changed","unknown":"do not drop"}),
        json!({"state":"resolved"}),
        json!({"test_url":"file:///C:/private"}),
        json!({"plan":[{"text":"Step","extra":"do not drop"}]}),
        json!({"run_state":"connected"}),
        json!({"recheck":{"status":"guess"}}),
        json!({"title":"x".repeat(2001)}),
    ] {
        let error = fail(
            &mut fx.store,
            "manager.update",
            json!({"id":row["id"],"changes":changes.to_string(),"request_key":"failed-update"}),
        );
        assert_eq!(error.status, 400);
        assert_eq!(
            call(&mut fx.store, "manager.get", json!({"id":row["id"]})),
            row
        );
    }
    let error = fail(
        &mut fx.store,
        "manager.capture",
        json!({"title":"Keep this text","attachments":"[\"unported-id\"]","request_key":"failed-capture"}),
    );
    assert_eq!(error.code, "unavailable");
    assert_eq!(fx.store.counts().unwrap().total, 1);
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.capture",
            json!({"title":"Valid title","project":"sample","extra":"do not drop"})
        )
        .status,
        400
    );
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.capture",
            json!({"title":"Bad event","event_at":"2026-02-30T00:00:00Z"})
        )
        .status,
        400
    );
}

#[test]
fn only_explicit_acceptance_records_outcomes_and_repeat_preserves_them() {
    let mut fx = Fixture::new();
    let row = capture(&mut fx.store, "Button ignores Enter");
    let id = &row["id"];
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.complete",
            json!({"id":id,"summary":"Fixed","accepted":false})
        )
        .status,
        400
    );
    let ready = call(
        &mut fx.store,
        "manager.update",
        json!({"id":id,"changes":json!({"state":"ready","plan":[{"id":"key","text":"Handle Enter","done":true}]}).to_string()}),
    );
    assert_eq!(ready["outcomes"], json!([]));
    let accepted = call(
        &mut fx.store,
        "manager.complete",
        json!({"id":id,"summary":"Enter now saves","evidence":"Tried the button","accepted":true,"resolved_elsewhere":false,"request_key":"accept-once"}),
    );
    assert_eq!(accepted["state"], "resolved");
    assert_eq!(accepted["version"], 3);
    assert_eq!(accepted["outcomes"][0]["acceptance"], "user_confirmed");
    assert_eq!(accepted["outcomes"][0]["evidence"], "Tried the button");
    assert_eq!(
        call(
            &mut fx.store,
            "manager.complete",
            json!({"id":id,"summary":"Already accepted","accepted":true})
        ),
        accepted
    );
    let repeated = call(
        &mut fx.store,
        "manager.repeat",
        json!({"id":id,"note":"Happened again","request_key":"repeat-once"}),
    );
    assert_eq!(repeated["state"], "open");
    assert_eq!(repeated["run_state"], "not_sent");
    assert_eq!(repeated["plan"][0]["done"], false);
    assert_eq!(repeated["outcomes"], accepted["outcomes"]);
    assert_eq!(repeated["occurrences"].as_array().unwrap().len(), 2);
    assert_eq!(repeated["occurrences"][1]["note"], "Happened again");
}

#[test]
fn original_accepts_open_itch_without_fabricated_ready_history() {
    let mut fx = Fixture::new();
    let row = capture(&mut fx.store, "Already handled elsewhere");
    let accepted = call(
        &mut fx.store,
        "manager.complete",
        json!({"id":row["id"],"summary":"Reviewed manually","accepted":true,"resolved_elsewhere":true}),
    );
    assert_eq!(accepted["state"], "resolved");
    assert_eq!(accepted["version"], 2);
    let history = fx.store.itch_history(row["id"].as_str().unwrap()).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].from_status, Some(ItchStatus::Stashed));
    assert_eq!(history[1].to_status, Some(ItchStatus::Resolved));
}

#[test]
fn exact_states_and_search_pagination_survive_coarse_core_buckets() {
    let mut fx = Fixture::new();
    let open = capture(&mut fx.store, "Open page alpha");
    let archived = capture(&mut fx.store, "Archived page alpha");
    let archived = call(
        &mut fx.store,
        "manager.update",
        json!({"id":archived["id"],"changes":"{\"state\":\"archived\"}"}),
    );
    let working = capture(&mut fx.store, "Working page alpha");
    call(
        &mut fx.store,
        "manager.update",
        json!({"id":working["id"],"changes":"{\"state\":\"working\"}"}),
    );
    let page = call(
        &mut fx.store,
        "manager.list",
        json!({"project":"sample","query":"page alpha","limit":1,"offset":0}),
    );
    assert_eq!(page["total"], 3);
    assert_eq!(page["next_offset"], 1);
    assert_eq!(page["items"][0]["id"], working["id"]);
    let only_open = call(&mut fx.store, "manager.list", json!({"state":"open"}));
    assert_eq!(only_open["items"], json!([open]));
    let mut fx = fx.reopen();
    assert_eq!(
        call(&mut fx.store, "manager.get", json!({"id":archived["id"]})),
        archived
    );
}

#[test]
fn local_context_is_explicitly_selected_and_capture_keeps_old_snapshot() {
    let mut fx = Fixture::new();
    let old = call(
        &mut fx.store,
        "manager.context.publish",
        json!({"project":"sample","thread_id":"t3-thread://v1/exact-user-thread","task":"Keep selected page","testing":"Reload inbox","entities":"page","candidates":"[\"Keep the selected page on reload\"]","workspace":"C:\\ExplicitUserNote","source":"user_entered"}),
    );
    assert!(call(&mut fx.store, "manager.state", json!({}))["active_context"].is_null());
    call(
        &mut fx.store,
        "manager.context.select",
        json!({"id":old["id"]}),
    );
    let new = call(
        &mut fx.store,
        "manager.context.publish",
        json!({"project":"sample","thread_id":"exact-user-thread","task":"Task changed","testing":"Different check","source":"user_entered"}),
    );
    assert_eq!(new["id"], old["id"]);
    assert_eq!(new["version"], 2);
    let captured = call(
        &mut fx.store,
        "manager.capture",
        json!({"title":"Observed before task changed","project":"sample","context_id":old["id"],"context_snapshot":old.to_string(),"use_active":false}),
    );
    assert_eq!(captured["context"], old);
    assert_eq!(captured["context_id"], old["id"]);
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.capture",
            json!({"title":"Wrong project","project":"other","context_id":old["id"]})
        )
        .status,
        400
    );
    let mut forged = old.clone();
    forged["version"] = json!(3);
    assert_eq!(fail(&mut fx.store,"manager.capture",json!({"title":"Future snapshot","context_id":old["id"],"context_snapshot":forged.to_string()})).status,400);
    let mut fx = fx.reopen();
    assert_eq!(call(&mut fx.store, "manager.context", json!({})), new);
    assert_eq!(
        call(&mut fx.store, "manager.get", json!({"id":captured["id"]}))["context"],
        old
    );
    call(&mut fx.store, "manager.context.select", json!({"id":""}));
    assert_eq!(
        call(&mut fx.store, "manager.context", json!({}))["state"],
        "unlinked"
    );
    assert_eq!(
        call(&mut fx.store, "t3.threads", json!({}))["items"],
        json!([])
    );
}

#[test]
fn preference_writes_bridge_native_settings_without_changing_other_preferences() {
    let mut fx = Fixture::new();
    fx.store
        .save_settings(&SettingsInput {
            motion: Some("reduced".into()),
            default_project: Some("keep".into()),
            theme: None,
        })
        .unwrap();
    assert_eq!(
        call(&mut fx.store, "manager.state", json!({}))["preferences"]["motion"],
        false
    );
    assert_eq!(
        call(&mut fx.store, "manager.settings", json!({"motion":true})),
        json!({"motion":true})
    );
    assert_eq!(fx.store.settings().unwrap().motion, Motion::Full);
    assert_eq!(fx.store.settings().unwrap().default_project, "keep");
    call(
        &mut fx.store,
        "manager.settings",
        json!({"motion":false,"request_key":"motion-off"}),
    );
    let mut fx = fx.reopen();
    assert_eq!(fx.store.settings().unwrap().motion, Motion::Off);
    assert_eq!(
        call(&mut fx.store, "manager.state", json!({}))["preferences"]["motion"],
        false
    );
    assert_eq!(
        fail(
            &mut fx.store,
            "manager.settings",
            json!({"motion":true,"theme":"extra"})
        )
        .status,
        400
    );
}

#[test]
fn suggestions_and_briefs_use_saved_local_words_without_agent_claims() {
    let mut fx = Fixture::new();
    let saved = capture(&mut fx.store, "Keep the selected page on reload");
    capture(&mut fx.store, "Unrelated request");
    let answer = call(
        &mut fx.store,
        "manager.suggest",
        json!({"text":"Keep the selected","project":"sample","jev":true,"mode":"itch","candidates":"[]","surface":"Now / Itches","fresh":true}),
    );
    assert_eq!(answer["items"][0]["text"], saved["title"]);
    assert_eq!(answer["source"], "local");
    assert_eq!(answer["generation"]["status"], "unavailable");
    assert_eq!(
        call(
            &mut fx.store,
            "manager.suggest",
            json!({"text":"Keep the selected","project":"other"})
        )["items"],
        json!([])
    );
    let brief = call(
        &mut fx.store,
        "manager.brief",
        json!({"id":saved["id"],"direction":"Snappier"}),
    );
    assert!(brief["brief"].as_str().unwrap().contains("Snappier"));
    assert_eq!(brief["handoff"], "copy_brief");
    assert!(brief["thread_url"].is_null());
    assert_eq!(
        call(&mut fx.store, "manager.get", json!({"id":saved["id"]})),
        saved
    );
}

#[test]
fn native_commands_and_legacy_commands_keep_fields_and_versions_coherent() {
    let mut fx = Fixture::new();
    let native = fx.add("Native title", "bug", "sample");
    let baseline = call(&mut fx.store, "manager.get", json!({"id":native.id}));
    assert_eq!(baseline["state"], "open");
    assert_eq!(baseline["version"], 1);
    assert_eq!(baseline["outcomes"], json!([]));
    assert_eq!(baseline["occurrences"], json!([]));
    let archived = call(
        &mut fx.store,
        "manager.update",
        json!({"id":native.id,"changes":"{\"state\":\"archived\",\"desired_outcome\":\"Keep notes\"}"}),
    );
    let mut input = edit_input(&fx.store.get_itch(&native.id).unwrap());
    input.title = Some("Native edit".into());
    fx.store.save_itch(&input).unwrap();
    let after = call(&mut fx.store, "manager.get", json!({"id":native.id}));
    assert_eq!(after["state"], "archived");
    assert_eq!(after["desired_outcome"], archived["desired_outcome"]);
    assert_eq!(after["title"], "Native edit");
    assert_eq!(after["version"], 3);
    fx.store.transition_itch(&native.id, "ready").unwrap();
    let ready = call(&mut fx.store, "manager.get", json!({"id":native.id}));
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["run_state"], "not_sent");
    assert_eq!(ready["outcomes"], json!([]));
}

#[test]
fn v1_migration_keeps_original_core_records_and_does_not_reconstruct_outcomes() {
    let mut fx = Fixture::new();
    let itch = fx.add("A real earlier Rust record", "bug", "sample");
    fx.store.transition_itch(&itch.id, "ready").unwrap();
    fx.store.transition_itch(&itch.id, "resolved").unwrap();
    let old = fx.store.get_itch(&itch.id).unwrap();
    let history = fx.store.itch_history(&itch.id).unwrap();
    let Fixture { store, paths, dir } = fx;
    drop(store);
    let conn = rusqlite::Connection::open(paths.database_path()).unwrap();
    conn.execute_batch("DROP TABLE legacy_itches; DROP TABLE legacy_contexts; DROP TABLE legacy_events; DROP TABLE legacy_requests; PRAGMA user_version=1;").unwrap();
    drop(conn);
    let mut store = Store::open(&paths.database_path()).unwrap();
    assert_eq!(store.get_itch(&itch.id).unwrap(), old);
    assert_eq!(store.itch_history(&itch.id).unwrap(), history);
    let row = call(&mut store, "manager.get", json!({"id":itch.id}));
    assert_eq!(row["state"], "resolved");
    assert_eq!(row["version"], 1);
    assert_eq!(row["outcomes"], json!([]));
    assert_eq!(row["run_state"], "not_sent");
    let events = call(&mut store, "manager.history", json!({"id":itch.id}));
    assert_eq!(events["items"][0]["action"], "native_snapshot");
    drop(store);
    drop(paths);
    drop(dir);
}

#[test]
fn unknown_and_external_capabilities_are_bounded_structured_refusals() {
    let mut fx = Fixture::new();
    for name in [
        "t3.send",
        "t3.connect",
        "manager.send",
        "manager.typing_configure",
        "manager.social.webhook_send",
        "manager.social.gif",
        "manager.scout.plan",
        "manager.learning.record",
        "browser.start",
        "git.action",
        "source.read",
        "unknown.capability",
    ] {
        let error = fail(&mut fx.store, name, json!({"arbitrary":"input"}));
        assert_eq!(error.status, 501, "{name}");
        assert_eq!(error.code, "unavailable");
        assert!(error.error.len() < 300);
        let body = serde_json::to_value(error).unwrap();
        assert!(body["error"].is_string());
    }
    for path in [
        "/api/cook",
        "/api/itch-upload",
        "/api/preview",
        "/api/shutdown",
    ] {
        assert_eq!(
            fx.store
                .legacy_api(path, &json!({"action":"send"}))
                .unwrap_err()
                .code,
            "unavailable"
        );
    }
    assert_eq!(
        fx.store
            .legacy_api("/api/unknown", &json!({}))
            .unwrap_err()
            .status,
        404
    );
    for path in [
        "https://example.invalid/api/call",
        "/api/call?proxy=1",
        "/api/..\\private",
    ] {
        assert_eq!(
            fx.store.legacy_api(path, &json!({})).unwrap_err().status,
            400
        );
    }
    assert_eq!(
        fx.store
            .legacy_api(
                "/api/call",
                &json!({"name":"manager.capture","args":[],"extra":"no"})
            )
            .unwrap_err()
            .status,
        400
    );
    assert_eq!(
        fx.store
            .legacy_api("/api/call", &json!({"name":"x".repeat(150_000),"args":{}}))
            .unwrap_err()
            .status,
        400
    );
    assert_eq!(fx.store.counts().unwrap().total, 0);
}

#[test]
fn simultaneous_original_capture_retry_saves_one_record() {
    let fx = Fixture::new();
    let path = fx.paths.database_path();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                barrier.wait();
                call(
                    &mut store,
                    "manager.capture",
                    json!({"title":"One concurrent stash","request_key":"concurrent-retry"}),
                )
            })
        })
        .collect::<Vec<_>>();
    let rows = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows[0], rows[1]);
    assert_eq!(fx.store.counts().unwrap().total, 1);
}

#[test]
fn sqlite_failure_rolls_back_core_compatibility_history_and_retry_receipt_together() {
    let mut fx = Fixture::new();
    let row = capture(&mut fx.store, "Original before storage failure");
    let id = row["id"].as_str().unwrap();
    let native = fx.store.get_itch(id).unwrap();
    let history = fx.store.itch_history(id).unwrap();
    let conn = rusqlite::Connection::open(fx.paths.database_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER fixture_fail BEFORE UPDATE ON legacy_itches BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    let args = json!({"id":id,"changes":"{\"title\":\"Edited after failure\",\"state\":\"ready\"}","expected_version":1,"request_key":"retry-after-storage-failure"});
    assert_eq!(
        fail(&mut fx.store, "manager.update", args.clone()).status,
        500
    );
    assert_eq!(fx.store.get_itch(id).unwrap(), native);
    assert_eq!(fx.store.itch_history(id).unwrap(), history);
    assert_eq!(call(&mut fx.store, "manager.get", json!({"id":id})), row);
    assert_eq!(
        call(&mut fx.store, "manager.history", json!({"id":id}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    conn.execute_batch("DROP TRIGGER fixture_fail;").unwrap();
    drop(conn);
    let saved = call(&mut fx.store, "manager.update", args);
    assert_eq!(saved["title"], "Edited after failure");
    assert_eq!(saved["state"], "ready");
    assert_eq!(saved["version"], 2);
}
