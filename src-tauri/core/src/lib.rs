//! Headless domain logic for DumbManager Rust.
//!
//! Nothing in this crate depends on Tauri, a window or a web view, so the
//! whole itch/settings/export core runs under plain `cargo test` against
//! temporary directories. The desktop shell in `src-tauri` is a thin adapter
//! over [`Store`], [`AppPaths`] and [`ExportJob`].
//!
//! Isolation rules enforced here (see `docs/backend-notes.md`):
//! - data lives only under the directory handed to [`AppPaths::new`];
//! - a database that belongs to another application is refused, untouched;
//! - nothing in this crate opens sockets, spawns processes or reads tokens.

pub mod error;
pub mod export;
pub mod integrations;
pub mod legacy;
pub mod model;
pub mod paths;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod time;
pub mod validate;

pub use error::{CoreError, CoreResult};
pub use export::{
    export_card, CardExport, CardFormat, CardRenderer, CardRequest, ExportGate, ExportJob,
    ExportPermit, RenderedFile, CARD_RENDERER_UNAVAILABLE, MAX_PARALLEL_EXPORTS,
};
pub use integrations::{integration_statuses, IntegrationState, IntegrationStatus};
pub use model::{check_transition, Itch, ItchEvent, ItchInput, ItchKind, ItchStatus, ListFilter};
pub use paths::AppPaths;
pub use settings::{Motion, Settings, SettingsInput, SettingsView, Theme};
pub use snapshot::{AppSnapshot, StatusCounts};
pub use store::Store;
