use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::chunker::chunk_identity;
use crate::contract::{
    is_sha256_digest, ProjectionFreshness, RetrievalChunk, RetrievalChunkMetadata,
    RetrievalConfidence, RetrievalHit, RetrievalParentContext, RetrievalProviderStageStatus,
    RetrievalSearchResult, SourceCitation, RETRIEVAL_CONTRACT,
};
use crate::digest::sha256;
use crate::{RetrievalError, RetrievalResult};

/// Serializable result type whose relationship-bearing identifiers have been
/// checked against the eligible, discoverable endpoint sets.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedRetrievalSearchResult {
    pub(crate) contract_version: String,
    pub(crate) query_digest: String,
    pub(crate) projection_id: String,
    pub(crate) projection_digest: String,
    pub(crate) projection_freshness: ProjectionFreshness,
    pub(crate) hits: Vec<AuthorizedRetrievalHit>,
    pub(crate) confidence: RetrievalConfidence,
    pub(crate) applied_filters: Vec<String>,
    pub(crate) eligible_result_count: u32,
    pub(crate) stages: AuthorizedRetrievalSearchStages,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedRetrievalHit {
    pub(crate) chunk: RetrievalChunk,
    pub(crate) citation: SourceCitation,
    pub(crate) stage_scores: crate::contract::RetrievalStageScores,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_context: Option<RetrievalParentContext>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedRetrievalSearchStages {
    pub(crate) lexical: RetrievalProviderStageStatus,
    pub(crate) vector: RetrievalProviderStageStatus,
    pub(crate) reranker: RetrievalProviderStageStatus,
}

impl AuthorizedRetrievalSearchResult {
    pub fn contract_version(&self) -> &str {
        &self.contract_version
    }
    pub fn query_digest(&self) -> &str {
        &self.query_digest
    }
    pub fn projection_id(&self) -> &str {
        &self.projection_id
    }
    pub fn projection_digest(&self) -> &str {
        &self.projection_digest
    }
    pub fn projection_freshness(&self) -> ProjectionFreshness {
        self.projection_freshness
    }
    pub fn hits(&self) -> &[AuthorizedRetrievalHit] {
        &self.hits
    }
    pub fn confidence(&self) -> &RetrievalConfidence {
        &self.confidence
    }
    pub fn applied_filters(&self) -> &[String] {
        &self.applied_filters
    }
    pub fn eligible_result_count(&self) -> u32 {
        self.eligible_result_count
    }
    pub fn stages(&self) -> &AuthorizedRetrievalSearchStages {
        &self.stages
    }
}

impl AuthorizedRetrievalHit {
    pub fn chunk(&self) -> &RetrievalChunk {
        &self.chunk
    }
    pub fn citation(&self) -> &SourceCitation {
        &self.citation
    }
    pub fn stage_scores(&self) -> &crate::contract::RetrievalStageScores {
        &self.stage_scores
    }
    pub fn parent_context(&self) -> Option<&RetrievalParentContext> {
        self.parent_context.as_ref()
    }
}

impl AuthorizedRetrievalSearchStages {
    pub fn lexical(&self) -> &RetrievalProviderStageStatus {
        &self.lexical
    }
    pub fn vector(&self) -> &RetrievalProviderStageStatus {
        &self.vector
    }
    pub fn reranker(&self) -> &RetrievalProviderStageStatus {
        &self.reranker
    }
}

/// Prepare an externally serializable result. Policy filtering still belongs
/// before scoring; this is a final relationship/existence-leak backstop.
pub(crate) fn authorize_search_result(
    result: &RetrievalSearchResult,
    discoverable_source_ids: &BTreeSet<String>,
    eligible_discoverable_chunks: &BTreeMap<&str, &RetrievalChunk>,
) -> RetrievalResult<AuthorizedRetrievalSearchResult> {
    if result.eligible_result_count as usize != eligible_discoverable_chunks.len()
        || result.contract_version != RETRIEVAL_CONTRACT
        || !is_sha256_digest(&result.query_digest)
        || !is_sha256_digest(&result.projection_digest)
        || result.projection_id != format!("retrieval:{}", &result.projection_digest[7..31])
        || result.hits.iter().any(|hit| {
            !discoverable_source_ids.contains(&hit.chunk.source_id)
                || !eligible_discoverable_chunks.contains_key(hit.chunk.chunk_id.as_str())
                || !hit_binding_is_valid(hit, eligible_discoverable_chunks)
        })
    {
        return Err(RetrievalError::NotDiscoverable(
            "assembled result is not wholly policy-eligible and citation-verified".to_owned(),
        ));
    }
    let hits = result
        .hits
        .iter()
        .map(|hit| authorize_hit(hit, discoverable_source_ids, eligible_discoverable_chunks))
        .collect::<Vec<_>>();
    let eligible_count = eligible_discoverable_chunks.len();
    Ok(AuthorizedRetrievalSearchResult {
        contract_version: result.contract_version.clone(),
        query_digest: result.query_digest.clone(),
        projection_id: result.projection_id.clone(),
        projection_digest: result.projection_digest.clone(),
        projection_freshness: result.projection_freshness,
        hits,
        confidence: result.confidence.clone(),
        applied_filters: result.applied_filters.clone(),
        eligible_result_count: u32::try_from(eligible_count).unwrap_or(u32::MAX),
        stages: AuthorizedRetrievalSearchStages {
            lexical: result.stages.lexical.clone(),
            vector: result.stages.vector.clone(),
            reranker: result.stages.reranker.clone(),
        },
    })
}

fn hit_binding_is_valid(
    hit: &RetrievalHit,
    eligible_chunks: &BTreeMap<&str, &RetrievalChunk>,
) -> bool {
    let chunk = &hit.chunk;
    let chunk_content_length = u64::try_from(chunk.text.len()).ok();
    let chunk_identity_is_valid = chunk_identity(
        &chunk.source_id,
        &chunk.structural_position,
        chunk.part_ordinal,
        &chunk.content_digest,
    )
    .ok()
    .is_some_and(|expected| expected == chunk.chunk_id);
    if eligible_chunks.get(chunk.chunk_id.as_str()).copied() != Some(chunk)
        || !is_sha256_digest(&chunk.chunk_id)
        || !is_sha256_digest(&chunk.source_digest)
        || chunk.content_digest != sha256(chunk.text.as_bytes())
        || !chunk_identity_is_valid
        || chunk.end_byte.checked_sub(chunk.start_byte) != chunk_content_length
        || !citation_binds_text(&hit.citation, &chunk.text)
        || hit.citation.source_id != chunk.source_id
        || hit.citation.path != chunk.source_path
        || hit.citation.source_digest != chunk.source_digest
        || hit.citation.heading_path != chunk.heading_path
        || hit.citation.start_byte != chunk.start_byte
        || hit.citation.end_byte != chunk.end_byte
        || hit.citation.start_line != chunk.start_line
        || hit.citation.end_line != chunk.end_line
    {
        return false;
    }
    match &hit.parent_context {
        None => true,
        Some(parent) => {
            let Some(actual_parent) = eligible_chunks.get(parent.chunk_id.as_str()).copied() else {
                return false;
            };
            chunk.parent_chunk_id.as_deref() == Some(parent.chunk_id.as_str())
                && actual_parent.source_id == chunk.source_id
                && parent.text == actual_parent.text
                && parent.citation.source_id == actual_parent.source_id
                && parent.citation.path == actual_parent.source_path
                && parent.citation.source_digest == actual_parent.source_digest
                && parent.citation.heading_path == actual_parent.heading_path
                && parent.citation.start_byte == actual_parent.start_byte
                && parent.citation.end_byte == actual_parent.end_byte
                && parent.citation.start_line == actual_parent.start_line
                && parent.citation.end_line == actual_parent.end_line
                && citation_binds_text(&parent.citation, &actual_parent.text)
        }
    }
}

fn citation_binds_text(citation: &SourceCitation, text: &str) -> bool {
    if !citation.verified
        || citation.stale
        || citation.start_byte >= citation.end_byte
        || citation.start_line == 0
        || citation.end_line < citation.start_line
        || citation.end_byte.checked_sub(citation.start_byte) != u64::try_from(text.len()).ok()
        || citation.matched_spans.len() > 8
    {
        return false;
    }
    let mut returned_bytes = 0_usize;
    citation.matched_spans.iter().all(|span| {
        let Some(relative_start) = span
            .start_byte
            .checked_sub(citation.start_byte)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        let Some(relative_end) = span
            .end_byte
            .checked_sub(citation.start_byte)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        if span.start_byte >= span.end_byte
            || span.end_byte > citation.end_byte
            || span.text.len() > 256
            || returned_bytes.saturating_add(span.text.len()) > 1_024
            || text.as_bytes().get(relative_start..relative_end) != Some(span.text.as_bytes())
        {
            return false;
        }
        returned_bytes += span.text.len();
        true
    })
}

fn authorize_hit(
    hit: &RetrievalHit,
    _discoverable_source_ids: &BTreeSet<String>,
    eligible_chunks: &BTreeMap<&str, &RetrievalChunk>,
) -> AuthorizedRetrievalHit {
    let mut chunk = hit.chunk.clone();
    // Phase 1 has no authorized endpoint resolver contract. Even a currently
    // discoverable target must remain suppressed until the lineage/MCP phase
    // can authorize and resolve the relationship endpoint explicitly.
    chunk.supersedes.clear();
    chunk.superseded_by.clear();
    chunk.metadata = authorized_metadata(&chunk.metadata);
    if chunk
        .parent_chunk_id
        .as_ref()
        .is_some_and(|chunk_id| !eligible_chunks.contains_key(chunk_id.as_str()))
    {
        chunk.parent_chunk_id = None;
    }
    let parent_context = hit
        .parent_context
        .as_ref()
        .filter(|parent| eligible_chunks.contains_key(parent.chunk_id.as_str()))
        .cloned();
    AuthorizedRetrievalHit {
        chunk,
        citation: hit.citation.clone(),
        stage_scores: hit.stage_scores.clone(),
        parent_context,
    }
}

fn authorized_metadata(metadata: &RetrievalChunkMetadata) -> RetrievalChunkMetadata {
    RetrievalChunkMetadata {
        title: metadata.title.clone(),
        tags: metadata.tags.clone(),
        topic: metadata.topic.clone(),
        category: metadata.category.clone(),
        authored_at: metadata.authored_at.clone(),
        sensitivity: metadata.sensitivity,
        gkx_type: metadata.gkx_type.clone(),
        epistemic_state: metadata.epistemic_state.clone(),
        governance_state: metadata.governance_state.clone(),
        review_state: metadata.review_state.clone(),
        authoritative: metadata.authoritative,
        moc_relationships: None,
        author_agent_id: None,
        quality: metadata.quality,
        archived: metadata.archived,
        extra: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use crate::chunker::{chunk_source, ChunkingOptions};
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, ConfidenceLevel,
        DiscoverabilityDecision, RetrievalChunkMetadata, RetrievalProviderStageKind,
        RetrievalSearchStages, RetrievalSource, RetrievalStageScores, RetrievalStageState,
        RETRIEVAL_CONTRACT,
    };
    use crate::digest::sha256;

    use super::*;

    fn disabled_stage() -> RetrievalProviderStageStatus {
        RetrievalProviderStageStatus {
            kind: RetrievalProviderStageKind::None,
            state: RetrievalStageState::Disabled,
            provider_id: None,
            model_id: None,
            reason_codes: Vec::new(),
        }
    }

    fn source_for(text: &str) -> RetrievalSource {
        RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: "019b2d14-4230-7db7-87d4-7d81cfaeca01".to_owned(),
            source_path: "visible.md".to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        }
    }

    fn verified_citation(chunk: &RetrievalChunk) -> SourceCitation {
        let mut citation = SourceCitation::from(chunk);
        citation.verified = true;
        citation.stale = false;
        citation
    }

    fn result_for(
        chunk: RetrievalChunk,
        parent_context: Option<RetrievalParentContext>,
        eligible_result_count: u32,
    ) -> RetrievalSearchResult {
        let projection_digest = sha256(b"projection");
        RetrievalSearchResult {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            query_digest: sha256(b"query"),
            projection_id: format!("retrieval:{}", &projection_digest[7..31]),
            projection_digest,
            projection_freshness: ProjectionFreshness::Fresh,
            hits: vec![RetrievalHit {
                citation: verified_citation(&chunk),
                chunk,
                stage_scores: RetrievalStageScores {
                    lexical_score: Some(1.0),
                    semantic_score: None,
                    fusion_score: 1.0 / 61.0,
                    reranker_score: None,
                    mmr_score: None,
                    lexical_rank: Some(1),
                    semantic_rank: None,
                    fused_rank: 1,
                    reranker_rank: None,
                    final_rank: 1,
                },
                parent_context,
            }],
            confidence: RetrievalConfidence {
                level: ConfidenceLevel::High,
                low_confidence: false,
                reason_codes: Vec::new(),
                lexical_signal: Some(1.0),
                semantic_signal: None,
                reranker_signal: None,
                coverage_signal: Some(1.0),
            },
            applied_filters: Vec::new(),
            eligible_result_count,
            stages: RetrievalSearchStages {
                lexical: RetrievalProviderStageStatus {
                    kind: RetrievalProviderStageKind::SqliteFts5,
                    state: RetrievalStageState::Active,
                    provider_id: None,
                    model_id: None,
                    reason_codes: Vec::new(),
                },
                vector: disabled_stage(),
                reranker: disabled_stage(),
            },
        }
    }

    #[test]
    fn phase_one_result_suppresses_relationships_and_unknown_metadata() {
        let hidden = "019b2d14-4230-7db7-87d4-7d81cfaecaff";
        let visible_target = "019b2d14-4230-7db7-87d4-7d81cfaeca02";
        let hidden_path = "private/hidden.md";
        let text = "# Visible\nAuthorized text";
        let mut source = source_for(text);
        source.lineage.supersedes = vec![hidden.to_owned(), visible_target.to_owned()];
        source.metadata.title = Some("Visible title".to_owned());
        source.metadata.moc_relationships =
            Some(vec![hidden.to_owned(), visible_target.to_owned()]);
        source.metadata.extra.insert(
            "custom_relationship".to_owned(),
            serde_json::json!({ "target": hidden, "path": hidden_path }),
        );
        let chunk = chunk_source(&source, ChunkingOptions::default())
            .unwrap()
            .remove(0);
        let result = result_for(chunk.clone(), None, 1);
        let eligible = BTreeMap::from([(chunk.chunk_id.as_str(), &chunk)]);
        let authorized = authorize_search_result(
            &result,
            &BTreeSet::from([source.source_id, visible_target.to_owned()]),
            &eligible,
        )
        .unwrap();
        let json = serde_json::to_string(&authorized).unwrap();
        assert!(!json.contains(hidden));
        assert!(!json.contains(visible_target));
        assert!(!json.contains(hidden_path));
        assert_eq!(authorized.eligible_result_count, 1);
        assert_eq!(
            authorized.hits[0].chunk.metadata.title.as_deref(),
            Some("Visible title")
        );
        assert!(authorized.hits[0].chunk.supersedes.is_empty());
        assert!(authorized.hits[0]
            .chunk
            .metadata
            .moc_relationships
            .is_none());
        assert!(authorized.hits[0].chunk.metadata.extra.is_empty());
    }

    #[test]
    fn forged_hit_path_text_and_matched_span_are_rejected() {
        let source = source_for("# Visible\nAuthorized text");
        let chunk = chunk_source(&source, ChunkingOptions::default())
            .unwrap()
            .remove(0);
        let sources = BTreeSet::from([source.source_id]);
        let eligible = BTreeMap::from([(chunk.chunk_id.as_str(), &chunk)]);

        let mut forged_path = result_for(chunk.clone(), None, 1);
        forged_path.hits[0].citation.path = "private/forged.md".to_owned();
        assert!(authorize_search_result(&forged_path, &sources, &eligible).is_err());

        let mut forged_text = result_for(chunk.clone(), None, 1);
        forged_text.hits[0].chunk.text.push('!');
        assert!(authorize_search_result(&forged_text, &sources, &eligible).is_err());

        let mut forged_span = result_for(chunk.clone(), None, 1);
        forged_span.hits[0].citation.matched_spans = vec![crate::contract::MatchedSpan {
            start_byte: chunk.start_byte,
            end_byte: chunk.start_byte + 1,
            text: "X".to_owned(),
        }];
        assert!(authorize_search_result(&forged_span, &sources, &eligible).is_err());
    }

    #[test]
    fn forged_or_stale_parent_context_is_rejected() {
        let source = source_for("# Parent\ntext\n## Child\nAuthorized text");
        let chunks = chunk_source(&source, ChunkingOptions::default()).unwrap();
        let parent = chunks[0].clone();
        let child = chunks[1].clone();
        let parent_context = RetrievalParentContext {
            chunk_id: parent.chunk_id.clone(),
            text: parent.text.clone(),
            citation: verified_citation(&parent),
        };
        let source_ids = BTreeSet::from([source.source_id]);
        let eligible = BTreeMap::from([
            (parent.chunk_id.as_str(), &parent),
            (child.chunk_id.as_str(), &child),
        ]);
        let valid = result_for(child.clone(), Some(parent_context), 2);
        assert!(authorize_search_result(&valid, &source_ids, &eligible).is_ok());

        let mut stale = valid.clone();
        stale.hits[0]
            .parent_context
            .as_mut()
            .unwrap()
            .citation
            .stale = true;
        assert!(authorize_search_result(&stale, &source_ids, &eligible).is_err());

        let mut forged = valid;
        forged.hits[0]
            .parent_context
            .as_mut()
            .unwrap()
            .citation
            .path = "private/forged-parent.md".to_owned();
        assert!(authorize_search_result(&forged, &source_ids, &eligible).is_err());
    }
}
