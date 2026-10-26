//! One error type for the whole core. Every variant carries a message that is
//! safe to show to the user as-is (no secrets, no file contents).

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    /// Input failed validation. Nothing was written.
    #[error("{0}")]
    Invalid(String),
    /// The requested record does not exist.
    #[error("{0}")]
    NotFound(String),
    /// A real feature this build cannot perform yet. Never a fake success.
    #[error("{0}")]
    Unavailable(String),
    /// The database file belongs to something else and was left untouched.
    #[error("{0}")]
    ForeignDatabase(String),
    /// Temporarily refused (for example, too many exports at once). Try again.
    #[error("{0}")]
    Busy(String),
    /// The card renderer failed or produced an unusable file. Nothing was kept.
    #[error("{0}")]
    Render(String),
    /// SQLite reported a failure. The transaction was rolled back.
    #[error("Local storage error: {0}")]
    Storage(String),
    /// A filesystem operation failed.
    #[error("File error: {0}")]
    Io(String),
}

impl CoreError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Short machine-readable category, useful for logs and tests.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::NotFound(_) => "not_found",
            Self::Unavailable(_) => "unavailable",
            Self::ForeignDatabase(_) => "foreign_database",
            Self::Busy(_) => "busy",
            Self::Render(_) => "render",
            Self::Storage(_) => "storage",
            Self::Io(_) => "io",
        }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::Busy(
                    "Local storage is busy with another operation. Please try again.".to_owned(),
                )
            }
            _ => Self::Storage(error.to_string()),
        }
    }
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        // io::Error's message never includes the path, so no profile names leak.
        Self::Io(error.to_string())
    }
}
