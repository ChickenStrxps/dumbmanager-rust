use dumbmanager_rust_lib::window::{is_bundled_page, NavigationPolicy};
use tauri::Url;

#[test]
fn only_local_bundled_origins_can_navigate() {
    for local in [
        "http://tauri.localhost/",
        "http://tauri.localhost/index.html?tab=review#itch",
        "https://tauri.localhost/index.html",
        "tauri://localhost/index.html",
    ] {
        assert!(is_bundled_page(&Url::parse(local).unwrap()), "{local}");
    }
    for remote in [
        "https://example.com/",
        "http://localhost/",
        "http://127.0.0.1/",
        "http://tauri.localhost:8080/",
        "http://tauri.localhost.example.com/",
        "http://user@tauri.localhost/",
        "file:///C:/test.html",
        "data:text/html,test",
        "javascript:void(0)",
        "about:blank",
    ] {
        assert!(
            !NavigationPolicy::default().allows(&Url::parse(remote).unwrap()),
            "{remote}"
        );
    }
}

#[test]
fn an_explicit_dev_origin_cannot_expand_to_other_origins() {
    let policy = NavigationPolicy::new(Some(Url::parse("http://localhost:1420/").unwrap()));
    for other in [
        "http://localhost:1421/",
        "https://localhost:1420/",
        "https://example.com/",
    ] {
        assert!(!policy.allows(&Url::parse(other).unwrap()));
    }
    assert_eq!(
        policy.allows(&Url::parse("http://localhost:1420/index.html").unwrap()),
        tauri::is_dev()
    );
}
