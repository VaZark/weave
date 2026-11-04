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
    #[error("player error: {0}")]
    Player(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<weaver::Error> for Error {
    fn from(e: weaver::Error) -> Self {
        match e {
            weaver::Error::InvalidFilter(m) => Self::InvalidFilter(m),
            weaver::Error::NotFound(m) => Self::NotFound(m),
            weaver::Error::AlreadyExists(m) => Self::AlreadyExists(m),
            weaver::Error::InvalidInput(m) => Self::InvalidInput(m),
            weaver::Error::Storage(m) => Self::Storage(m),
        }
    }
}
