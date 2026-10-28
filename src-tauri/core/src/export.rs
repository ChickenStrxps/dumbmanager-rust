//! Card export pipeline with a clear renderer boundary.
//!
//! A [`CardRenderer`] writes one new card file into a folder it is handed. In
//! the desktop app that is a thin adapter over the parent's
//! `dumbmanager-card-renderer` crate (`src-tauri/src/card.rs`). Everything
//! around that call is decided here and tested headless: input validation,
//! the safe exports folder, a cap on simultaneous exports and checks on what
//! the renderer reports. A reported file must be a plain file directly inside
//! the exports folder, carry the right extension and signature, and stay
//! within size, dimension and frame limits. Otherwise it is deleted (only
//! ever inside the exports folder) and the export fails. Without a renderer
//! the export is explicitly unavailable and nothing is written. It never
//! fakes success.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

use crate::error::{CoreError, CoreResult};
use crate::model::Itch;
use crate::paths::AppPaths;
use crate::settings::Theme;
use crate::store::Store;

pub const CARD_RENDERER_UNAVAILABLE: &str =
    "Card export is unavailable in this build: the card renderer has not been integrated yet. Nothing was exported.";

/// Upper bounds for renderer output, so a faulty renderer cannot fill the disk.
pub const MAX_CARD_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_CARD_DIMENSION: u32 = 8_192;
pub const MAX_CARD_FRAMES: u32 = 10_000;
/// Exports an [`ExportGate`] lets run at once (a GIF and a PNG side by side).
pub const MAX_PARALLEL_EXPORTS: usize = 2;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CardFormat {
    Gif,
    Png,
}

impl CardFormat {
    pub fn parse(raw: &str) -> CoreResult<Self> {
        match raw.trim() {
            "gif" => Ok(Self::Gif),
            "png" => Ok(Self::Png),
            _ => Err(CoreError::invalid("Card format must be gif or png.")),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Png => "png",
        }
    }

    fn matches_signature(self, head: &[u8]) -> bool {
        match self {
            Self::Gif => head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a"),
            Self::Png => head.starts_with(PNG_SIGNATURE),
        }
    }
}

/// Everything a renderer gets to draw one card.
#[derive(Debug, Clone, Copy)]
pub struct CardRequest<'a> {
    pub itch: &'a Itch,
    pub format: CardFormat,
    pub theme: Theme,
}

/// What a renderer reports after writing a card file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
}

/// The integration point for a card renderer.
pub trait CardRenderer: Send + Sync {
    /// Writes one new card file directly inside `directory` (which already
    /// exists), never replacing an existing file, and describes it. On error,
    /// leave no partial file behind; the message is shown to the user.
    fn render_into(
        &self,
        request: CardRequest<'_>,
        directory: &Path,
    ) -> Result<RenderedFile, String>;
}

/// `card_export` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardExport {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub bytes: u64,
}

/// A validated export request, copied out of the database so the slow
/// rendering step does not need the store.
#[derive(Debug, Clone)]
pub struct ExportJob {
    itch: Itch,
    format: CardFormat,
    theme: Theme,
}

impl ExportJob {
    /// Checks the format and loads the itch. Both problems are reported
    /// before "unavailable", so the user gets the most useful error first.
    pub fn prepare(store: &Store, id: &str, format: &str) -> CoreResult<Self> {
        let format = CardFormat::parse(format)?;
        let itch = store.get_itch(id)?;
        let theme = store.settings()?.theme;
        Ok(Self {
            itch,
            format,
            theme,
        })
    }

    pub fn itch(&self) -> &Itch {
        &self.itch
    }

    pub fn format(&self) -> CardFormat {
        self.format
    }

    /// Renders into the exports folder and verifies the result. Nothing
    /// touches the disk when there is no renderer.
    pub fn run(
        &self,
        paths: &AppPaths,
        renderer: Option<&dyn CardRenderer>,
    ) -> CoreResult<CardExport> {
        let Some(renderer) = renderer else {
            return Err(CoreError::Unavailable(CARD_RENDERER_UNAVAILABLE.to_owned()));
        };
        let dir = paths.ensure_exports_dir()?;
        let request = CardRequest {
            itch: &self.itch,
            format: self.format,
            theme: self.theme,
        };
        let rendered = renderer.render_into(request, &dir).map_err(|message| {
            CoreError::Render(format!(
                "The card renderer failed: {}. Nothing was exported.",
                message.trim().trim_end_matches('.')
            ))
        })?;
        verify_rendered(&dir, &rendered, self.format)
    }
}

/// Prepares and runs one export (convenience for callers holding the store).
pub fn export_card(
    store: &Store,
    paths: &AppPaths,
    renderer: Option<&dyn CardRenderer>,
    id: &str,
    format: &str,
) -> CoreResult<CardExport> {
    ExportJob::prepare(store, id, format)?.run(paths, renderer)
}

fn verify_rendered(
    dir: &Path,
    rendered: &RenderedFile,
    format: CardFormat,
) -> CoreResult<CardExport> {
    let unusable = |why: &str| {
        CoreError::Render(format!(
            "The card renderer returned an unusable file: {why}. Nothing was exported."
        ))
    };

    // Only a plain file directly inside the exports folder counts. Anything
    // else is reported but never touched.
    let Some(name) = rendered.path.file_name() else {
        return Err(unusable("it has no file name"));
    };
    let inside = match (
        rendered.path.parent().map(fs::canonicalize),
        fs::canonicalize(dir),
    ) {
        (Some(Ok(parent)), Ok(dir)) => parent == dir,
        _ => false,
    };
    if !inside {
        return Err(unusable("it is not inside the exports folder"));
    }
    let path = dir.join(name);
    let meta =
        fs::symlink_metadata(&path).map_err(|_| unusable("the reported file does not exist"))?;
    if !meta.file_type().is_file() {
        return Err(unusable("it is not a plain file"));
    }

    // From here on the file is ours: delete it if it fails a check.
    let reject = |why: &str| {
        let _ = fs::remove_file(&path);
        Err(unusable(why))
    };
    let extension_ok = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension()));
    if !extension_ok {
        return reject("its file extension does not match the requested format");
    }
    let bytes = meta.len();
    if bytes == 0 {
        return reject("it is empty");
    }
    if bytes > MAX_CARD_BYTES {
        return reject("it is larger than 64 MiB");
    }
    let dimension_ok = |value: u32| (1..=MAX_CARD_DIMENSION).contains(&value);
    if !dimension_ok(rendered.width) || !dimension_ok(rendered.height) {
        return reject("its dimensions are out of range");
    }
    let frames_ok = match format {
        CardFormat::Png => rendered.frames == 1,
        CardFormat::Gif => (1..=MAX_CARD_FRAMES).contains(&rendered.frames),
    };
    if !frames_ok {
        return reject("its frame count is out of range");
    }
    let mut head = Vec::with_capacity(8);
    let read = File::open(&path).and_then(|file| file.take(8).read_to_end(&mut head));
    if read.is_err() || !format.matches_signature(&head) {
        return reject("its contents do not match the requested format");
    }

    Ok(CardExport {
        path: path.display().to_string(),
        width: rendered.width,
        height: rendered.height,
        frames: rendered.frames,
        bytes,
    })
}

/// Caps how many exports run at once. Extra requests get a readable
/// [`CoreError::Busy`] instead of piling up rendering work.
#[derive(Debug, Default)]
pub struct ExportGate {
    running: AtomicUsize,
}

/// Holds one export slot; dropping it frees the slot.
#[derive(Debug)]
pub struct ExportPermit<'a> {
    gate: &'a ExportGate,
}

impl ExportGate {
    pub const fn new() -> Self {
        Self {
            running: AtomicUsize::new(0),
        }
    }

    pub fn try_enter(&self) -> CoreResult<ExportPermit<'_>> {
        self.running
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_PARALLEL_EXPORTS).then_some(n + 1)
            })
            .map(|_| ExportPermit { gate: self })
            .map_err(|_| {
                CoreError::Busy(format!(
                    "{MAX_PARALLEL_EXPORTS} card exports are already running. Try again when one finishes."
                ))
            })
    }

    pub fn running(&self) -> usize {
        self.running.load(Ordering::Acquire)
    }
}

impl Drop for ExportPermit<'_> {
    fn drop(&mut self) {
        self.gate.running.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Helper for renderers that produce bytes in memory: writes `bytes` to
/// `<dir>/<stem>.<ext>` (or `<stem>-2.<ext>`, ...) without ever replacing an
/// existing file. The data goes to a hidden temporary file in the same folder
/// first, is flushed to disk, then linked into place, so a crash never
/// leaves a half-written card under the final name.
pub fn write_new_file(
    dir: &Path,
    stem: &str,
    extension: &str,
    bytes: &[u8],
) -> CoreResult<PathBuf> {
    let safe = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if !safe(stem) || !safe(extension) {
        return Err(CoreError::invalid(
            "Export file names may only use letters, digits, '-' and '_'.",
        ));
    }
    let temp = dir.join(format!(".{stem}.{}.partial", uuid::Uuid::new_v4().simple()));
    let written = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    for attempt in 1..=100u32 {
        let name = if attempt == 1 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{attempt}.{extension}")
        };
        let target = dir.join(name);
        // hard_link creates the destination atomically and never replaces
        // an existing name (unlike rename on Unix). The temp is on the same
        // filesystem; concurrent writers can safely retry the next suffix.
        match fs::hard_link(&temp, &target) {
            Ok(()) => {
                let _ = fs::remove_file(&temp);
                return Ok(target);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error.into());
            }
        }
    }
    let _ = fs::remove_file(&temp);
    Err(CoreError::Io(
        "Could not find a free export file name.".to_owned(),
    ))
}
