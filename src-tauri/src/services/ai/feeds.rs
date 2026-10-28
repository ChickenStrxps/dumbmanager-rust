//! Fixed public inspiration sources. No caller-supplied fetch destination.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read, time::Duration};

pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub kind: &'static str,
    pub url: &'static str,
    pub home: &'static str,
    pub note: &'static str,
}
pub const SOURCES: &[Source] = &[
    Source {
        id: "typesafe",
        name: "TypeSafe",
        category: "models",
        kind: "blog",
        url: "https://typesafe.ai/blog",
        home: "https://typesafe.ai/blog",
        note: "System One news from TypeSafe.",
    },
    Source {
        id: "huggingface",
        name: "Hugging Face",
        category: "models",
        kind: "rss",
        url: "https://huggingface.co/blog/feed.xml",
        home: "https://huggingface.co/blog",
        note: "Open models and tooling; claims belong to the authors.",
    },
    Source {
        id: "openai",
        name: "OpenAI",
        category: "models",
        kind: "rss",
        url: "https://openai.com/news/rss.xml",
        home: "https://openai.com/news/",
        note: "Official release and research news.",
    },
    Source {
        id: "deepmind",
        name: "Google DeepMind",
        category: "science",
        kind: "rss",
        url: "https://deepmind.google/blog/rss.xml",
        home: "https://deepmind.google/blog/",
        note: "Official AI and science research reports.",
    },
    Source {
        id: "freecad",
        name: "FreeCAD",
        category: "cad",
        kind: "rss",
        url: "https://blog.freecad.org/feed/",
        home: "https://blog.freecad.org/",
        note: "CAD project examples and releases.",
    },
    Source {
        id: "hackaday",
        name: "Hackaday · CAD",
        category: "cad",
        kind: "rss",
        url: "https://hackaday.com/tag/cad/feed/",
        home: "https://hackaday.com/tag/cad/",
        note: "Editorial coverage of maker projects.",
    },
    Source {
        id: "mit-health",
        name: "MIT · Health",
        category: "science",
        kind: "rss",
        url: "https://news.mit.edu/rss/topic/health",
        home: "https://news.mit.edu/topic/health",
        note: "Research reports; not evidence of clinical availability.",
    },
];
pub fn known(id: &str) -> bool {
    ["manual", "show-hn", "docs"].contains(&id) || SOURCES.iter().any(|s| s.id == id)
}
fn plain(value: &str, limit: usize) -> String {
    let document = Html::parse_fragment(value);
    document
        .root_element()
        .descendants()
        .filter_map(|node| {
            let scraper::Node::Text(text) = node.value() else {
                return None;
            };
            if node.ancestors().any(|n| {
                n.value()
                    .as_element()
                    .is_some_and(|e| matches!(e.name(), "script" | "style" | "noscript"))
            }) {
                None
            } else {
                Some(text.text.as_ref())
            }
        })
        .collect::<Vec<&str>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}
fn address(base: &str, raw: &str) -> Option<String> {
    let url = url::Url::parse(base).ok()?.join(raw).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().len() <= 1500)
        .then(|| url.to_string())
}
fn row(source: &Source, title: String, url: String, excerpt: String, published: Value) -> Value {
    json!({"id":format!("{}-{:x}",source.id,Sha256::digest(url.as_bytes())),"title":title,"url":url,"excerpt":excerpt,"category":source.category,"source_id":source.id,"source_name":source.name,"provenance":source.note,"published_at":published,"fetched_at":dumbmanager_core::time::now_iso(),"first_seen_at":dumbmanager_core::time::now_iso(),"saved":false,"hidden":false,"itches":{}})
}
pub fn parse(source: &Source, bytes: &[u8]) -> LegacyResult<Vec<Value>> {
    if bytes.len() > 900_000 {
        return Err(LegacyError::unavailable(
            "Source response exceeded its size limit.",
        ));
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    if source.kind == "blog" {
        let html = std::str::from_utf8(bytes)
            .map_err(|_| LegacyError::unavailable("Source text encoding is unsupported."))?;
        let document = Html::parse_document(html);
        let anchors = Selector::parse("a[href]").expect("fixed selector");
        let headings = Selector::parse("h1,h2,h3,h4").expect("fixed selector");
        for anchor in document.select(&anchors) {
            let Some(url) = address(source.home, anchor.value().attr("href").unwrap_or("")) else {
                continue;
            };
            let parsed =
                url::Url::parse(&url).map_err(|_| LegacyError::invalid("Invalid source link."))?;
            if parsed.host_str() != Some("typesafe.ai")
                || !parsed.path().starts_with("/blog/")
                || parsed.path().trim_end_matches('/') == "/blog"
                || !seen.insert(url.clone())
            {
                continue;
            }
            let title = plain(
                &anchor
                    .select(&headings)
                    .next()
                    .map(|h| h.inner_html())
                    .unwrap_or_else(|| anchor.inner_html()),
                240,
            );
            if !title.is_empty() {
                rows.push(row(source, title, url, String::new(), Value::Null));
            }
            if rows.len() == 18 {
                break;
            }
        }
        if rows.is_empty() {
            return Err(LegacyError::unavailable(
                "The blog layout supplied no readable article links.",
            ));
        }
    } else {
        let feed = feed_rs::parser::parse(bytes)
            .map_err(|_| LegacyError::unavailable("Source feed could not be parsed."))?;
        for entry in feed.entries.into_iter().take(72) {
            let title = entry
                .title
                .as_ref()
                .map(|t| plain(&t.content, 240))
                .unwrap_or_default();
            let Some(url) = entry
                .links
                .iter()
                .filter(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
                .find_map(|l| address(source.home, &l.href))
            else {
                continue;
            };
            if title.is_empty() || !seen.insert(url.clone()) {
                continue;
            }
            let excerpt = entry
                .summary
                .as_ref()
                .map(|t| t.content.as_str())
                .or_else(|| entry.content.as_ref().and_then(|c| c.body.as_deref()))
                .map(|s| plain(s, 300))
                .unwrap_or_default();
            rows.push(row(
                source,
                title,
                url,
                excerpt,
                entry
                    .published
                    .or(entry.updated)
                    .map(|d| json!(d.to_rfc3339()))
                    .unwrap_or(Value::Null),
            ));
            if rows.len() == 18 {
                break;
            }
        }
    }
    Ok(rows)
}
pub fn fetch(source: &Source) -> LegacyResult<Vec<Value>> {
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| LegacyError::unavailable("Source unavailable."))?
        .get(source.url)
        .header("User-Agent", "DumbManager-Rust/0.1")
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| {
            LegacyError::unavailable("Source unavailable. No saved items were changed.")
        })?;
    let mut bytes = Vec::new();
    response
        .take(900_001)
        .read_to_end(&mut bytes)
        .map_err(|_| LegacyError::unavailable("Source read failed."))?;
    parse(source, &bytes)
}
