use base64::{engine::general_purpose::STANDARD, Engine};
use dumbmanager_core::component_updates::{
    Availability, Component, FeedError, Installed, Target, VerifiedFeed, SIGNING_CONTEXT,
};
use ed25519_dalek::{Signer, SigningKey};
use semver::Version;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const BUNDLE: &[u8] = b"fixture bundle";

fn release(version: &str, requirement: &str, major: u32) -> Value {
    json!({
        "component": "dumbissues", "version": version, "app": requirement,
        "contracts": {"draft": major}, "target": "any",
        "url": "https://example.test/dumbissues.zip", "size": BUNDLE.len(),
        "sha256": format!("{:x}", Sha256::digest(BUNDLE))
    })
}

fn envelope(releases: &[Value], key: &SigningKey) -> Value {
    let payload =
        serde_json::to_vec(&json!({"schema": 1, "sequence": 7, "releases": releases})).unwrap();
    let signature = key.sign(&[SIGNING_CONTEXT, &payload].concat());
    json!({"payload": STANDARD.encode(payload), "signature": STANDARD.encode(signature.to_bytes())})
}

#[test]
fn selects_latest_compatible_enabled_set_without_downgrade() {
    let key = SigningKey::from_bytes(&[3; 32]);
    let source = envelope(
        &[
            release("1.0.0", ">=1.0.0, <2.0.0", 1),
            release("1.2.0", ">=1.0.0, <2.0.0", 1),
            release("2.0.0", ">=2.0.0", 1),
            release("3.0.0", ">=1.0.0", 2),
        ],
        &key,
    );
    let feed =
        VerifiedFeed::verify(source.to_string().as_bytes(), &key.verifying_key(), 7).unwrap();
    assert_eq!(feed.sequence(), 7);
    let app = Version::new(1, 0, 0);
    let installed = Version::new(1, 1, 0);
    let current = Version::new(1, 3, 0);
    let v1 = [("draft".to_string(), 1)].into();
    let unsupported = [("draft".to_string(), 3)].into();
    let select = |enabled, version, supported_contracts| {
        feed.select(
            Component::Dumbissues,
            &app,
            Target::WindowsX86_64,
            Installed {
                enabled,
                version,
                supported_contracts,
            },
        )
    };
    assert_eq!(select(false, None, &v1), Availability::Disabled);
    assert_eq!(select(true, Some(&current), &v1), Availability::Current);
    assert_eq!(select(true, None, &unsupported), Availability::Incompatible);
    let Availability::Update(candidate) = select(true, Some(&installed), &v1) else {
        panic!("expected update")
    };
    assert_eq!(candidate.version(), &Version::new(1, 2, 0));
    assert_eq!(candidate.verify_artifact(BUNDLE), Ok(()));
    assert_eq!(
        candidate.verify_artifact(b"fixture bundlx"),
        Err(FeedError::Artifact)
    );
    assert_eq!(
        candidate.verify_artifact(b"incomplete"),
        Err(FeedError::Artifact)
    );
}

#[test]
fn rejects_stale_tampered_untrusted_and_ambiguous_feeds() {
    let key = SigningKey::from_bytes(&[3; 32]);
    let other = SigningKey::from_bytes(&[4; 32]);
    let package = release("1.0.0", "*", 1);
    let signed = envelope(&[package.clone()], &key);
    let verify = |feed: &Value, seq| {
        VerifiedFeed::verify(feed.to_string().as_bytes(), &key.verifying_key(), seq).unwrap_err()
    };
    assert_eq!(verify(&signed, 8), FeedError::Stale);
    assert_eq!(
        verify(&envelope(&[package.clone()], &other), 0),
        FeedError::Signature
    );
    let mut tampered = signed;
    tampered["payload"] = json!(STANDARD.encode(b"{}"));
    assert_eq!(verify(&tampered, 0), FeedError::Signature);
    assert_eq!(
        verify(&envelope(&[package.clone(), package], &key), 0),
        FeedError::Format
    );
}

#[test]
fn windows_specific_package_wins_a_same_version_tie() {
    let key = SigningKey::from_bytes(&[3; 32]);
    let portable = release("1.0.0", "*", 1);
    let mut windows = portable.clone();
    windows["target"] = json!("windows-x86-64");
    windows["url"] = json!("https://example.test/dumbissues-win.zip");
    let source = envelope(&[windows, portable], &key);
    let feed =
        VerifiedFeed::verify(source.to_string().as_bytes(), &key.verifying_key(), 0).unwrap();
    let app = Version::new(1, 0, 0);
    let supported_contracts = [("draft".to_string(), 1)].into();
    for (target, suffix) in [
        (Target::WindowsX86_64, "dumbissues-win.zip"),
        (Target::Any, "dumbissues.zip"),
    ] {
        let choice = feed.select(
            Component::Dumbissues,
            &app,
            target,
            Installed {
                enabled: true,
                version: None,
                supported_contracts: &supported_contracts,
            },
        );
        let Availability::Update(candidate) = choice else {
            panic!("expected update")
        };
        assert!(candidate.url().ends_with(suffix));
    }
}

#[test]
fn accepts_the_openssl_feed_producer_fixture() {
    let fixture: Value = serde_json::from_str(include_str!("component_feed.fixture.json")).unwrap();
    let public_key = STANDARD
        .decode(fixture["public_key"].as_str().unwrap())
        .unwrap();
    let key = ed25519_dalek::VerifyingKey::from_bytes(&public_key.try_into().unwrap()).unwrap();
    let feed = VerifiedFeed::verify(fixture["feed"].to_string().as_bytes(), &key, 0).unwrap();
    assert_eq!(feed.sequence(), 17);
    let contracts = [("draft".to_owned(), 1), ("labels".to_owned(), 1)].into();
    let result = feed.select(
        Component::Dumbissues,
        &Version::new(0, 1, 0),
        Target::WindowsX86_64,
        Installed {
            enabled: true,
            version: None,
            supported_contracts: &contracts,
        },
    );
    let Availability::Update(release) = result else {
        panic!("expected update")
    };
    assert_eq!(release.verify_artifact(BUNDLE), Ok(()));
}
