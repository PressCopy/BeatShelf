use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("trash error: {0}")]
    Trash(String),
    #[error("audio error: {0}")]
    Audio(String),
    #[error("copy verification failed for {0}")]
    VerifyFailed(PathBuf),
    #[error("{0}")]
    Msg(String),
}

impl From<trash::Error> for Error {
    fn from(e: trash::Error) -> Self {
        Error::Trash(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
