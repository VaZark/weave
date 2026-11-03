#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid filter: {0}")]
    InvalidFilter(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn db_err(e: rusqlite::Error) -> Error {
    Error::Storage(e.to_string())
}
