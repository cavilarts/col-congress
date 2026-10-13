use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SourceError {
    #[error("network failure")]
    Network,
    #[error("unexpected HTTP status {0}")]
    UnexpectedStatus(u16),
    #[error("unexpected structure: {0}")]
    Parse(String),
    #[error("source returned no valid records")]
    Empty,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("storage failure: {0}")]
pub struct RepositoryError(pub String);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SyncError {
    #[error("all sources failed")]
    AllSourcesFailed,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ListError {
    #[error("invalid term")]
    InvalidTerm,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}
