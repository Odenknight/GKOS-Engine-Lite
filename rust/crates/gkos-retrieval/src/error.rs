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

/// Convert only disposable cache corruption/version mismatches into a miss.
/// Filesystem authority, containment, permission, locking, and resource
/// failures must propagate before any source text crosses a provider boundary.
pub(crate) fn cache_value_or_miss<T>(result: RetrievalResult<T>) -> RetrievalResult<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(RetrievalError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(RetrievalError::ContractMismatch { .. })
        | Err(RetrievalError::InvalidEnvelope(_))
        | Err(RetrievalError::NonFiniteScore(_))
        | Err(RetrievalError::ProjectionMismatch(_))
        | Err(RetrievalError::MissingChunk(_))
        | Err(RetrievalError::Serialization(_)) => Ok(None),
        Err(RetrievalError::Sqlite(error)) if disposable_sqlite_cache_error(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn disposable_sqlite_cache_error(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => matches!(
            failure.code,
            rusqlite::ErrorCode::DatabaseCorrupt
                | rusqlite::ErrorCode::NotADatabase
                | rusqlite::ErrorCode::SchemaChanged
                | rusqlite::ErrorCode::TypeMismatch
                | rusqlite::ErrorCode::ConstraintViolation
        ),
        // These variants can be produced only after opening the disposable
        // generation and attempting to decode its frozen schema. They are
        // therefore evidence of persisted-row/schema incompatibility rather
        // than filesystem authority or runtime-resource failure.
        rusqlite::Error::FromSqlConversionFailure(..)
        | rusqlite::Error::IntegralValueOutOfRange(..)
        | rusqlite::Error::Utf8Error(..)
        | rusqlite::Error::QueryReturnedNoRows
        | rusqlite::Error::QueryReturnedMoreThanOneRow
        | rusqlite::Error::InvalidColumnIndex(..)
        | rusqlite::Error::InvalidColumnName(..)
        | rusqlite::Error::InvalidColumnType(..) => true,
        // Fail closed for every unclassified error. In particular,
        // InvalidPath and SqliteSingleThreadedMode are authority/runtime
        // failures and must stop before provider invocation.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_miss_classification_never_swallows_authority_or_permission_errors() {
        assert!(matches!(
            cache_value_or_miss::<()>(Err(RetrievalError::InvalidConfig(
                "filesystem alias".to_owned()
            ))),
            Err(RetrievalError::InvalidConfig(_))
        ));
        assert!(matches!(
            cache_value_or_miss::<()>(Err(RetrievalError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "denied",
            )))),
            Err(RetrievalError::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert!(
            cache_value_or_miss::<()>(Err(RetrievalError::ProjectionMismatch(
                "incompatible disposable cache".to_owned()
            )))
            .unwrap()
            .is_none()
        );
        assert!(cache_value_or_miss::<()>(Err(RetrievalError::Serialization(
            serde_json::from_str::<serde_json::Value>("{").unwrap_err()
        )))
        .unwrap()
        .is_none());
        assert!(matches!(
            cache_value_or_miss::<()>(Err(RetrievalError::Sqlite(rusqlite::Error::InvalidPath(
                std::path::PathBuf::from("invalid")
            ),))),
            Err(RetrievalError::Sqlite(rusqlite::Error::InvalidPath(_)))
        ));
        assert!(matches!(
            cache_value_or_miss::<()>(Err(RetrievalError::Sqlite(
                rusqlite::Error::SqliteSingleThreadedMode,
            ))),
            Err(RetrievalError::Sqlite(
                rusqlite::Error::SqliteSingleThreadedMode
            ))
        ));
        for code in [
            rusqlite::ErrorCode::Unknown,
            rusqlite::ErrorCode::TooBig,
            rusqlite::ErrorCode::NotFound,
        ] {
            assert!(matches!(
                cache_value_or_miss::<()>(Err(RetrievalError::Sqlite(
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error {
                            code,
                            extended_code: 0,
                        },
                        None,
                    ),
                ))),
                Err(RetrievalError::Sqlite(rusqlite::Error::SqliteFailure(_, _)))
            ));
        }
        assert!(cache_value_or_miss::<()>(Err(RetrievalError::Sqlite(
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error {
                    code: rusqlite::ErrorCode::DatabaseCorrupt,
                    extended_code: rusqlite::ffi::SQLITE_CORRUPT,
                },
                None,
            ),
        )))
        .unwrap()
        .is_none());
    }
}
