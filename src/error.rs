use thiserror::Error;

#[derive(Debug, Error)]
pub enum CxError {
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Permission denied: {0}")]
    Permission(String),

    #[error("API request failed ({status}): {message}")]
    Api { status: u16, message: String },

    /// A request the CLI refuses to send because its arguments would make
    /// the API do something other than what the user asked for.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("HTTP request timed out")]
    Timeout,

    #[error("HTTP error: {0}")]
    Http(reqwest::Error),

    #[error("HTTP middleware error: {0}")]
    HttpMiddleware(#[source] anyhow::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Query stream error: {0}")]
    QueryStream(String),
}

pub type Result<T> = std::result::Result<T, CxError>;

impl From<reqwest::Error> for CxError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else {
            Self::Http(error)
        }
    }
}

impl From<reqwest_middleware::Error> for CxError {
    fn from(error: reqwest_middleware::Error) -> Self {
        match error {
            reqwest_middleware::Error::Reqwest(error) => error.into(),
            reqwest_middleware::Error::Middleware(error) => Self::HttpMiddleware(error),
        }
    }
}
