//! Published HTML stays on a separate, sandboxed origin without native IPC.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_HTML: usize = 2 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(600);
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' https:; style-src 'unsafe-inline' https:; img-src data: blob: https:; font-src data: https:; media-src data: blob: https:; connect-src 'none'; worker-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; sandbox allow-scripts";

#[derive(Default)]
pub struct PreviewCache(Mutex<VecDeque<(String, Instant, String)>>);
impl PreviewCache {
    pub fn insert(&self, html: String) -> LegacyResult<String> {
        if html.is_empty() || html.len() > MAX_HTML {
            return Err(LegacyError::invalid(
                "That published preview is empty or too large.",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut rows = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rows.retain(|(_, at, _)| at.elapsed() < TTL);
        while rows.len() >= 4 {
            rows.pop_front();
        }
        rows.push_back((id.clone(), Instant::now(), html));
        #[cfg(windows)]
        return Ok(format!("http://dm-preview.localhost/{id}"));
        #[cfg(not(windows))]
        Ok(format!("dm-preview://localhost/{id}"))
    }
    pub fn get(&self, path: &str) -> Option<String> {
        let id = path.strip_prefix('/')?;
        if uuid::Uuid::parse_str(id).is_err() {
            return None;
        }
        let mut rows = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rows.retain(|(_, at, _)| at.elapsed() < TTL);
        rows.iter()
            .find(|(key, _, _)| key == id)
            .map(|(_, _, html)| html.clone())
    }
}
