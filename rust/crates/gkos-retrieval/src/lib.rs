//! Pin-bound Rust implementation of the Full-owned GKOS retrieval contracts.
//!
//! This crate consumes canonical retrieval source envelopes. It does not parse
//! GKX frontmatter or decide identity, lineage, temporal validity, sensitivity,
//! discoverability, governance standing, or authority.
//!
//! Raw SQLite retrieval is intentionally not a public API. All external search
//! results pass through the policy-gated coordinator:
//!
//! ```compile_fail
//! use gkos_retrieval_lite::sqlite_store::SqliteRetrievalStore;
//! ```
//!
//! ```compile_fail
//! use gkos_retrieval_lite::open_active_retrieval_generation;
//! ```

pub mod chunker;
pub mod confidence;
pub mod config;
pub mod contract;
pub mod coordinator;
pub mod digest;
pub mod error;
pub mod filters;
pub mod fusion;
pub mod parent;
mod path_security;
pub mod providers;
mod redaction;
mod sqlite_store;

pub use error::{RetrievalError, RetrievalResult};
pub use redaction::{
    AuthorizedRetrievalHit, AuthorizedRetrievalSearchResult, AuthorizedRetrievalSearchStages,
};
pub use sqlite_store::{
    activate_retrieval_generation, build_retrieval_generation, lexical_field_score,
    normalized_lexical_terms, BuiltRetrievalGeneration, RetrievalGenerationInput, StoredVector,
};
