//! The one place a card renderer is plugged in.
//!
//! `dumbmanager-core` owns validation, the exports folder, the limit on
//! parallel exports and the checks on every written file (`ExportJob`). This
//! adapter only maps an itch onto the parent's `dumbmanager-card-renderer`
//! crate. To ship a build without card export, make [`renderer`] return
//! `None`: `card_export` then rejects with an explicit "unavailable" message
//! and writes nothing.

use std::path::Path;

use dumbmanager_card_renderer as native;
use dumbmanager_core::{CardRenderer, CardRequest, RenderedFile};

/// Native GIF/PNG cards drawn in-process (no browser, network or external tool).
#[derive(Debug, Default)]
pub struct NativeCardRenderer;

impl CardRenderer for NativeCardRenderer {
    fn render_into(
        &self,
        request: CardRequest<'_>,
        directory: &Path,
    ) -> Result<RenderedFile, String> {
        let itch = request.itch;
        let input = native::CardInput {
            title: &itch.title,
            description: &itch.description,
            project: &itch.project,
            status: itch.status.as_str(),
            seed: &itch.id,
        };
        let written = native::export_card(directory, &input, request.format.extension())
            .map_err(|error| error.to_string())?;
        Ok(RenderedFile {
            frames: u32::try_from(written.frames)
                .map_err(|_| "frame count out of range".to_owned())?,
            path: written.path,
            width: written.width,
            height: written.height,
        })
    }
}

/// The renderer this build uses for `card_export`.
pub fn renderer() -> Option<Box<dyn CardRenderer>> {
    Some(Box::new(NativeCardRenderer))
}
