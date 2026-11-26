//! Authenticate component feed data and select compatible, non-downgrade updates.
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use url::Url;

pub const SIGNING_CONTEXT: &[u8] = b"DumbManager component feed v1\0";
const MAX_FEED_BYTES: usize = 256 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FeedError {
    #[error("component feed is too large")]
    TooLarge,
    #[error("component feed format is invalid")]
    Format,
    #[error("component feed signature did not verify")]
    Signature,
    #[error("component feed is older than the last verified feed")]
    Stale,
    #[error("component artifact did not match its signed size and hash")]
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Component {
    Dumbissues,
    DumbmcpHost,
    Dumbmods,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Any,
    #[serde(rename = "windows-x86-64")]
    WindowsX86_64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: String,
    signature: String,
}

#[derive(Debug)]
pub struct VerifiedFeed {
    sequence: u64,
    releases: Vec<Release>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedPayload {
    schema: u32,
    sequence: u64,
    releases: Vec<Release>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    component: Component,
    version: Version,
    app: VersionReq,
    contracts: BTreeMap<String, u32>,
    target: Target,
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Availability<'a> {
    Disabled,
    Unavailable,
    Incompatible,
    Current,
    Update(&'a Release),
}

#[derive(Debug)]
pub struct Installed<'a> {
    pub enabled: bool,
    pub version: Option<&'a Version>,
    pub supported_contracts: &'a BTreeMap<String, u32>,
}

fn stable(version: &Version) -> bool {
    version.pre.is_empty() && version.build.is_empty()
}

impl VerifiedFeed {
    pub fn verify(
        bytes: &[u8],
        key: &VerifyingKey,
        minimum_sequence: u64,
    ) -> Result<Self, FeedError> {
        if bytes.len() > MAX_FEED_BYTES {
            return Err(FeedError::TooLarge);
        }
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| FeedError::Format)?;
        let payload = STANDARD
            .decode(envelope.payload)
            .map_err(|_| FeedError::Format)?;
        let signature = STANDARD
            .decode(envelope.signature)
            .map_err(|_| FeedError::Signature)?;
        let signature = Signature::from_slice(&signature).map_err(|_| FeedError::Signature)?;
        let message = [SIGNING_CONTEXT, &payload].concat();
        key.verify_strict(&message, &signature)
            .map_err(|_| FeedError::Signature)?;

        let feed: FeedPayload = serde_json::from_slice(&payload).map_err(|_| FeedError::Format)?;
        if feed.schema != 1 || feed.sequence == 0 {
            return Err(FeedError::Format);
        }
        if feed.sequence < minimum_sequence {
            return Err(FeedError::Stale);
        }
        let mut seen = BTreeSet::new();
        for release in &feed.releases {
            if !release.is_valid()
                || !seen.insert((release.component, &release.version, release.target))
            {
                return Err(FeedError::Format);
            }
        }
        Ok(Self {
            sequence: feed.sequence,
            releases: feed.releases,
        })
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn select(
        &self,
        component: Component,
        app: &Version,
        target: Target,
        installed: Installed<'_>,
    ) -> Availability<'_> {
        if !installed.enabled {
            return Availability::Disabled;
        }
        let mut found = false;
        let newest = self
            .releases
            .iter()
            .filter(|release| {
                if release.component != component {
                    return false;
                }
                found = true;
                (release.target == target || release.target == Target::Any)
                    && release.app.matches(app)
                    && release.contracts == *installed.supported_contracts
            })
            .max_by_key(|release| (&release.version, release.target != Target::Any));

        match newest {
            Some(release)
                if installed
                    .version
                    .is_some_and(|version| version >= &release.version) =>
            {
                Availability::Current
            }
            Some(release) => Availability::Update(release),
            None if found => Availability::Incompatible,
            None => Availability::Unavailable,
        }
    }
}

impl Release {
    fn is_valid(&self) -> bool {
        let Ok(url) = Url::parse(&self.url) else {
            return false;
        };
        stable(&self.version)
            && !self.contracts.is_empty()
            && self.contracts.iter().all(|(name, major)| {
                !name.is_empty()
                    && name.len() <= 64
                    && *major > 0
                    && name.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            })
            && (1..=MAX_ARTIFACT_BYTES).contains(&self.size)
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn verify_artifact(&self, bytes: &[u8]) -> Result<(), FeedError> {
        if bytes.len() as u64 != self.size || format!("{:x}", Sha256::digest(bytes)) != self.sha256
        {
            return Err(FeedError::Artifact);
        }
        Ok(())
    }
}
