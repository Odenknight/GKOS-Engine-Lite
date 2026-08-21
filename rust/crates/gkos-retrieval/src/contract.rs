use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{RetrievalError, RetrievalResult};

pub const RETRIEVAL_CONTRACT: &str = "gkos-retrieval/1.0.0-draft.1";
pub const RETRIEVAL_RESULT_SCHEMA: &str = "gkos-retrieval-result/1.0.0-draft.1";
pub const CHUNKER_VERSION: &str = "gkos-heading-chunker/1";
pub const TOKENIZER_VERSION: &str = "gkos-ascii-whitespace/1";
pub const PROJECTION_SCHEMA_VERSION: u32 = 2;
pub const RRF_DEFAULT_K: u64 = 60;
pub const MMR_DEFAULT_LAMBDA: f64 = 0.7;
pub const MAX_CHUNK_BYTES: usize = 16_384;
pub const MAX_RESULT_BYTES: usize = 131_072;
pub const PARENT_EXPANSION_MAX_CHILD_TOKENS: u32 = 80;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverabilityDecision {
    Allow,
    Deny,
    Indeterminate,
    Error,
}

impl DiscoverabilityDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Indeterminate => "indeterminate",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GkxSensitivity {
    Public,
    Internal,
    Restricted,
    Confidential,
    Regulated,
    Phi,
    Secret,
}

impl GkxSensitivity {
    pub fn rank(self) -> u8 {
        match self {
            Self::Public => 0,
            Self::Internal => 1,
            Self::Restricted => 2,
            Self::Confidential => 3,
            Self::Regulated => 4,
            Self::Phi => 5,
            Self::Secret => 6,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalLineageEnvelope {
    pub lineage_id: Option<String>,
    pub lineage_neutral: bool,
    pub supersedes: Vec<String>,
    pub superseded_by: Vec<String>,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalTemporalEnvelope {
    pub assertion_time: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub valid_from_unix_ms: Option<i64>,
    pub valid_to_unix_ms: Option<i64>,
}

/// Canonical, policy-evaluated input supplied by the GKX host boundary.
///
/// Lite consumes this envelope. It never parses frontmatter or assigns any of
/// its identity, lineage, temporal, sensitivity, policy, or authority fields.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalSource {
    pub contract_version: String,
    pub vault_id: String,
    pub source_id: String,
    pub source_path: String,
    pub source_digest: String,
    pub text: String,
    pub discoverability: DiscoverabilityDecision,
    pub lineage: CanonicalLineageEnvelope,
    pub temporal: CanonicalTemporalEnvelope,
    pub metadata: RetrievalChunkMetadata,
}

impl RetrievalSource {
    pub fn validate_binding(&self) -> RetrievalResult<()> {
        if self.contract_version != RETRIEVAL_CONTRACT {
            return Err(RetrievalError::ContractMismatch {
                expected: RETRIEVAL_CONTRACT.to_owned(),
                actual: self.contract_version.clone(),
            });
        }
        for (field, value) in [
            ("vault_id", self.vault_id.as_str()),
            ("source_id", self.source_id.as_str()),
            ("source_path", self.source_path.as_str()),
            ("source_digest", self.source_digest.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(RetrievalError::InvalidEnvelope(format!(
                    "{field} must not be empty"
                )));
            }
        }
        if !is_valid_authored_uid(&self.source_id) {
            return Err(RetrievalError::InvalidEnvelope(
                "source_id must be a valid canonical GKX authored UUID".to_owned(),
            ));
        }
        if !is_normalized_relative_path(&self.source_path) {
            return Err(RetrievalError::InvalidEnvelope(
                "source_path must be a normalized, contained, slash-separated relative path"
                    .to_owned(),
            ));
        }
        if !is_sha256_digest(&self.source_digest) {
            return Err(RetrievalError::InvalidEnvelope(
                "source_digest must be a SHA-256 digest".to_owned(),
            ));
        }
        if self.source_digest != crate::digest::sha256(self.text.as_bytes()) {
            return Err(RetrievalError::InvalidEnvelope(
                "source_digest does not bind the exact UTF-8 source bytes".to_owned(),
            ));
        }
        if let Some(quality) = self.metadata.quality {
            if !quality.is_finite() || !(0.0..=1.0).contains(&quality) {
                return Err(RetrievalError::InvalidEnvelope(
                    "quality must be finite and between zero and one".to_owned(),
                ));
            }
        }
        if let (Some(valid_from), Some(valid_to)) = (
            self.temporal.valid_from_unix_ms,
            self.temporal.valid_to_unix_ms,
        ) {
            if valid_from >= valid_to {
                return Err(RetrievalError::InvalidEnvelope(
                    "canonical validity interval must be non-empty and half-open".to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub fn validate_for_retrieval(&self) -> RetrievalResult<()> {
        self.validate_binding()?;
        if self.discoverability != DiscoverabilityDecision::Allow {
            return Err(RetrievalError::NotDiscoverable(
                self.discoverability.as_str().to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RetrievalChunkMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<GkxSensitivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gkx_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epistemic_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoritative: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moc_relationships: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalChunk {
    pub chunk_id: String,
    pub source_id: String,
    pub source_path: String,
    pub source_digest: String,
    pub heading_path: Vec<String>,
    pub heading_depth: u8,
    pub ordinal_within_source: u32,
    pub structural_position: String,
    pub part_ordinal: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
    pub content_digest: String,
    pub text: String,
    pub token_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_chunk_id: Option<String>,
    pub lineage_id: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub supersedes: Vec<String>,
    pub superseded_by: Vec<String>,
    pub metadata: RetrievalChunkMetadata,
}

impl RetrievalChunk {
    pub fn validate_against(&self, source: &RetrievalSource) -> RetrievalResult<()> {
        source.validate_for_retrieval()?;
        if self.source_id != source.source_id
            || self.source_path != source.source_path
            || self.source_digest != source.source_digest
        {
            return Err(RetrievalError::InvalidEnvelope(
                "chunk source binding differs from canonical source envelope".to_owned(),
            ));
        }
        let start = usize::try_from(self.start_byte).map_err(|_| {
            RetrievalError::InvalidEnvelope("chunk start_byte is out of range".to_owned())
        })?;
        let end = usize::try_from(self.end_byte).map_err(|_| {
            RetrievalError::InvalidEnvelope("chunk end_byte is out of range".to_owned())
        })?;
        let bytes = source.text.as_bytes();
        if start >= end || end > bytes.len() || bytes.get(start..end) != Some(self.text.as_bytes())
        {
            return Err(RetrievalError::InvalidEnvelope(
                "chunk bytes do not round-trip to the canonical source".to_owned(),
            ));
        }
        if self.heading_depth > 6
            || self.ordinal_within_source == 0
            || self.part_ordinal == 0
            || self.start_line == 0
            || self.end_line < self.start_line
            || self.structural_position.is_empty()
            || self.text.len() > MAX_CHUNK_BYTES
        {
            return Err(RetrievalError::InvalidEnvelope(
                "chunk structural or coordinate invariants are invalid".to_owned(),
            ));
        }
        let expected_content_digest = crate::digest::sha256(self.text.as_bytes());
        if self.content_digest != expected_content_digest || !is_sha256_digest(&self.content_digest)
        {
            return Err(RetrievalError::InvalidEnvelope(
                "content_digest does not bind the exact chunk text".to_owned(),
            ));
        }
        let expected_chunk_id = crate::chunker::chunk_identity(
            &self.source_id,
            &self.structural_position,
            self.part_ordinal,
            &self.content_digest,
        )?;
        if self.chunk_id != expected_chunk_id || !is_sha256_digest(&self.chunk_id) {
            return Err(RetrievalError::InvalidEnvelope(
                "chunk_id does not bind the frozen identity inputs".to_owned(),
            ));
        }
        if usize::try_from(self.token_count).ok() != Some(crate::chunker::token_count(&self.text)) {
            return Err(RetrievalError::InvalidEnvelope(
                "token_count does not match the frozen ASCII-whitespace tokenizer".to_owned(),
            ));
        }
        if self.start_line != crate::chunker::line_number(&source.text, start)
            || self.end_line
                != crate::chunker::line_number(&source.text, end.saturating_sub(1).max(start))
        {
            return Err(RetrievalError::InvalidEnvelope(
                "line coordinates do not match exact source bytes".to_owned(),
            ));
        }
        if self.metadata != source.metadata
            || self.lineage_id != source.lineage.lineage_id
            || self.supersedes != source.lineage.supersedes
            || self.superseded_by != source.lineage.superseded_by
            || self.valid_from != source.temporal.valid_from
            || self.valid_to != source.temporal.valid_to
        {
            return Err(RetrievalError::InvalidEnvelope(
                "chunk metadata, lineage, or temporal fields diverge from the canonical envelope"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MatchedSpan {
    pub start_byte: u64,
    pub end_byte: u64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCitation {
    pub source_id: String,
    pub path: String,
    pub source_digest: String,
    pub heading_path: Vec<String>,
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
    pub verified: bool,
    pub stale: bool,
    pub matched_spans: Vec<MatchedSpan>,
}

impl From<&RetrievalChunk> for SourceCitation {
    fn from(chunk: &RetrievalChunk) -> Self {
        Self {
            source_id: chunk.source_id.clone(),
            path: chunk.source_path.clone(),
            source_digest: chunk.source_digest.clone(),
            heading_path: chunk.heading_path.clone(),
            start_byte: chunk.start_byte,
            end_byte: chunk.end_byte,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            verified: false,
            stale: true,
            matched_spans: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalFilters {
    pub vault: Option<String>,
    pub path_include: Option<Vec<String>>,
    pub path_exclude: Option<Vec<String>>,
    pub tags_any: Option<Vec<String>>,
    pub tags_all: Option<Vec<String>>,
    pub topics: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub authored_from: Option<String>,
    pub authored_to: Option<String>,
    pub sensitivity_ceiling: Option<GkxSensitivity>,
    pub gkx_types: Option<Vec<String>>,
    pub epistemic_states: Option<Vec<String>>,
    pub governance_states: Option<Vec<String>>,
    pub review_states: Option<Vec<String>>,
    pub authoritative: Option<bool>,
    pub moc_relationships: Option<Vec<String>>,
    pub source_digests: Option<Vec<String>>,
    pub author_agent_ids: Option<Vec<String>>,
    pub minimum_quality: Option<f64>,
    pub include_archives: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalStageScores {
    pub lexical_score: Option<f64>,
    pub semantic_score: Option<f64>,
    pub fusion_score: f64,
    pub reranker_score: Option<f64>,
    pub mmr_score: Option<f64>,
    pub lexical_rank: Option<u32>,
    pub semantic_rank: Option<u32>,
    pub fused_rank: u32,
    pub reranker_rank: Option<u32>,
    pub final_rank: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
    Insufficient,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalConfidence {
    pub level: ConfidenceLevel,
    pub low_confidence: bool,
    pub reason_codes: Vec<String>,
    pub lexical_signal: Option<f64>,
    pub semantic_signal: Option<f64>,
    pub reranker_signal: Option<f64>,
    pub coverage_signal: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalParentContext {
    pub chunk_id: String,
    pub text: String,
    pub citation: SourceCitation,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RetrievalHit {
    pub(crate) chunk: RetrievalChunk,
    pub(crate) citation: SourceCitation,
    pub(crate) stage_scores: RetrievalStageScores,
    pub(crate) parent_context: Option<RetrievalParentContext>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalProviderKind {
    None,
    OpenaiCompatible,
    LocalOnnx,
    Mcp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalStageState {
    Active,
    Disabled,
    Skipped,
    Degraded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalProviderStageKind {
    None,
    SqliteFts5,
    SqliteLexicalScan,
    OpenaiCompatible,
    LocalOnnx,
    Mcp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqliteLexicalBackend {
    SqliteFts5,
    SqliteLexicalScan,
}

impl SqliteLexicalBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SqliteFts5 => "sqlite_fts5",
            Self::SqliteLexicalScan => "sqlite_lexical_scan",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalProviderStageStatus {
    pub kind: RetrievalProviderStageKind,
    pub state: RetrievalStageState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
    pub lexical_top_k: Option<u32>,
    pub semantic_top_k: Option<u32>,
    pub filters: Option<RetrievalFilters>,
    pub rrf_k: Option<u64>,
    pub mmr: Option<bool>,
    pub mmr_lambda: Option<f64>,
    pub parent_expansion: Option<bool>,
    pub parent_expansion_max_child_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalSearchStages {
    pub lexical: RetrievalProviderStageStatus,
    pub vector: RetrievalProviderStageStatus,
    pub reranker: RetrievalProviderStageStatus,
}

/// Internal search assembly. Serialize only through
/// `redaction::authorize_search_result` so relationship IDs are endpoint-gated.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RetrievalSearchResult {
    pub(crate) contract_version: String,
    pub(crate) query_digest: String,
    pub(crate) projection_id: String,
    pub(crate) projection_digest: String,
    pub(crate) projection_freshness: ProjectionFreshness,
    pub(crate) hits: Vec<RetrievalHit>,
    pub(crate) confidence: RetrievalConfidence,
    pub(crate) applied_filters: Vec<String>,
    pub(crate) eligible_result_count: u32,
    pub(crate) stages: RetrievalSearchStages,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFreshness {
    Fresh,
    Stale,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalProjectionManifest {
    pub contract_version: String,
    pub projection_schema_version: u32,
    pub projection_id: String,
    pub engine_version: String,
    pub vault_id: String,
    pub source_snapshot_digest: String,
    pub configuration_digest: String,
    pub policy_digest: String,
    pub chunker_version: String,
    pub tokenizer_version: String,
    pub lexical_backend: SqliteLexicalBackend,
    pub embedding_provider_id: Option<String>,
    pub embedding_model_id: Option<String>,
    pub embedding_dimensions: Option<u32>,
    pub source_count: u32,
    pub chunk_count: u32,
    pub projection_digest: String,
}

impl RetrievalProjectionManifest {
    pub fn validate(&self) -> RetrievalResult<()> {
        if self.projection_schema_version != PROJECTION_SCHEMA_VERSION
            || self.contract_version != RETRIEVAL_CONTRACT
            || self.chunker_version != CHUNKER_VERSION
            || self.tokenizer_version != TOKENIZER_VERSION
        {
            return Err(RetrievalError::ProjectionMismatch(
                "manifest contract coordinates are incompatible".to_owned(),
            ));
        }
        for (field, value) in [
            ("projection_id", self.projection_id.as_str()),
            ("engine_version", self.engine_version.as_str()),
            ("vault_id", self.vault_id.as_str()),
            (
                "source_snapshot_digest",
                self.source_snapshot_digest.as_str(),
            ),
            ("configuration_digest", self.configuration_digest.as_str()),
            ("policy_digest", self.policy_digest.as_str()),
            ("projection_digest", self.projection_digest.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(RetrievalError::ProjectionMismatch(format!(
                    "manifest {field} must not be empty"
                )));
            }
        }
        for (field, value) in [
            (
                "source_snapshot_digest",
                self.source_snapshot_digest.as_str(),
            ),
            ("configuration_digest", self.configuration_digest.as_str()),
            ("policy_digest", self.policy_digest.as_str()),
            ("projection_digest", self.projection_digest.as_str()),
        ] {
            if !is_sha256_digest(value) {
                return Err(RetrievalError::ProjectionMismatch(format!(
                    "manifest {field} must be a lowercase SHA-256 digest"
                )));
            }
        }
        let expected_id = format!("retrieval:{}", &self.projection_digest[7..31]);
        if self.projection_id != expected_id {
            return Err(RetrievalError::ProjectionMismatch(
                "projection_id does not bind projection_digest".to_owned(),
            ));
        }
        match (
            &self.embedding_provider_id,
            &self.embedding_model_id,
            self.embedding_dimensions,
        ) {
            (None, None, None) => {}
            (Some(provider), Some(model), Some(dimensions))
                if !provider.is_empty() && !model.is_empty() && dimensions > 0 => {}
            _ => {
                return Err(RetrievalError::ProjectionMismatch(
                    "embedding manifest fields must be all absent or all valid".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RankedCandidate {
    pub chunk_id: String,
    pub source_id: String,
    pub lexical_rank: Option<u32>,
    pub lexical_score: Option<f64>,
    pub semantic_rank: Option<u32>,
    pub semantic_score: Option<f64>,
    pub fusion_score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmr_score: Option<f64>,
}

pub fn is_valid_authored_uid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || [8, 13, 18, 23]
            .into_iter()
            .any(|index| bytes[index] != b'-')
    {
        return false;
    }
    for (index, byte) in bytes.iter().copied().enumerate() {
        if [8, 13, 18, 23].contains(&index) {
            continue;
        }
        if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    matches!(bytes[14].to_ascii_lowercase(), b'1'..=b'8')
        && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
}

pub(crate) fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.strip_prefix("sha256:").is_some_and(|digest| {
            digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

pub fn is_valid_retrieval_source_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && !component
                    .chars()
                    .any(|character| character <= '\u{1f}' || "<>:\"|?*".contains(character))
                && !component.ends_with('.')
                && !component.ends_with(' ')
        })
}

pub(crate) use is_valid_retrieval_source_path as is_normalized_relative_path;
