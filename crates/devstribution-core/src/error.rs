use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// Errors are written for the person (or coding agent) who has to fix them:
/// they say what is wrong and, where possible, what to change.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("pyproject.toml: {0}")]
    Manifest(String),

    #[error("uv.lock: {0}")]
    Lock(String),

    #[error("package refused: {0}")]
    Package(String),

    #[error("signature: {0}")]
    Signature(String),

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
