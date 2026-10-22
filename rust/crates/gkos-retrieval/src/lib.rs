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
//!
//! Full-produced Phase-3 owner envelopes are verified only behind the
//! crate-private trusted-host seam. Full remains the sole filesystem staging
//! and activation authority:
//!
//! ```compile_fail
//! use gkos_retrieval_lite::ingest::verify_ingest_owner_bundle;
//! ```
//!
//! Full-produced Phase-4 normalized evaluation envelopes are verified only by
//! a crate-private pure seam. Raw golden parsing, fixture/provider execution,
//! search, tuning, and publication remain Full-only authorities:
//!
//! ```compile_fail
//! use gkos_retrieval_lite::evaluation::compute_query_metrics;
//! ```

#[cfg_attr(not(test), allow(dead_code))]
mod authorized_view;
#[cfg_attr(not(test), allow(dead_code))]
mod candidate;
// Phase-4 consumes only Full-produced normalized evaluation envelopes.
pub mod chunker;
pub mod confidence;
pub mod config;
pub mod contract;
pub mod coordinator;
pub mod digest;
pub mod error;
#[cfg_attr(not(test), allow(dead_code))]
mod evaluation;
pub mod filters;
pub mod fusion;
#[cfg_attr(not(test), allow(dead_code))]
mod ingest;
// Schema-3 construction remains a sealed trusted-host boundary. The frozen
// Full-owned draft.2 pack is consumed as data, without exposing a second GKX
// parser, resolver, or identity authority through the public library surface.
#[cfg_attr(not(test), allow(dead_code))]
#[path = "candidate_store.rs"]
mod lineage_store;
pub mod parent;
mod path_security;
#[cfg_attr(not(test), allow(dead_code))]
mod provenance;
pub mod providers;
mod redaction;
mod sqlite_store;
#[cfg_attr(not(test), allow(dead_code))]
mod temporal_coordinator;
#[cfg_attr(not(test), allow(dead_code))]
mod writer_lock;

pub use error::{RetrievalError, RetrievalResult};
pub use provenance::{normalize_retrieval_as_of, GkxPublicProvenance};
pub use redaction::{
    AuthorizedRetrievalHit, AuthorizedRetrievalSearchResult, AuthorizedRetrievalSearchStages,
};
pub use sqlite_store::{
    activate_retrieval_generation, build_retrieval_generation, lexical_field_score,
    normalized_lexical_terms, recover_stale_retrieval_writer, BuiltRetrievalGeneration,
    RetrievalGenerationInput, StoredVector,
};
pub use temporal_coordinator::{
    GkxAuthorizedRetrievalSearchResult, GkxProjectionFreshness, GkxRetrievalCoordinator,
    GkxRetrievalCoordinatorOptions, GkxRetrievalHit, GkxRetrievalParentContext,
    GkxSourceDiscoverabilityPolicy, GkxSourcePolicyRecord, GkxTemporalResultState,
};
