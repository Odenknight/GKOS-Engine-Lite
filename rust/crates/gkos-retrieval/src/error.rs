use std::fmt::{Display, Formatter};

pub type RetrievalResult<T> = Result<T, RetrievalError>;

#[derive(Debug)]
pub enum RetrievalError {
    ContractMismatch { expected: String, actual: String },
    InvalidConfig(String),
    InvalidEnvelope(String),
    NotDiscoverable(String),
    NonFiniteScore(String),
    ProjectionMismatch(String),
    MissingChunk(String),
    ProviderResponse(String),
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Serialization(serde_json::Error),
}

impl Display for RetrievalError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContractMismatch { expected, actual } => {
                write!(
                    formatter,
                    "contract mismatch: expected {expected}, received {actual}"
                )
            }
            Self::InvalidConfig(message) => {
                write!(formatter, "invalid retrieval config: {message}")
            }
            Self::InvalidEnvelope(message) => {
                write!(formatter, "invalid source envelope: {message}")
            }
            Self::NotDiscoverable(decision) => {
                write!(formatter, "source is not discoverable: {decision}")
            }
            Self::NonFiniteScore(stage) => write!(formatter, "non-finite score in {stage}"),
            Self::ProjectionMismatch(message) => {
                write!(formatter, "projection mismatch: {message}")
            }
            Self::MissingChunk(chunk_id) => {
                write!(formatter, "missing retrieval chunk: {chunk_id}")
            }
            Self::ProviderResponse(message) => {
                write!(formatter, "invalid provider response: {message}")
            }
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Sqlite(error) => Display::fmt(error, formatter),
            Self::Serialization(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RetrievalError {}

impl From<rusqlite::Error> for RetrievalError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<std::io::Error> for RetrievalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RetrievalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}
