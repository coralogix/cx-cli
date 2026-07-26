use thiserror::Error;

#[derive(Debug, Error)]
pub enum CxError {
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Permission denied: {0}")]
    Permission(String),

    #[error("API request failed ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Query stream error: {0}")]
    QueryStream(String),
}

pub type Result<T> = std::result::Result<T, CxError>;
