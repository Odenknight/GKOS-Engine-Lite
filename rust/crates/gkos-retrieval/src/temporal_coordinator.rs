use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use serde::Serialize;

use crate::authorized_view::build_authorized_candidate_view;
use crate::candidate::{GkxCandidateChunk, GkxCandidateSource, GkxCandidateVector};
use crate::confidence::assess_retrieval_confidence;
use crate::contract::{
    DiscoverabilityDecision, GkxRetrievalSearchRequest, RankedCandidate, RetrievalChunk,
    RetrievalChunkMetadata, RetrievalConfidence, RetrievalProviderStageKind,
    RetrievalProviderStageStatus, RetrievalSearchRequest, RetrievalSearchStages,
    RetrievalStageScores, RetrievalStageState, SourceCitation, MMR_DEFAULT_LAMBDA,
    PARENT_EXPANSION_MAX_CHILD_TOKENS, RETRIEVAL_LINEAGE_CONTRACT, RRF_DEFAULT_K,
};
use crate::coordinator::{
    deduplicate_overlap_evidence, embed_chunks, lexical_stage, stage, validate_search_request,
    verified_citation, AcceptedCitationInterval, DiscoverabilityPolicy, MatchedSpanKey,
    SourceReader,
};
use crate::digest::{canonical_digest, canonical_json, sha256};
use crate::error::cache_value_or_miss;
use crate::filters::{applied_filter_names, matches_retrieval_filters};
use crate::fusion::{
    code_unit_compare, maximal_marginal_relevance_with_relevance, reciprocal_rank_fusion,
};
use crate::lineage_store::{
    build_gkx_retrieval_generation_with_writer, open_active_gkx_retrieval_generation,
    try_open_active_gkx_retrieval_generation, BuiltGkxRetrievalGeneration,
    GkxRetrievalGenerationInput, GkxSqliteRetrievalStore,
};
use crate::provenance::{
    build_public_provenance, normalize_retrieval_as_of, GkxAuthorizedTemporalSource,
    GkxPublicProvenance, GkxStoredSourceProvenance, TemporalCoverage,
};
use crate::providers::{
    bounded_provider_call, validate_rerank_provider_identity, validate_vector_provider_identity,
    RerankProvider, VectorProvider, VectorProviderIdentity,
};
use crate::sqlite_store::trim_ecmascript_whitespace;
use crate::writer_lock::{
    acquire_legacy_retrieval_writer, assert_legacy_writer_capability, assert_no_phase3_authority,
    finish_with_writer,
};
use crate::{RetrievalError, RetrievalResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GkxProjectionFreshness {
    Fresh,
    Stale,
    Unverified,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxSourcePolicyRecord {
    pub source_id: String,
    pub source_path: String,
    pub source_digest: String,
    pub lineage_id: (),
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub supersedes: Vec<String>,
    pub superseded_by: Vec<String>,
    pub metadata: RetrievalChunkMetadata,
}

pub type GkxSourceDiscoverabilityPolicy =
    dyn Fn(&GkxSourcePolicyRecord) -> RetrievalResult<DiscoverabilityDecision> + Send + Sync;

pub struct GkxRetrievalCoordinatorOptions<'a> {
    pub runtime_policy_digest: String,
    pub source_discoverability_policy: &'a GkxSourceDiscoverabilityPolicy,
    pub discoverability_policy: &'a DiscoverabilityPolicy,
    pub vector_provider: Option<&'a dyn VectorProvider>,
    pub rerank_provider: Option<&'a dyn RerankProvider>,
    pub source_reader: &'a dyn SourceReader,
    pub lineage_view_freshness: GkxProjectionFreshness,
    pub max_parent_bytes: usize,
    pub max_result_bytes: usize,
}

impl GkxRetrievalCoordinatorOptions<'_> {
    fn validate(&self) -> RetrievalResult<()> {
        if !crate::contract::is_sha256_digest(&self.runtime_policy_digest)
            || !(256..=65_536).contains(&self.max_parent_bytes)
            || !(16_384..=1_048_576).contains(&self.max_result_bytes)
        {
            return Err(RetrievalError::InvalidConfig(
                "schema-3 coordinator policy digest or byte budgets are invalid".to_owned(),
            ));
        }
        if let Some(provider) = self.vector_provider {
            validate_vector_provider_identity(provider.identity())?;
        }
        if let Some(provider) = self.rerank_provider {
            validate_rerank_provider_identity(provider.identity())?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxTemporalResultState {
    as_of: Option<String>,
    coverage: TemporalCoverage,
    reason_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxRetrievalParentContext {
    chunk_id: String,
    text: String,
    citation: SourceCitation,
    provenance: GkxPublicProvenance,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxRetrievalHit {
    chunk: RetrievalChunk,
    citation: SourceCitation,
    provenance: GkxPublicProvenance,
    stage_scores: RetrievalStageScores,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_context: Option<GkxRetrievalParentContext>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxAuthorizedRetrievalSearchResult {
    contract_version: String,
    query_digest: String,
    projection_id: String,
    projection_digest: String,
    projection_freshness: GkxProjectionFreshness,
    hits: Vec<GkxRetrievalHit>,
    confidence: RetrievalConfidence,
    temporal: GkxTemporalResultState,
    applied_filters: Vec<String>,
    eligible_result_count: u32,
    stages: RetrievalSearchStages,
}

impl GkxAuthorizedRetrievalSearchResult {
    pub fn hits(&self) -> &[GkxRetrievalHit] {
        &self.hits
    }

    pub fn projection_digest(&self) -> &str {
        &self.projection_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IndexGkxRetrievalResult {
    pub(crate) generation: BuiltGkxRetrievalGeneration,
    pub(crate) vector_stage: RetrievalProviderStageStatus,
}

pub(crate) async fn index_gkx_retrieval_generation(
    input: GkxRetrievalGenerationInput,
    vector_provider: Option<&dyn VectorProvider>,
) -> RetrievalResult<IndexGkxRetrievalResult> {
    if !input.vectors.is_empty()
        || input.embedding_provider_id.is_some()
        || input.embedding_model_id.is_some()
        || input.embedding_dimensions.is_some()
    {
        return Err(RetrievalError::InvalidConfig(
            "schema-3 index coordinator accepts only an unembedded generation".to_owned(),
        ));
    }
    let prepared = crate::lineage_store::prepare_chunks_for_embedding(&input)?;
    let mut writer = acquire_legacy_retrieval_writer(&input.state_directory)?;
    let result = async {
        let identity = vector_provider.map(|provider| provider.identity().clone());
        if let Some(identity) = &identity {
            validate_vector_provider_identity(identity)?;
        }
        index_gkx_retrieval_generation_with_writer(
            input,
            vector_provider,
            identity,
            prepared,
            &writer,
        )
        .await
    }
    .await;
    finish_with_writer(result, &mut writer)
}

async fn index_gkx_retrieval_generation_with_writer(
    mut input: GkxRetrievalGenerationInput,
    vector_provider: Option<&dyn VectorProvider>,
    identity: Option<VectorProviderIdentity>,
    prepared: Vec<GkxCandidateChunk>,
    writer: &crate::writer_lock::LegacyRetrievalWriterCapability,
) -> RetrievalResult<IndexGkxRetrievalResult> {
    assert_legacy_writer_capability(writer, &input.state_directory)?;
    assert_no_phase3_authority(writer.state_directory())?;
    let Some(provider) = vector_provider else {
        return Ok(IndexGkxRetrievalResult {
            generation: build_gkx_retrieval_generation_with_writer(input, writer)?,
            vector_stage: stage(
                RetrievalProviderStageKind::None,
                RetrievalStageState::Disabled,
                &["VECTOR_DISABLED"],
                None,
            ),
        });
    };
    let identity = identity.expect("provider identity is preflighted with its provider");
    let eligible_ids = input
        .embedding_eligible_candidate_chunk_keys
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let eligible = prepared
        .iter()
        .filter(|chunk| eligible_ids.contains(&chunk.candidate_chunk_key))
        .cloned()
        .collect::<Vec<_>>();
    let cache = match cache_value_or_miss(try_open_active_gkx_retrieval_generation(
        &input.state_directory,
    ))? {
        Some(Some(store))
            if store.manifest.vault_id == input.vault_id
                && store.manifest.policy_digest == input.policy_digest =>
        {
            cache_value_or_miss(store.cached_vectors_by_content(
                &identity.provider_id,
                &identity.model_id,
                identity.dimensions,
                &eligible_ids,
            ))?
            .unwrap_or_default()
        }
        _ => BTreeMap::new(),
    };
    let nested = eligible
        .iter()
        .map(|item| item.chunk.clone())
        .collect::<Vec<_>>();
    match embed_chunks(provider, &nested, &cache).await {
        Ok(vectors) => {
            input.vectors = eligible
                .iter()
                .zip(vectors)
                .map(|(candidate, vector)| GkxCandidateVector {
                    candidate_chunk_key: candidate.candidate_chunk_key.clone(),
                    vector: vector.vector,
                })
                .collect();
            input.embedding_provider_id = Some(identity.provider_id.clone());
            input.embedding_model_id = Some(identity.model_id.clone());
            input.embedding_dimensions = Some(identity.dimensions);
            Ok(IndexGkxRetrievalResult {
                generation: build_gkx_retrieval_generation_with_writer(input, writer)?,
                vector_stage: stage(
                    identity.kind,
                    RetrievalStageState::Active,
                    &[],
                    Some(&identity),
                ),
            })
        }
        Err(_) => Ok(IndexGkxRetrievalResult {
            generation: build_gkx_retrieval_generation_with_writer(input, writer)?,
            vector_stage: stage(
                identity.kind,
                RetrievalStageState::Degraded,
                &["VECTOR_UNAVAILABLE"],
                Some(&identity),
            ),
        }),
    }
}

pub struct GkxRetrievalCoordinator<'a> {
    store: GkxSqliteRetrievalStore,
    options: GkxRetrievalCoordinatorOptions<'a>,
}

impl<'a> GkxRetrievalCoordinator<'a> {
    pub fn open_active(
        state_directory: &Path,
        options: GkxRetrievalCoordinatorOptions<'a>,
    ) -> RetrievalResult<Self> {
        options.validate()?;
        let store = open_active_gkx_retrieval_generation(state_directory)?;
        if store.manifest.policy_digest != options.runtime_policy_digest {
            return Err(RetrievalError::ProjectionMismatch(
                "RETRIEVAL_RUNTIME_POLICY_DIGEST_MISMATCH".to_owned(),
            ));
        }
        Ok(Self { store, options })
    }

    pub async fn search(
        &self,
        request: &GkxRetrievalSearchRequest,
    ) -> RetrievalResult<GkxAuthorizedRetrievalSearchResult> {
        let phase1 = RetrievalSearchRequest::from(request);
        validate_search_request(&request.query, &phase1)?;
        let query = trim_ecmascript_whitespace(&request.query);
        let normalized_as_of = request
            .as_of
            .as_deref()
            .map(normalize_retrieval_as_of)
            .transpose()?;
        let filters = request.filters.clone().unwrap_or_default();
        let limit = request.limit.unwrap_or(5) as usize;
        let lexical_top_k = request
            .lexical_top_k
            .map_or(20_usize.max(limit * 4), |value| value as usize);
        let semantic_top_k = request
            .semantic_top_k
            .map_or(20_usize.max(limit * 4), |value| value as usize);

        // Full supplies immutable source-local candidates and resolver tiers.
        // The trusted runtime applies source policy, then typed filters, then
        // whole-source chunk policy before any cross-record fact is derived.
        let all_sources = self.store.list_candidate_sources()?;
        let mut candidates = Vec::new();
        for source in all_sources {
            let policy_record = source_policy_record(&source);
            let filter_record = source_filter_record(&policy_record);
            let allowed =
                source_policy_allows(self.options.source_discoverability_policy, &policy_record)
                    && matches_retrieval_filters(
                        &filter_record,
                        &filters,
                        &self.store.manifest.vault_id,
                    )?;
            if allowed {
                candidates.push(source);
            }
        }
        let candidate_keys = candidates
            .iter()
            .map(|source| source.record_key.clone())
            .collect::<Vec<_>>();
        // Candidate rows excluded by source policy or typed filters never
        // cross SQLite for whole-source chunk-policy evaluation.
        let candidate_chunks = self
            .store
            .list_candidate_chunks_for_record_keys(&candidate_keys)?;
        let mut candidate_groups = BTreeMap::<String, Vec<GkxCandidateChunk>>::new();
        for chunk in candidate_chunks.iter().cloned() {
            candidate_groups
                .entry(chunk.record_key.clone())
                .or_default()
                .push(chunk);
        }
        let authorized_sources = candidates
            .into_iter()
            .filter(|source| {
                candidate_groups
                    .get(&source.record_key)
                    .is_none_or(|group| {
                        group.iter().all(|chunk| {
                            chunk_policy_allows(
                                self.options.discoverability_policy,
                                &chunk_policy_record(&chunk.chunk),
                            )
                        })
                    })
            })
            .collect::<Vec<_>>();
        let authorized_keys = authorized_sources
            .iter()
            .map(|source| source.record_key.clone())
            .collect::<Vec<_>>();
        // Parser-owned declaration receipts are internal authority-bearing
        // data and cross SQLite only for records admitted by both policy gates.
        let declarations = self
            .store
            .list_candidate_declarations_for_record_keys(&authorized_keys)?;
        let authorized_key_set = authorized_keys
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let authorized_chunks = candidate_chunks
            .into_iter()
            .filter(|chunk| authorized_key_set.contains(chunk.record_key.as_str()))
            .collect::<Vec<_>>();
        // Unknown and future records are partitioned before identity,
        // resolution, declaration, branch, cycle, and temporal-order checks.
        // Every visible conflict maps to one non-content-bearing error.
        let temporal_view = build_authorized_candidate_view(
            &authorized_sources,
            &declarations,
            &authorized_chunks,
            normalized_as_of.as_deref(),
        )?;
        let coordinate = result_projection_coordinate(
            &self.store,
            &temporal_view.sources,
            &temporal_view.temporal_sources,
        )?;
        let provenance_by_source = temporal_view
            .sources
            .iter()
            .map(|source| (source.source_id.clone(), source.clone()))
            .collect::<BTreeMap<_, _>>();
        let temporal_by_source = temporal_view
            .temporal_sources
            .iter()
            .map(|source| (source.source_id.clone(), source))
            .collect::<BTreeMap<_, _>>();
        let mut temporal_coverage = TemporalCoverage::NotRequested;
        if normalized_as_of.is_some() {
            if temporal_view.authorized_source_count == 0 {
                return self.empty_result(
                    query,
                    normalized_as_of,
                    &filters,
                    "NO_ELIGIBLE_RESULTS",
                    TemporalCoverage::NotEvaluated,
                    coordinate,
                    self.options.lineage_view_freshness,
                );
            }
            if temporal_view.coverage == TemporalCoverage::Insufficient {
                return self.empty_result(
                    query,
                    normalized_as_of,
                    &filters,
                    "TEMPORAL_COVERAGE_INSUFFICIENT",
                    TemporalCoverage::Insufficient,
                    coordinate,
                    self.options.lineage_view_freshness,
                );
            }
            temporal_coverage = TemporalCoverage::Sufficient;
        }

        // The complete temporal view must be covered before any live source
        // bytes or query text can cross an I/O or provider boundary. Stale
        // citation suppression cannot hide a persisted eligibility mismatch.
        if self.store.manifest.embedding_provider_id.is_some() {
            let temporal_vector_keys = temporal_view
                .eligible_candidate_chunk_keys
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            self.store
                .candidate_vector_eligibility_covers(&temporal_vector_keys)?;
        }

        // SQL sees only opaque candidate keys already admitted by all policy
        // gates and the half-open authorization-scoped temporal view.
        let candidate_chunks = self
            .store
            .list_candidate_chunks_for_keys(&temporal_view.eligible_candidate_chunk_keys)?;
        let mut groups = BTreeMap::<String, Vec<GkxCandidateChunk>>::new();
        for chunk in candidate_chunks {
            groups
                .entry(chunk.record_key.clone())
                .or_default()
                .push(chunk);
        }
        let mut source_bytes = BTreeMap::<String, Vec<u8>>::new();
        let mut eligible = Vec::new();
        let mut live_candidate_keys = BTreeSet::new();
        let mut stale_citation = false;
        for group in groups.into_values() {
            let first = &group[0].chunk;
            let bytes = match self.options.source_reader.read(&first.source_path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    stale_citation = true;
                    continue;
                }
            };
            if sha256(&bytes) != first.source_digest
                || group
                    .iter()
                    .any(|chunk| verified_citation(&chunk.chunk, "", &bytes).is_err())
            {
                stale_citation = true;
                continue;
            }
            source_bytes.insert(first.source_path.clone(), bytes);
            for candidate in group {
                live_candidate_keys.insert(candidate.candidate_chunk_key);
                eligible.push(candidate.chunk);
            }
        }
        let freshness = if stale_citation {
            GkxProjectionFreshness::Stale
        } else {
            self.options.lineage_view_freshness
        };
        if eligible.is_empty() {
            return self.empty_result(
                query,
                normalized_as_of,
                &filters,
                "NO_ELIGIBLE_RESULTS",
                temporal_coverage,
                coordinate,
                freshness,
            );
        }

        let eligible_ids = eligible
            .iter()
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<BTreeSet<_>>();
        let eligible_key_list = live_candidate_keys.iter().cloned().collect::<Vec<_>>();
        let lexical =
            self.store
                .lexical_search_eligible(query, &eligible_key_list, lexical_top_k)?;
        let lexical_status = lexical_stage(self.store.manifest.lexical_backend, true);
        let (semantic, vector_status) = self
            .semantic_search(query, &live_candidate_keys, semantic_top_k)
            .await?;
        let fused =
            reciprocal_rank_fusion(&lexical, &semantic, request.rrf_k.unwrap_or(RRF_DEFAULT_K))?;
        let fused_ranks = fused
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.chunk_id.clone(), index as u32 + 1))
            .collect::<BTreeMap<_, _>>();
        let (ordered, reranker_status, reranker_scores, reranker_ranks) =
            self.rerank(query, &eligible, &fused).await;
        let rerank_relevance = (reranker_status.state == RetrievalStageState::Active).then(|| {
            ordered
                .iter()
                .enumerate()
                .map(|(index, candidate)| (candidate.chunk_id.clone(), 1.0 / (index as f64 + 1.0)))
                .collect::<BTreeMap<_, _>>()
        });
        let selected = if request.mmr == Some(true) {
            maximal_marginal_relevance_with_relevance(
                &ordered,
                ordered.len(),
                request.mmr_lambda.unwrap_or(MMR_DEFAULT_LAMBDA),
                rerank_relevance.as_ref(),
            )?
        } else {
            ordered
        };
        let by_id = eligible
            .iter()
            .map(|chunk| (chunk.chunk_id.as_str(), chunk))
            .collect::<BTreeMap<_, _>>();
        let mut hits = Vec::new();
        let mut claimed_spans = BTreeSet::<MatchedSpanKey>::new();
        let mut accepted_intervals = Vec::<AcceptedCitationInterval>::new();
        let parent_threshold = request
            .parent_expansion_max_child_tokens
            .unwrap_or(PARENT_EXPANSION_MAX_CHILD_TOKENS);
        for candidate in selected {
            if hits.len() >= limit {
                break;
            }
            let chunk = by_id
                .get(candidate.chunk_id.as_str())
                .copied()
                .ok_or_else(|| RetrievalError::MissingChunk(candidate.chunk_id.clone()))?;
            let live = source_bytes.get(&chunk.source_path).ok_or_else(|| {
                RetrievalError::ProjectionMismatch("verified source bytes missing".to_owned())
            })?;
            let Some(evidence) = deduplicate_overlap_evidence(
                verified_citation(chunk, query, live)?,
                &claimed_spans,
                &accepted_intervals,
            ) else {
                continue;
            };
            let temporal = temporal_by_source
                .get(&chunk.source_id)
                .copied()
                .ok_or_else(|| {
                    RetrievalError::ProjectionMismatch("temporal row missing".to_owned())
                })?;
            let stored = provenance_by_source.get(&chunk.source_id).ok_or_else(|| {
                RetrievalError::ProjectionMismatch("source provenance missing".to_owned())
            })?;
            let output_chunk = authorized_result_chunk(chunk, &eligible_ids, temporal);
            let provenance = build_public_provenance(
                stored,
                &output_chunk,
                temporal,
                normalized_as_of.as_deref(),
            )?;
            let scores = RetrievalStageScores {
                lexical_score: candidate.lexical_score,
                semantic_score: candidate.semantic_score,
                fusion_score: candidate.fusion_score,
                reranker_score: reranker_scores.get(&chunk.chunk_id).copied(),
                mmr_score: candidate.mmr_score,
                lexical_rank: candidate.lexical_rank,
                semantic_rank: candidate.semantic_rank,
                fused_rank: fused_ranks[&chunk.chunk_id],
                reranker_rank: reranker_ranks.get(&chunk.chunk_id).copied(),
                final_rank: hits.len() as u32 + 1,
            };
            let parent_context =
                if request.parent_expansion == Some(true) && chunk.token_count < parent_threshold {
                    self.parent_context(
                        chunk,
                        &by_id,
                        &source_bytes,
                        &provenance_by_source,
                        &temporal_by_source,
                        normalized_as_of.as_deref(),
                    )?
                } else {
                    None
                };
            let mut candidate_hit = GkxRetrievalHit {
                chunk: output_chunk,
                citation: evidence.citation,
                provenance,
                stage_scores: scores,
                parent_context,
            };
            let stages = RetrievalSearchStages {
                lexical: lexical_status.clone(),
                vector: vector_status.clone(),
                reranker: reranker_status.clone(),
            };
            let mut candidate_result = assemble_result(
                query,
                normalized_as_of.as_deref(),
                &coordinate,
                freshness,
                &hits,
                Some(&candidate_hit),
                &filters,
                eligible.len(),
                temporal_coverage,
                &stages,
            )?;
            if canonical_json(&candidate_result)?.len() > self.options.max_result_bytes
                && candidate_hit.parent_context.is_some()
            {
                candidate_hit.parent_context = None;
                candidate_result = assemble_result(
                    query,
                    normalized_as_of.as_deref(),
                    &coordinate,
                    freshness,
                    &hits,
                    Some(&candidate_hit),
                    &filters,
                    eligible.len(),
                    temporal_coverage,
                    &stages,
                )?;
            }
            if canonical_json(&candidate_result)?.len() > self.options.max_result_bytes {
                continue;
            }
            claimed_spans.extend(evidence.span_keys);
            accepted_intervals.push(AcceptedCitationInterval {
                source_id: chunk.source_id.clone(),
                start_byte: chunk.start_byte,
                end_byte: chunk.end_byte,
            });
            hits.push(candidate_hit);
        }
        assemble_result(
            query,
            normalized_as_of.as_deref(),
            &coordinate,
            freshness,
            &hits,
            None,
            &filters,
            eligible.len(),
            temporal_coverage,
            &RetrievalSearchStages {
                lexical: lexical_status,
                vector: vector_status,
                reranker: reranker_status,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn empty_result(
        &self,
        query: &str,
        normalized_as_of: Option<String>,
        filters: &crate::contract::RetrievalFilters,
        reason: &'static str,
        coverage: TemporalCoverage,
        coordinate: ResultCoordinate,
        freshness: GkxProjectionFreshness,
    ) -> RetrievalResult<GkxAuthorizedRetrievalSearchResult> {
        let lexical = stage(
            self.store.manifest.lexical_backend.into_stage_kind(),
            RetrievalStageState::Skipped,
            &[reason],
            None,
        );
        let vector = self.options.vector_provider.map_or_else(
            || {
                stage(
                    RetrievalProviderStageKind::None,
                    RetrievalStageState::Disabled,
                    &["VECTOR_DISABLED"],
                    None,
                )
            },
            |provider| {
                stage(
                    provider.identity().kind,
                    RetrievalStageState::Skipped,
                    &[reason],
                    Some(provider.identity()),
                )
            },
        );
        let reranker = self.options.rerank_provider.map_or_else(
            || {
                stage(
                    RetrievalProviderStageKind::None,
                    RetrievalStageState::Skipped,
                    &["RERANKER_NOT_CONFIGURED"],
                    None,
                )
            },
            |provider| {
                stage(
                    provider.identity().kind,
                    RetrievalStageState::Skipped,
                    &[reason],
                    Some(provider.identity()),
                )
            },
        );
        let mut reasons = vec![reason.to_owned()];
        if freshness == GkxProjectionFreshness::Stale {
            reasons.push("STALE_PROJECTION".to_owned());
        }
        if freshness == GkxProjectionFreshness::Unverified {
            reasons.push("PROJECTION_FRESHNESS_UNVERIFIED".to_owned());
        }
        reasons.sort_by(|a, b| code_unit_compare(a, b));
        Ok(GkxAuthorizedRetrievalSearchResult {
            contract_version: RETRIEVAL_LINEAGE_CONTRACT.to_owned(),
            query_digest: lineage_query_digest(query, normalized_as_of.as_deref())?,
            projection_id: coordinate.projection_id,
            projection_digest: coordinate.projection_digest,
            projection_freshness: freshness,
            hits: vec![],
            confidence: RetrievalConfidence {
                level: crate::contract::ConfidenceLevel::Insufficient,
                low_confidence: true,
                reason_codes: reasons,
                lexical_signal: None,
                semantic_signal: None,
                reranker_signal: None,
                coverage_signal: None,
            },
            temporal: GkxTemporalResultState {
                as_of: normalized_as_of,
                coverage,
                reason_codes: if coverage == TemporalCoverage::Insufficient {
                    vec!["TEMPORAL_COVERAGE_INSUFFICIENT".to_owned()]
                } else {
                    Vec::new()
                },
            },
            applied_filters: applied_filter_names(filters),
            eligible_result_count: 0,
            stages: RetrievalSearchStages {
                lexical,
                vector,
                reranker,
            },
        })
    }

    async fn semantic_search(
        &self,
        query: &str,
        eligible_ids: &BTreeSet<String>,
        limit: usize,
    ) -> RetrievalResult<(
        Vec<crate::fusion::RankedInput>,
        RetrievalProviderStageStatus,
    )> {
        let Some(provider) = self.options.vector_provider else {
            return Ok((
                vec![],
                stage(
                    RetrievalProviderStageKind::None,
                    RetrievalStageState::Disabled,
                    &["VECTOR_DISABLED"],
                    None,
                ),
            ));
        };
        let identity = provider.identity();
        if self.store.manifest.embedding_provider_id.is_none() {
            return Ok((
                vec![],
                stage(
                    identity.kind,
                    RetrievalStageState::Degraded,
                    &["VECTOR_PROJECTION_UNAVAILABLE"],
                    Some(identity),
                ),
            ));
        }
        if self.store.manifest.embedding_provider_id.as_deref() != Some(&identity.provider_id)
            || self.store.manifest.embedding_model_id.as_deref() != Some(&identity.model_id)
            || self.store.manifest.embedding_dimensions != Some(identity.dimensions)
        {
            return Err(RetrievalError::ProjectionMismatch(
                "VECTOR_SPACE_MISMATCH_REBUILD_REQUIRED".to_owned(),
            ));
        }
        let request_id = sha256(query.as_bytes());
        let texts = [query.to_owned()];
        let attempt = bounded_provider_call(
            provider.embed(&request_id, &texts),
            identity.timeout_ms,
            "EMBEDDING_QUERY_TIMEOUT",
        )
        .await;
        let Ok(vectors) = attempt else {
            return Ok((
                vec![],
                stage(
                    identity.kind,
                    RetrievalStageState::Degraded,
                    &["VECTOR_UNAVAILABLE"],
                    Some(identity),
                ),
            ));
        };
        if vectors.len() != 1
            || vectors[0].len() != identity.dimensions as usize
            || vectors[0].iter().any(|value| !value.is_finite())
        {
            return Ok((
                vec![],
                stage(
                    identity.kind,
                    RetrievalStageState::Degraded,
                    &["VECTOR_UNAVAILABLE"],
                    Some(identity),
                ),
            ));
        }
        let query_vector = vectors[0]
            .iter()
            .copied()
            .map(f64::from)
            .collect::<Vec<_>>();
        let semantic = self.store.vector_search(
            &query_vector,
            eligible_ids,
            limit,
            &identity.provider_id,
            &identity.model_id,
        )?;
        Ok((
            semantic,
            stage(
                identity.kind,
                RetrievalStageState::Active,
                &[],
                Some(identity),
            ),
        ))
    }

    async fn rerank(
        &self,
        query: &str,
        eligible: &[RetrievalChunk],
        fused: &[RankedCandidate],
    ) -> (
        Vec<RankedCandidate>,
        RetrievalProviderStageStatus,
        BTreeMap<String, f64>,
        BTreeMap<String, u32>,
    ) {
        let Some(provider) = self.options.rerank_provider else {
            return (
                fused.to_vec(),
                stage(
                    RetrievalProviderStageKind::None,
                    RetrievalStageState::Skipped,
                    &["RERANKER_NOT_CONFIGURED"],
                    None,
                ),
                BTreeMap::new(),
                BTreeMap::new(),
            );
        };
        let identity = provider.identity();
        let by_id = eligible
            .iter()
            .map(|chunk| (chunk.chunk_id.as_str(), chunk))
            .collect::<BTreeMap<_, _>>();
        let inputs = fused
            .iter()
            .filter_map(|candidate| {
                by_id
                    .get(candidate.chunk_id.as_str())
                    .map(|chunk| crate::providers::RerankInput {
                        chunk_id: candidate.chunk_id.clone(),
                        text: chunk.text.clone(),
                    })
            })
            .collect::<Vec<_>>();
        let request_id = sha256(format!("rerank\0{query}").as_bytes());
        let response = bounded_provider_call(
            provider.rerank(&request_id, query, &inputs),
            identity.timeout_ms,
            "RERANKER_TIMEOUT",
        )
        .await;
        let Ok(response) = response else {
            return degraded_rerank(fused, identity);
        };
        if response.len() != fused.len() {
            return degraded_rerank(fused, identity);
        }
        let expected = fused
            .iter()
            .map(|candidate| candidate.chunk_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut scores = BTreeMap::new();
        for item in response {
            if !expected.contains(item.chunk_id.as_str())
                || !item.score.is_finite()
                || scores.insert(item.chunk_id, item.score).is_some()
            {
                return degraded_rerank(fused, identity);
            }
        }
        let mut ordered = fused.to_vec();
        ordered.sort_by(|a, b| {
            scores[&b.chunk_id]
                .total_cmp(&scores[&a.chunk_id])
                .then_with(|| b.fusion_score.total_cmp(&a.fusion_score))
                .then_with(|| code_unit_compare(&a.chunk_id, &b.chunk_id))
        });
        let ranks = ordered
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.chunk_id.clone(), index as u32 + 1))
            .collect();
        (
            ordered,
            stage(
                identity.kind,
                RetrievalStageState::Active,
                &[],
                Some(identity),
            ),
            scores,
            ranks,
        )
    }

    fn parent_context(
        &self,
        winner: &RetrievalChunk,
        eligible_by_id: &BTreeMap<&str, &RetrievalChunk>,
        source_bytes: &BTreeMap<String, Vec<u8>>,
        provenance_by_source: &BTreeMap<String, GkxStoredSourceProvenance>,
        temporal_by_source: &BTreeMap<String, &GkxAuthorizedTemporalSource>,
        normalized_as_of: Option<&str>,
    ) -> RetrievalResult<Option<GkxRetrievalParentContext>> {
        let Some(parent_id) = winner.parent_chunk_id.as_deref() else {
            return Ok(None);
        };
        let Some(parent) = eligible_by_id.get(parent_id).copied() else {
            return Ok(None);
        };
        if parent.text.len() > self.options.max_parent_bytes {
            return Ok(None);
        }
        let live = source_bytes.get(&parent.source_path).ok_or_else(|| {
            RetrievalError::ProjectionMismatch("verified parent bytes missing".to_owned())
        })?;
        let stored = provenance_by_source.get(&parent.source_id).ok_or_else(|| {
            RetrievalError::ProjectionMismatch("parent provenance missing".to_owned())
        })?;
        let temporal = temporal_by_source
            .get(&parent.source_id)
            .copied()
            .ok_or_else(|| {
                RetrievalError::ProjectionMismatch("parent temporal row missing".to_owned())
            })?;
        let eligible_ids = eligible_by_id
            .keys()
            .map(|value| (*value).to_owned())
            .collect::<BTreeSet<_>>();
        let scoped_parent = authorized_result_chunk(parent, &eligible_ids, temporal);
        Ok(Some(GkxRetrievalParentContext {
            chunk_id: parent.chunk_id.clone(),
            text: parent.text.clone(),
            citation: verified_citation(parent, "", live)?,
            provenance: build_public_provenance(
                stored,
                &scoped_parent,
                temporal,
                normalized_as_of,
            )?,
        }))
    }
}

fn degraded_rerank(
    fused: &[RankedCandidate],
    identity: &crate::providers::RerankProviderIdentity,
) -> (
    Vec<RankedCandidate>,
    RetrievalProviderStageStatus,
    BTreeMap<String, f64>,
    BTreeMap<String, u32>,
) {
    (
        fused.to_vec(),
        stage(
            identity.kind,
            RetrievalStageState::Degraded,
            &["RERANKER_UNAVAILABLE"],
            Some(identity),
        ),
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

fn safe_metadata(metadata: &RetrievalChunkMetadata) -> RetrievalChunkMetadata {
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
        extra: BTreeMap::new(),
    }
}

fn source_policy_record(source: &GkxCandidateSource) -> GkxSourcePolicyRecord {
    GkxSourcePolicyRecord {
        source_id: source.source_id.clone(),
        source_path: source.source_path.clone(),
        source_digest: source.source_digest.clone(),
        lineage_id: (),
        valid_from: source.valid_from.clone(),
        valid_to: None,
        supersedes: vec![],
        superseded_by: vec![],
        metadata: safe_metadata(&source.source_metadata),
    }
}

fn source_filter_record(source: &GkxSourcePolicyRecord) -> RetrievalChunk {
    RetrievalChunk {
        chunk_id: sha256(format!("policy\0{}", source.source_id).as_bytes()),
        source_id: source.source_id.clone(),
        source_path: source.source_path.clone(),
        source_digest: source.source_digest.clone(),
        heading_path: vec![],
        heading_depth: 0,
        ordinal_within_source: 1,
        structural_position: "policy".to_owned(),
        part_ordinal: 1,
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
        content_digest: sha256(b"policy"),
        text: String::new(),
        token_count: 0,
        parent_chunk_id: None,
        lineage_id: None,
        valid_from: source.valid_from.clone(),
        valid_to: None,
        supersedes: vec![],
        superseded_by: vec![],
        metadata: source.metadata.clone(),
    }
}

fn chunk_policy_record(chunk: &RetrievalChunk) -> RetrievalChunk {
    let mut record = chunk.clone();
    record.lineage_id = None;
    record.valid_to = None;
    record.supersedes.clear();
    record.superseded_by.clear();
    record.metadata = safe_metadata(&chunk.metadata);
    record
}

fn source_policy_allows(
    policy: &GkxSourceDiscoverabilityPolicy,
    source: &GkxSourcePolicyRecord,
) -> bool {
    catch_unwind(AssertUnwindSafe(|| policy(source)))
        .ok()
        .and_then(Result::ok)
        == Some(DiscoverabilityDecision::Allow)
}

fn chunk_policy_allows(policy: &DiscoverabilityPolicy, chunk: &RetrievalChunk) -> bool {
    catch_unwind(AssertUnwindSafe(|| policy(chunk)))
        .ok()
        .and_then(Result::ok)
        == Some(DiscoverabilityDecision::Allow)
}

#[derive(Clone)]
struct ResultCoordinate {
    projection_id: String,
    projection_digest: String,
}

#[derive(Serialize)]
struct ResultCoordinateSource<'a> {
    source_id: &'a str,
    source_path: &'a str,
    source_digest: &'a str,
    metadata: RetrievalChunkMetadata,
    assertion_time: &'a Option<String>,
    assertion_origin: Option<crate::provenance::GkxAssertionOrigin>,
    valid_from: &'a Option<String>,
    valid_to: &'a Option<String>,
    validity_origin: crate::provenance::GkxValidityOrigin,
    lineage_id: (),
    supersedes: &'a [String],
    superseded_by: &'a [String],
    temporal_state: crate::provenance::GkxTemporalViewState,
    ledger_binding_verified: bool,
    lineage_neutral: bool,
}

#[derive(Serialize)]
struct ResultCoordinateEnvelope<'a> {
    contract_version: &'a str,
    engine_version: &'a str,
    projection_schema_version: u32,
    provenance_contract_version: &'a str,
    gkx_standard_commit: &'a str,
    gkx_projection_profile: &'a str,
    vault_id: &'a str,
    configuration_digest: &'a str,
    policy_digest: &'a str,
    chunker_version: &'a str,
    tokenizer_version: &'a str,
    lexical_backend: crate::contract::SqliteLexicalBackend,
    embedding_provider_id: &'a Option<String>,
    embedding_model_id: &'a Option<String>,
    embedding_dimensions: Option<u32>,
    sources: &'a [ResultCoordinateSource<'a>],
}

fn result_projection_coordinate(
    store: &GkxSqliteRetrievalStore,
    authorized: &[GkxStoredSourceProvenance],
    temporal: &[GkxAuthorizedTemporalSource],
) -> RetrievalResult<ResultCoordinate> {
    let temporal = temporal
        .iter()
        .map(|source| (source.source_id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    let mut sources = authorized
        .iter()
        .map(|source| {
            let scoped = temporal.get(source.source_id.as_str()).ok_or_else(|| {
                RetrievalError::ProjectionMismatch("result temporal binding missing".to_owned())
            })?;
            Ok(ResultCoordinateSource {
                source_id: &source.source_id,
                source_path: &source.source_path,
                source_digest: &source.source_digest,
                metadata: safe_metadata(&source.source_metadata),
                assertion_time: &source.assertion_time,
                assertion_origin: source.assertion_origin,
                valid_from: &scoped.valid_from,
                valid_to: &scoped.valid_to,
                validity_origin: source.validity_origin,
                lineage_id: (),
                supersedes: &scoped.supersedes,
                superseded_by: &scoped.superseded_by,
                temporal_state: scoped.temporal_state,
                ledger_binding_verified: false,
                lineage_neutral: scoped.supersedes.is_empty() && scoped.superseded_by.is_empty(),
            })
        })
        .collect::<RetrievalResult<Vec<_>>>()?;
    sources.sort_by(|a, b| code_unit_compare(a.source_id, b.source_id));
    let manifest = &store.manifest;
    let projection_digest = canonical_digest(&ResultCoordinateEnvelope {
        contract_version: RETRIEVAL_LINEAGE_CONTRACT,
        engine_version: &manifest.engine_version,
        projection_schema_version: manifest.projection_schema_version,
        provenance_contract_version: &manifest.provenance_contract_version,
        gkx_standard_commit: &manifest.gkx_standard_commit,
        gkx_projection_profile: &manifest.gkx_projection_profile,
        vault_id: &manifest.vault_id,
        configuration_digest: &manifest.configuration_digest,
        policy_digest: &manifest.policy_digest,
        chunker_version: &manifest.chunker_version,
        tokenizer_version: &manifest.tokenizer_version,
        lexical_backend: manifest.lexical_backend,
        embedding_provider_id: &manifest.embedding_provider_id,
        embedding_model_id: &manifest.embedding_model_id,
        embedding_dimensions: manifest.embedding_dimensions,
        sources: &sources,
    })?;
    Ok(ResultCoordinate {
        projection_id: format!("retrieval:{}", &projection_digest[7..31]),
        projection_digest,
    })
}

fn authorized_result_chunk(
    chunk: &RetrievalChunk,
    eligible_ids: &BTreeSet<String>,
    temporal: &GkxAuthorizedTemporalSource,
) -> RetrievalChunk {
    let mut result = chunk.clone();
    if result
        .parent_chunk_id
        .as_ref()
        .is_some_and(|parent| !eligible_ids.contains(parent))
    {
        result.parent_chunk_id = None;
    }
    result.valid_from = temporal.valid_from.clone();
    result.valid_to = temporal.valid_to.clone();
    result.supersedes = temporal.supersedes.clone();
    result.superseded_by = temporal.superseded_by.clone();
    result.metadata = safe_metadata(&chunk.metadata);
    result
}

fn lineage_query_digest(query: &str, normalized_as_of: Option<&str>) -> RetrievalResult<String> {
    #[derive(Serialize)]
    struct QueryDigest<'a> {
        as_of: Option<&'a str>,
        query: &'a str,
    }
    canonical_digest(&QueryDigest {
        as_of: normalized_as_of,
        query,
    })
}

#[allow(clippy::too_many_arguments)]
fn assemble_result(
    query: &str,
    normalized_as_of: Option<&str>,
    coordinate: &ResultCoordinate,
    freshness: GkxProjectionFreshness,
    existing_hits: &[GkxRetrievalHit],
    candidate: Option<&GkxRetrievalHit>,
    filters: &crate::contract::RetrievalFilters,
    eligible_count: usize,
    coverage: TemporalCoverage,
    stages: &RetrievalSearchStages,
) -> RetrievalResult<GkxAuthorizedRetrievalSearchResult> {
    let mut hits = existing_hits.to_vec();
    if let Some(candidate) = candidate {
        hits.push(candidate.clone());
    }
    let mut confidence = assess_retrieval_confidence(
        &hits
            .iter()
            .map(|hit| hit.stage_scores.clone())
            .collect::<Vec<_>>(),
        &stages.vector,
        &stages.reranker,
        eligible_count,
        freshness == GkxProjectionFreshness::Stale,
    );
    if freshness == GkxProjectionFreshness::Unverified {
        confidence.low_confidence = true;
        if confidence.level != crate::contract::ConfidenceLevel::Insufficient {
            confidence.level = crate::contract::ConfidenceLevel::Low;
        }
        confidence
            .reason_codes
            .push("PROJECTION_FRESHNESS_UNVERIFIED".to_owned());
        confidence
            .reason_codes
            .sort_by(|a, b| code_unit_compare(a, b));
        confidence.reason_codes.dedup();
    }
    Ok(GkxAuthorizedRetrievalSearchResult {
        contract_version: RETRIEVAL_LINEAGE_CONTRACT.to_owned(),
        query_digest: lineage_query_digest(query, normalized_as_of)?,
        projection_id: coordinate.projection_id.clone(),
        projection_digest: coordinate.projection_digest.clone(),
        projection_freshness: freshness,
        hits,
        confidence,
        temporal: GkxTemporalResultState {
            as_of: normalized_as_of.map(str::to_owned),
            coverage,
            reason_codes: vec![],
        },
        applied_filters: applied_filter_names(filters),
        eligible_result_count: u32::try_from(eligible_count)
            .map_err(|_| RetrievalError::InvalidConfig("too many eligible chunks".to_owned()))?,
        stages: stages.clone(),
    })
}

trait LexicalBackendStageKind {
    fn into_stage_kind(self) -> RetrievalProviderStageKind;
}

impl LexicalBackendStageKind for crate::contract::SqliteLexicalBackend {
    fn into_stage_kind(self) -> RetrievalProviderStageKind {
        match self {
            Self::SqliteFts5 => RetrievalProviderStageKind::SqliteFts5,
            Self::SqliteLexicalScan => RetrievalProviderStageKind::SqliteLexicalScan,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::future::Future;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use futures_executor::block_on;
    use futures_util::task::noop_waker_ref;
    use tempfile::tempdir;

    use super::*;
    use crate::candidate::{
        GkxCandidateCategory, GkxCandidateDeclaration, GkxCandidateOrigin, GkxResolutionBasis,
        GkxResolutionTier,
    };
    use crate::contract::{GkxSensitivity, RetrievalProviderStageKind};
    use crate::lineage_store::test_support::{chunks, generation, source, NEW, OLD};
    use crate::lineage_store::{activate_gkx_retrieval_generation, build_gkx_retrieval_generation};
    use crate::providers::{
        ProviderFuture, RerankProviderIdentity, RerankScore, VectorProviderIdentity,
    };

    #[derive(serde::Deserialize)]
    struct Draft2Fixture {
        as_of_grammar: Vec<AsOfCase>,
        intervals: Vec<IntervalFixture>,
        point_in_time: Vec<PointInTimeFixture>,
        stored_provenance_assertion_validity: AssertionValidityFixture,
        branched_lineage: PendingDecision,
        authorization_dependent_diagnostics: AuthorizationMatrix,
        unicode_citation: UnicodeCitation,
        executable_projection: ExecutableProjection,
    }

    #[derive(serde::Deserialize)]
    struct IntervalFixture {
        source_id: String,
        valid_from: Option<String>,
        valid_to: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct PointInTimeFixture {
        as_of: String,
        expected_valid: Vec<String>,
        expected_historical: Vec<String>,
        expected_future: Vec<String>,
        expected_unknown: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct AssertionValidityFixture {
        accepted: Vec<AssertionValidityCase>,
        rejected: Vec<AssertionValidityCase>,
    }

    #[derive(serde::Deserialize)]
    struct AssertionValidityCase {
        assertion_time: Option<String>,
        valid_from: Option<String>,
        validity_origin: crate::provenance::GkxValidityOrigin,
    }

    #[derive(serde::Deserialize)]
    struct AsOfCase {
        value: String,
        accepted: bool,
        normalized: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct PendingDecision {
        status: String,
        frozen: bool,
    }

    #[derive(serde::Deserialize)]
    struct Draft2Contract {
        cross_record_topology_authority: CrossRecordTopologyAuthority,
    }

    #[derive(serde::Deserialize)]
    struct CrossRecordTopologyAuthority {
        status: String,
        frozen: bool,
        matrix_ids: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct AuthorizationMatrix {
        status: String,
        frozen: bool,
        matrix: Vec<AuthorizationMatrixRow>,
        differential_cases: Vec<AuthorizationDifferential>,
    }

    #[derive(serde::Deserialize)]
    struct AuthorizationMatrixRow {
        id: String,
        scope: String,
        required_owner_decision: bool,
        frozen: bool,
    }

    #[derive(serde::Deserialize)]
    struct AuthorizationDifferential {
        normative_retrieval_outcome: Option<serde_json::Value>,
        frozen: bool,
    }

    #[derive(serde::Deserialize)]
    struct UnicodeCitation {
        source_text: String,
        query: String,
        expected_span: crate::contract::MatchedSpan,
    }

    #[derive(serde::Deserialize)]
    struct ExecutableProjection {
        input_files: Vec<ExecutableInputFile>,
        expected_candidate_sources: Vec<crate::candidate::GkxCandidateSource>,
        expected_candidate_declarations: Vec<crate::candidate::GkxCandidateDeclaration>,
        expected_stored_provenance: Vec<GkxStoredSourceProvenance>,
        generation_input: ExecutableGenerationInput,
        expected_manifest: serde_json::Value,
        expected_fts5_manifest: serde_json::Value,
        search_request: GkxRetrievalSearchRequest,
        expected_result: serde_json::Value,
        expected_fts5_result: serde_json::Value,
    }

    #[derive(serde::Deserialize)]
    struct ExecutableInputFile {
        relative_path: String,
        content: String,
    }

    #[derive(serde::Deserialize)]
    struct ExecutableGenerationInput {
        vault_id: String,
        source_snapshot_digest: String,
        configuration_digest: String,
        policy_digest: String,
        embedding_eligible_candidate_chunk_keys: Vec<String>,
    }

    fn final_fixture() -> Draft2Fixture {
        serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
            "conformance-fixture.json"
        )))
        .unwrap()
    }

    fn final_contract() -> Draft2Contract {
        serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
            "contract.json"
        )))
        .unwrap()
    }

    fn with_assertion_validity_case(
        template: &GkxStoredSourceProvenance,
        case: &AssertionValidityCase,
    ) -> GkxStoredSourceProvenance {
        let mut candidate = template.clone();
        candidate.assertion_time = case.assertion_time.clone();
        candidate.assertion_origin = case
            .assertion_time
            .as_ref()
            .map(|_| crate::provenance::GkxAssertionOrigin::GkxCreatedAt);
        candidate.source_metadata.authored_at = case.assertion_time.clone();
        candidate.valid_from = case.valid_from.clone();
        candidate.valid_to = None;
        candidate.validity_origin = case.validity_origin;
        candidate.temporal_state = if case.valid_from.is_some()
            && case.validity_origin != crate::provenance::GkxValidityOrigin::Unknown
        {
            crate::provenance::GkxTemporalState::Current
        } else {
            crate::provenance::GkxTemporalState::Unknown
        };
        let mut reasons = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            if candidate.lineage_neutral {
                "LINEAGE_NEUTRAL".to_owned()
            } else {
                "LINEAGE_PARTICIPANT".to_owned()
            },
            match case.validity_origin {
                crate::provenance::GkxValidityOrigin::GkxAuthoredTimestamp => {
                    "VALIDITY_FROM_GKX_AUTHORED_TIMESTAMP"
                }
                crate::provenance::GkxValidityOrigin::SourceCreatedTime => {
                    "VALIDITY_FROM_SOURCE_CREATED_TIME"
                }
                crate::provenance::GkxValidityOrigin::SourceModifiedTime => {
                    "VALIDITY_FROM_SOURCE_MODIFIED_TIME"
                }
                crate::provenance::GkxValidityOrigin::ProjectionReferenceTime => {
                    "VALIDITY_FROM_PROJECTION_REFERENCE_TIME"
                }
                crate::provenance::GkxValidityOrigin::Unknown => "VALIDITY_UNKNOWN",
            }
            .to_owned(),
        ];
        if case.assertion_time.is_none() {
            reasons.push("ASSERTION_TIME_UNAVAILABLE".to_owned());
        }
        reasons.sort_by(|left, right| code_unit_compare(left, right));
        candidate.reason_codes = reasons;
        let mut value = serde_json::to_value(&candidate).unwrap();
        value.as_object_mut().unwrap().remove("provenance_digest");
        candidate.provenance_digest = canonical_digest(&value).unwrap();
        candidate
    }

    fn interval_provenance(
        template: &GkxStoredSourceProvenance,
        interval: &IntervalFixture,
        ordinal: usize,
    ) -> GkxStoredSourceProvenance {
        let mut candidate = template.clone();
        candidate.source_id = interval.source_id.clone();
        candidate.source_path = format!("interval-{ordinal}.md");
        candidate.source_digest = sha256(candidate.source_path.as_bytes());
        candidate.source_metadata.title = Some(format!("Interval {ordinal}"));
        candidate.valid_from = interval
            .valid_from
            .as_deref()
            .map(normalize_retrieval_as_of)
            .transpose()
            .unwrap();
        candidate.valid_to = interval
            .valid_to
            .as_deref()
            .map(normalize_retrieval_as_of)
            .transpose()
            .unwrap();
        candidate.authored_supersedes.clear();
        candidate.authored_superseded_by.clear();
        candidate.resolved_supersedes.clear();
        candidate.resolved_superseded_by.clear();
        candidate.lineage_neutral = true;
        if let Some(valid_from) = &candidate.valid_from {
            candidate.assertion_time = Some(valid_from.clone());
            candidate.assertion_origin = Some(crate::provenance::GkxAssertionOrigin::GkxCreatedAt);
            candidate.source_metadata.authored_at = Some(valid_from.clone());
            candidate.validity_origin = crate::provenance::GkxValidityOrigin::GkxAuthoredTimestamp;
            candidate.temporal_state = if candidate.valid_to.is_some() {
                crate::provenance::GkxTemporalState::Historical
            } else {
                crate::provenance::GkxTemporalState::Current
            };
        } else {
            // A canonical assertion can exist while lineage validity remains
            // honestly unknown; no interval is synthesized from it.
            candidate.validity_origin = crate::provenance::GkxValidityOrigin::Unknown;
            candidate.temporal_state = crate::provenance::GkxTemporalState::Unknown;
        }
        let mut reasons = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            "LINEAGE_NEUTRAL".to_owned(),
            match candidate.validity_origin {
                crate::provenance::GkxValidityOrigin::GkxAuthoredTimestamp => {
                    "VALIDITY_FROM_GKX_AUTHORED_TIMESTAMP"
                }
                crate::provenance::GkxValidityOrigin::SourceCreatedTime => {
                    "VALIDITY_FROM_SOURCE_CREATED_TIME"
                }
                crate::provenance::GkxValidityOrigin::SourceModifiedTime => {
                    "VALIDITY_FROM_SOURCE_MODIFIED_TIME"
                }
                crate::provenance::GkxValidityOrigin::ProjectionReferenceTime => {
                    "VALIDITY_FROM_PROJECTION_REFERENCE_TIME"
                }
                crate::provenance::GkxValidityOrigin::Unknown => "VALIDITY_UNKNOWN",
            }
            .to_owned(),
        ];
        if candidate.assertion_time.is_none() {
            reasons.push("ASSERTION_TIME_UNAVAILABLE".to_owned());
        }
        reasons.sort_by(|left, right| code_unit_compare(left, right));
        candidate.reason_codes = reasons;
        let mut value = serde_json::to_value(&candidate).unwrap();
        value.as_object_mut().unwrap().remove("provenance_digest");
        candidate.provenance_digest = canonical_digest(&value).unwrap();
        candidate
    }

    struct CountingProvider {
        identity: VectorProviderIdentity,
        identity_calls: AtomicUsize,
        calls: AtomicUsize,
        texts: AtomicUsize,
    }

    struct CountingReranker {
        identity: RerankProviderIdentity,
        calls: AtomicUsize,
    }

    struct PendingProvider {
        identity: VectorProviderIdentity,
        calls: AtomicUsize,
    }

    impl CountingProvider {
        fn new() -> Self {
            Self {
                identity: VectorProviderIdentity {
                    kind: RetrievalProviderStageKind::Mcp,
                    provider_id: "fixture-provider".to_owned(),
                    model_id: "fixture-2d".to_owned(),
                    dimensions: 2,
                    timeout_ms: 1_000,
                    configuration_digest: crate::digest::sha256(b"fixture-provider-config"),
                },
                identity_calls: AtomicUsize::new(0),
                calls: AtomicUsize::new(0),
                texts: AtomicUsize::new(0),
            }
        }
    }

    impl VectorProvider for CountingProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            self.identity_calls.fetch_add(1, Ordering::SeqCst);
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.texts.fetch_add(texts.len(), Ordering::SeqCst);
            Box::pin(async move { Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect()) })
        }
    }

    impl VectorProvider for PendingProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            _texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }
    }

    impl RerankProvider for CountingReranker {
        fn identity(&self) -> &RerankProviderIdentity {
            &self.identity
        }

        fn rerank<'a>(
            &'a self,
            _request_id: &'a str,
            _query: &'a str,
            _inputs: &'a [crate::providers::RerankInput],
        ) -> ProviderFuture<'a, Vec<RerankScore>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    struct MemoryReader(BTreeMap<String, Vec<u8>>);

    impl SourceReader for MemoryReader {
        fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>> {
            self.0.get(source_path).cloned().ok_or_else(|| {
                RetrievalError::ProjectionMismatch("fixture source missing".to_owned())
            })
        }
    }

    struct CountingReader {
        bytes: BTreeMap<String, Vec<u8>>,
        calls: AtomicUsize,
    }

    impl SourceReader for CountingReader {
        fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.bytes.get(source_path).cloned().ok_or_else(|| {
                RetrievalError::ProjectionMismatch("fixture source missing".to_owned())
            })
        }
    }

    fn request() -> GkxRetrievalSearchRequest {
        GkxRetrievalSearchRequest {
            query: "Policy".to_owned(),
            as_of: Some("2026-08-15T00:00Z".to_owned()),
            limit: Some(5),
            lexical_top_k: None,
            semantic_top_k: None,
            filters: None,
            rrf_k: None,
            mmr: None,
            mmr_lambda: None,
            parent_expansion: None,
            parent_expansion_max_child_tokens: None,
        }
    }

    #[test]
    fn hidden_candidate_equals_absence_and_visible_conflict_precedes_live_and_provider_work() {
        let root = tempdir().unwrap();
        let a_text = "# A\nPolicy visible.\n";
        let b_text = "# B\nPolicy hidden.\n";
        let pairs = vec![
            source(
                OLD,
                "a.md",
                a_text,
                "2026-07-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            source(
                NEW,
                "b.md",
                b_text,
                "2026-08-01T00:00:00.000Z",
                GkxSensitivity::Secret,
            ),
        ];
        let public = chunks(&pairs.iter().map(|pair| pair.0.clone()).collect::<Vec<_>>());
        let eligible = public
            .iter()
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<Vec<_>>();
        let vectors = public
            .iter()
            .map(|chunk| crate::sqlite_store::StoredVector {
                chunk_id: chunk.chunk_id.clone(),
                vector: vec![1.0, 0.0],
            })
            .collect::<Vec<_>>();
        let mut hidden = generation(
            root.path().join("hidden"),
            pairs,
            eligible,
            vectors,
            Some(("fixture-provider", "fixture-2d", 2)),
        );
        let hidden_record = hidden
            .candidate_sources
            .iter()
            .find(|item| item.source_id == NEW)
            .unwrap()
            .record_key
            .clone();
        let candidate_source = hidden
            .candidate_sources
            .iter_mut()
            .find(|item| item.record_key == hidden_record)
            .unwrap();
        candidate_source.source_id = OLD.to_owned();
        candidate_source.candidate_digest = candidate_source.expected_digest().unwrap();
        let old_hidden_keys = hidden
            .candidate_chunks
            .iter()
            .filter(|item| item.record_key == hidden_record)
            .map(|item| item.candidate_chunk_key.clone())
            .collect::<BTreeSet<_>>();
        let duplicate_source = source(
            OLD,
            "b.md",
            b_text,
            "2026-08-01T00:00:00.000Z",
            GkxSensitivity::Secret,
        )
        .0;
        let mut replacement = chunks(&[duplicate_source]);
        let key_by_public = replacement
            .iter()
            .map(|chunk| {
                (
                    chunk.chunk_id.clone(),
                    crate::candidate::candidate_chunk_key(&hidden_record, &chunk.chunk_id).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        hidden
            .candidate_chunks
            .retain(|item| item.record_key != hidden_record);
        for chunk in &mut replacement {
            chunk.valid_to = None;
            chunk.lineage_id = None;
            chunk.supersedes.clear();
            chunk.superseded_by.clear();
            hidden
                .candidate_chunks
                .push(crate::candidate::GkxCandidateChunk {
                    candidate_chunk_key: key_by_public[&chunk.chunk_id].clone(),
                    record_key: hidden_record.clone(),
                    parent_candidate_chunk_key: chunk
                        .parent_chunk_id
                        .as_ref()
                        .and_then(|id| key_by_public.get(id).cloned()),
                    chunk: chunk.clone(),
                });
        }
        hidden
            .embedding_eligible_candidate_chunk_keys
            .retain(|key| !old_hidden_keys.contains(key));
        hidden
            .vectors
            .retain(|item| !old_hidden_keys.contains(&item.candidate_chunk_key));
        for key in key_by_public.values() {
            hidden
                .embedding_eligible_candidate_chunk_keys
                .push(key.clone());
            hidden.vectors.push(crate::candidate::GkxCandidateVector {
                candidate_chunk_key: key.clone(),
                vector: vec![1.0, 0.0],
            });
        }
        let mut absent = hidden.clone();
        absent.state_directory = root.path().join("absent");
        absent
            .candidate_sources
            .retain(|item| item.record_key != hidden_record);
        absent
            .candidate_declarations
            .retain(|item| item.source_record_key != hidden_record);
        let removed_keys = absent
            .candidate_chunks
            .iter()
            .filter(|item| item.record_key == hidden_record)
            .map(|item| item.candidate_chunk_key.clone())
            .collect::<BTreeSet<_>>();
        absent
            .candidate_chunks
            .retain(|item| item.record_key != hidden_record);
        absent
            .embedding_eligible_candidate_chunk_keys
            .retain(|key| !removed_keys.contains(key));
        absent
            .vectors
            .retain(|item| !removed_keys.contains(&item.candidate_chunk_key));

        let hidden_built = build_gkx_retrieval_generation(hidden.clone()).unwrap();
        activate_gkx_retrieval_generation(&hidden.state_directory, &hidden_built).unwrap();
        let absent_built = build_gkx_retrieval_generation(absent.clone()).unwrap();
        activate_gkx_retrieval_generation(&absent.state_directory, &absent_built).unwrap();
        let hidden_reader = CountingReader {
            bytes: BTreeMap::from([("a.md".to_owned(), a_text.as_bytes().to_vec())]),
            calls: AtomicUsize::new(0),
        };
        let absent_reader = CountingReader {
            bytes: BTreeMap::from([("a.md".to_owned(), a_text.as_bytes().to_vec())]),
            calls: AtomicUsize::new(0),
        };
        let hidden_provider = CountingProvider::new();
        let absent_provider = CountingProvider::new();
        let deny_secret = |source: &GkxSourcePolicyRecord| {
            Ok(
                if source.metadata.sensitivity == Some(GkxSensitivity::Secret) {
                    DiscoverabilityDecision::Deny
                } else {
                    DiscoverabilityDecision::Allow
                },
            )
        };
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let hidden_service = GkxRetrievalCoordinator::open_active(
            &hidden.state_directory,
            options(
                &deny_secret,
                &allow_chunk,
                Some(&hidden_provider),
                &hidden_reader,
            ),
        )
        .unwrap();
        let absent_service = GkxRetrievalCoordinator::open_active(
            &absent.state_directory,
            options(
                &allow_source,
                &allow_chunk,
                Some(&absent_provider),
                &absent_reader,
            ),
        )
        .unwrap();
        let hidden_result = block_on(hidden_service.search(&request())).unwrap();
        let absent_result = block_on(absent_service.search(&request())).unwrap();
        assert_eq!(
            canonical_json(&hidden_result).unwrap(),
            canonical_json(&absent_result).unwrap()
        );
        assert_eq!(hidden_reader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(absent_reader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(hidden_provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(absent_provider.calls.load(Ordering::SeqCst), 1);

        let reads_before = hidden_reader.calls.load(Ordering::SeqCst);
        let provider_before = hidden_provider.calls.load(Ordering::SeqCst);
        let conflicting = GkxRetrievalCoordinator::open_active(
            &hidden.state_directory,
            options(
                &allow_source,
                &allow_chunk,
                Some(&hidden_provider),
                &hidden_reader,
            ),
        )
        .unwrap();
        assert!(
            matches!(block_on(conflicting.search(&request())),Err(RetrievalError::ProjectionMismatch(message)) if message=="RETRIEVAL_AUTHORIZED_VIEW_CONFLICT")
        );
        assert_eq!(hidden_reader.calls.load(Ordering::SeqCst), reads_before);
        assert_eq!(
            hidden_provider.calls.load(Ordering::SeqCst),
            provider_before
        );
    }

    fn options<'a>(
        source_policy: &'a GkxSourceDiscoverabilityPolicy,
        chunk_policy: &'a DiscoverabilityPolicy,
        vector_provider: Option<&'a dyn VectorProvider>,
        reader: &'a dyn SourceReader,
    ) -> GkxRetrievalCoordinatorOptions<'a> {
        GkxRetrievalCoordinatorOptions {
            runtime_policy_digest: crate::digest::sha256(b"phase2-policy"),
            source_discoverability_policy: source_policy,
            discoverability_policy: chunk_policy,
            vector_provider,
            rerank_provider: None,
            source_reader: reader,
            lineage_view_freshness: GkxProjectionFreshness::Fresh,
            max_parent_bytes: 4_096,
            max_result_bytes: 131_072,
        }
    }

    fn decision_a_receipt(
        source_record_key: &str,
        category: GkxCandidateCategory,
        field: &str,
        mut target_record_keys: Vec<String>,
    ) -> GkxCandidateDeclaration {
        target_record_keys.sort_by(|left, right| code_unit_compare(left, right));
        GkxCandidateDeclaration {
            source_record_key: source_record_key.to_owned(),
            category,
            field: field.to_owned(),
            origin: GkxCandidateOrigin::Authored,
            declaration_index: 0,
            raw_reference: "canonical-host-reference".to_owned(),
            resolution_tiers: vec![
                GkxResolutionTier {
                    basis: GkxResolutionBasis::UidExact,
                    candidate_record_keys: target_record_keys,
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::PathExact,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::PathWithoutExtensionExact,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::BasenameTitle,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::Alias,
                    candidate_record_keys: vec![],
                },
            ],
        }
    }

    fn activate_decision_a_input(
        input: crate::lineage_store::GkxRetrievalGenerationInput,
    ) -> PathBuf {
        let state = input.state_directory.clone();
        let built = build_gkx_retrieval_generation(input).unwrap();
        activate_gkx_retrieval_generation(&state, &built).unwrap();
        state
    }

    fn replace_candidate_record_envelope(
        input: &mut crate::lineage_store::GkxRetrievalGenerationInput,
        record_key: &str,
        replacement: crate::contract::GkxRetrievalSource,
    ) {
        let candidate = input
            .candidate_sources
            .iter_mut()
            .find(|source| source.record_key == record_key)
            .unwrap();
        candidate.source_id = replacement.source_id.clone();
        candidate.source_path = replacement.source_path.clone();
        candidate.source_digest = replacement.source_digest.clone();
        candidate.source_metadata = replacement.metadata.clone();
        candidate.assertion_time = replacement.temporal.assertion_time.clone();
        candidate.assertion_origin = candidate
            .assertion_time
            .as_ref()
            .map(|_| crate::provenance::GkxAssertionOrigin::GkxCreatedAt);
        candidate.valid_from = replacement.temporal.valid_from.clone();
        candidate.validity_origin = if candidate.valid_from.is_some() {
            crate::provenance::GkxValidityOrigin::GkxAuthoredTimestamp
        } else {
            crate::provenance::GkxValidityOrigin::Unknown
        };
        candidate.reason_codes = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            candidate.validity_origin.reason().to_owned(),
        ];
        if candidate.assertion_time.is_none() {
            candidate
                .reason_codes
                .push("ASSERTION_TIME_UNAVAILABLE".to_owned());
        }
        candidate
            .reason_codes
            .sort_by(|left, right| code_unit_compare(left, right));
        candidate.candidate_digest = candidate.expected_digest().unwrap();

        let removed_keys = input
            .candidate_chunks
            .iter()
            .filter(|chunk| chunk.record_key == record_key)
            .map(|chunk| chunk.candidate_chunk_key.clone())
            .collect::<BTreeSet<_>>();
        input
            .candidate_chunks
            .retain(|chunk| chunk.record_key != record_key);
        input
            .embedding_eligible_candidate_chunk_keys
            .retain(|key| !removed_keys.contains(key));
        input
            .vectors
            .retain(|vector| !removed_keys.contains(&vector.candidate_chunk_key));
        let mut nested = chunks(&[replacement]);
        let key_by_public = nested
            .iter()
            .map(|chunk| {
                (
                    chunk.chunk_id.clone(),
                    crate::candidate::candidate_chunk_key(record_key, &chunk.chunk_id).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for chunk in &mut nested {
            chunk.valid_to = None;
            chunk.lineage_id = None;
            chunk.supersedes.clear();
            chunk.superseded_by.clear();
            input
                .candidate_chunks
                .push(crate::candidate::GkxCandidateChunk {
                    candidate_chunk_key: key_by_public[&chunk.chunk_id].clone(),
                    record_key: record_key.to_owned(),
                    parent_candidate_chunk_key: chunk
                        .parent_chunk_id
                        .as_ref()
                        .and_then(|parent| key_by_public.get(parent).cloned()),
                    chunk: chunk.clone(),
                });
        }
        input.candidate_chunks.sort_by(|left, right| {
            code_unit_compare(&left.candidate_chunk_key, &right.candidate_chunk_key)
        });
    }

    fn mark_candidate_unknown(
        input: &mut crate::lineage_store::GkxRetrievalGenerationInput,
        record_key: &str,
    ) {
        let candidate = input
            .candidate_sources
            .iter_mut()
            .find(|source| source.record_key == record_key)
            .unwrap();
        candidate.valid_from = None;
        candidate.validity_origin = crate::provenance::GkxValidityOrigin::Unknown;
        candidate
            .reason_codes
            .retain(|reason| !reason.starts_with("VALIDITY_FROM_"));
        candidate.reason_codes.push("VALIDITY_UNKNOWN".to_owned());
        candidate
            .reason_codes
            .sort_by(|left, right| code_unit_compare(left, right));
        candidate.reason_codes.dedup();
        candidate.candidate_digest = candidate.expected_digest().unwrap();
        for chunk in input
            .candidate_chunks
            .iter_mut()
            .filter(|chunk| chunk.record_key == record_key)
        {
            chunk.chunk.valid_from = None;
        }
    }

    fn enable_fixture_vectors(input: &mut crate::lineage_store::GkxRetrievalGenerationInput) {
        input.embedding_provider_id = Some("fixture-provider".to_owned());
        input.embedding_model_id = Some("fixture-2d".to_owned());
        input.embedding_dimensions = Some(2);
        input.embedding_eligible_candidate_chunk_keys = input
            .candidate_chunks
            .iter()
            .map(|chunk| chunk.candidate_chunk_key.clone())
            .collect();
        input.vectors = input
            .candidate_chunks
            .iter()
            .map(|chunk| crate::candidate::GkxCandidateVector {
                candidate_chunk_key: chunk.candidate_chunk_key.clone(),
                vector: vec![1.0, 0.0],
            })
            .collect();
    }

    fn counting_reranker() -> CountingReranker {
        CountingReranker {
            identity: RerankProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "fixture-reranker".to_owned(),
                model_id: "fixture-reranker-v1".to_owned(),
                timeout_ms: 1_000,
                configuration_digest: sha256(b"fixture-reranker-config"),
            },
            calls: AtomicUsize::new(0),
        }
    }

    #[test]
    fn ratified_endpoint_declaration_and_topology_differentials_match_complete_results() {
        const THIRD: &str = "018f0000-0000-7000-8000-000000000303";
        let root = tempdir().unwrap();
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let deny_secret = |source: &GkxSourcePolicyRecord| {
            Ok(
                if source.metadata.sensitivity == Some(GkxSensitivity::Secret) {
                    DiscoverabilityDecision::Deny
                } else {
                    DiscoverabilityDecision::Allow
                },
            )
        };
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);

        let search = |state: &Path,
                      source_policy: &GkxSourceDiscoverabilityPolicy,
                      reader: &CountingReader|
         -> GkxAuthorizedRetrievalSearchResult {
            let service = GkxRetrievalCoordinator::open_active(
                state,
                options(source_policy, &allow_chunk, None, reader),
            )
            .unwrap();
            block_on(service.search(&request())).unwrap()
        };
        let assert_conflict_before_work = |state: &Path, reader: &CountingReader| {
            let provider = CountingProvider::new();
            let service = GkxRetrievalCoordinator::open_active(
                state,
                options(&allow_source, &allow_chunk, Some(&provider), reader),
            )
            .unwrap();
            assert!(matches!(
                block_on(service.search(&request())),
                Err(RetrievalError::ProjectionMismatch(message))
                    if message == "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT"
            ));
            assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
        };

        // Endpoint resolution: a denied or future same-tier target is exactly
        // absent; two authorized known targets are one generic conflict.
        let endpoint_base = vec![
            source(
                OLD,
                "target.md",
                "# Target\nPolicy target.\n",
                "2026-07-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            source(
                NEW,
                "successor.md",
                "# Successor\nPolicy successor.\n",
                "2026-08-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
        ];
        let endpoint_hidden = source(
            THIRD,
            "hidden-target.md",
            "# Hidden Target\nPolicy hidden.\n",
            "2026-07-02T00:00:00.000Z",
            GkxSensitivity::Secret,
        );
        let endpoint_future = source(
            THIRD,
            "future-target.md",
            "# Future Target\nPolicy future.\n",
            "2026-09-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let mut endpoint_absent = generation(
            root.path().join("endpoint-absent"),
            endpoint_base.clone(),
            vec![],
            vec![],
            None,
        );
        let endpoint_source_key = endpoint_absent
            .candidate_sources
            .iter()
            .find(|source| source.source_id == NEW)
            .unwrap()
            .record_key
            .clone();
        let endpoint_target_key = endpoint_absent
            .candidate_sources
            .iter()
            .find(|source| source.source_id == OLD)
            .unwrap()
            .record_key
            .clone();
        endpoint_absent.candidate_declarations = vec![decision_a_receipt(
            &endpoint_source_key,
            GkxCandidateCategory::Lineage,
            "supersedes",
            vec![endpoint_target_key],
        )];
        let endpoint_absent_state = activate_decision_a_input(endpoint_absent);
        let mut endpoint_hidden_input = generation(
            root.path().join("endpoint-hidden"),
            endpoint_base
                .clone()
                .into_iter()
                .chain([endpoint_hidden])
                .collect(),
            vec![],
            vec![],
            None,
        );
        let endpoint_hidden_source_key = endpoint_hidden_input
            .candidate_sources
            .iter()
            .find(|source| source.source_id == NEW)
            .unwrap()
            .record_key
            .clone();
        let endpoint_hidden_targets = endpoint_hidden_input
            .candidate_sources
            .iter()
            .filter(|source| source.source_id == OLD || source.source_id == THIRD)
            .map(|source| source.record_key.clone())
            .collect::<Vec<_>>();
        endpoint_hidden_input.candidate_declarations = vec![decision_a_receipt(
            &endpoint_hidden_source_key,
            GkxCandidateCategory::Lineage,
            "supersedes",
            endpoint_hidden_targets,
        )];
        let endpoint_hidden_state = activate_decision_a_input(endpoint_hidden_input);
        let mut endpoint_future_input = generation(
            root.path().join("endpoint-future"),
            endpoint_base
                .clone()
                .into_iter()
                .chain([endpoint_future])
                .collect(),
            vec![],
            vec![],
            None,
        );
        let endpoint_future_source_key = endpoint_future_input
            .candidate_sources
            .iter()
            .find(|source| source.source_id == NEW)
            .unwrap()
            .record_key
            .clone();
        let endpoint_future_targets = endpoint_future_input
            .candidate_sources
            .iter()
            .filter(|source| source.source_id == OLD || source.source_id == THIRD)
            .map(|source| source.record_key.clone())
            .collect::<Vec<_>>();
        endpoint_future_input.candidate_declarations = vec![decision_a_receipt(
            &endpoint_future_source_key,
            GkxCandidateCategory::Lineage,
            "supersedes",
            endpoint_future_targets,
        )];
        let endpoint_future_state = activate_decision_a_input(endpoint_future_input);
        let endpoint_bytes = BTreeMap::from([
            (
                "target.md".to_owned(),
                b"# Target\nPolicy target.\n".to_vec(),
            ),
            (
                "successor.md".to_owned(),
                b"# Successor\nPolicy successor.\n".to_vec(),
            ),
        ]);
        let endpoint_absent_reader = CountingReader {
            bytes: endpoint_bytes.clone(),
            calls: AtomicUsize::new(0),
        };
        let endpoint_hidden_reader = CountingReader {
            bytes: endpoint_bytes.clone(),
            calls: AtomicUsize::new(0),
        };
        let endpoint_future_reader = CountingReader {
            bytes: endpoint_bytes,
            calls: AtomicUsize::new(0),
        };
        let endpoint_baseline = search(
            &endpoint_absent_state,
            &allow_source,
            &endpoint_absent_reader,
        );
        assert_eq!(
            canonical_json(&search(
                &endpoint_hidden_state,
                &deny_secret,
                &endpoint_hidden_reader,
            ))
            .unwrap(),
            canonical_json(&endpoint_baseline).unwrap()
        );
        assert_eq!(
            canonical_json(&search(
                &endpoint_future_state,
                &allow_source,
                &endpoint_future_reader,
            ))
            .unwrap(),
            canonical_json(&endpoint_baseline).unwrap()
        );
        let endpoint_conflict_reader = CountingReader {
            bytes: BTreeMap::new(),
            calls: AtomicUsize::new(0),
        };
        assert_conflict_before_work(&endpoint_hidden_state, &endpoint_conflict_reader);

        // Declaration reconciliation: the conflicting authored inverse belongs
        // to the denied/future candidate and is absent from the scoped view.
        let declaration_base = vec![source(
            OLD,
            "declaration.md",
            "# Declaration\nPolicy declaration.\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        )];
        let declaration_hidden = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "hidden-declaration.md",
            "# Hidden Declaration\nPolicy hidden.\n",
            "2026-07-02T00:00:00.000Z",
            None,
            vec![],
            vec![OLD.to_owned()],
            GkxSensitivity::Secret,
        );
        let declaration_future = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "future-declaration.md",
            "# Future Declaration\nPolicy future.\n",
            "2026-09-01T00:00:00.000Z",
            None,
            vec![],
            vec![OLD.to_owned()],
            GkxSensitivity::Public,
        );
        let declaration_absent_state = activate_decision_a_input(generation(
            root.path().join("declaration-absent"),
            declaration_base.clone(),
            vec![],
            vec![],
            None,
        ));
        let declaration_hidden_state = activate_decision_a_input(generation(
            root.path().join("declaration-hidden"),
            declaration_base
                .clone()
                .into_iter()
                .chain([declaration_hidden])
                .collect(),
            vec![],
            vec![],
            None,
        ));
        let declaration_future_state = activate_decision_a_input(generation(
            root.path().join("declaration-future"),
            declaration_base
                .clone()
                .into_iter()
                .chain([declaration_future])
                .collect(),
            vec![],
            vec![],
            None,
        ));
        let declaration_bytes = BTreeMap::from([(
            "declaration.md".to_owned(),
            b"# Declaration\nPolicy declaration.\n".to_vec(),
        )]);
        let declaration_absent_reader = CountingReader {
            bytes: declaration_bytes.clone(),
            calls: AtomicUsize::new(0),
        };
        let declaration_hidden_reader = CountingReader {
            bytes: declaration_bytes.clone(),
            calls: AtomicUsize::new(0),
        };
        let declaration_future_reader = CountingReader {
            bytes: declaration_bytes,
            calls: AtomicUsize::new(0),
        };
        let declaration_baseline = search(
            &declaration_absent_state,
            &allow_source,
            &declaration_absent_reader,
        );
        assert_eq!(
            canonical_json(&search(
                &declaration_hidden_state,
                &deny_secret,
                &declaration_hidden_reader,
            ))
            .unwrap(),
            canonical_json(&declaration_baseline).unwrap()
        );
        assert_eq!(
            canonical_json(&search(
                &declaration_future_state,
                &allow_source,
                &declaration_future_reader,
            ))
            .unwrap(),
            canonical_json(&declaration_baseline).unwrap()
        );
        let declaration_conflict_reader = CountingReader {
            bytes: BTreeMap::new(),
            calls: AtomicUsize::new(0),
        };
        assert_conflict_before_work(&declaration_hidden_state, &declaration_conflict_reader);

        // Topology: a denied/future second successor is absent; two authorized
        // successors branch and fail before source or provider work.
        let topology_base = vec![
            source(
                OLD,
                "old.md",
                "# Old\nPolicy old.\n",
                "2026-07-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            crate::lineage_store::test_support::source_with_lineage(
                NEW,
                "new.md",
                "# New\nPolicy new.\n",
                "2026-08-01T00:00:00.000Z",
                None,
                vec![OLD.to_owned()],
                vec![],
                GkxSensitivity::Public,
            ),
        ];
        let topology_hidden = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "hidden-branch.md",
            "# Hidden Branch\nPolicy hidden.\n",
            "2026-08-02T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Secret,
        );
        let topology_future = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "future-branch.md",
            "# Future Branch\nPolicy future.\n",
            "2026-09-01T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Public,
        );
        let topology_absent_state = activate_decision_a_input(generation(
            root.path().join("topology-absent"),
            topology_base.clone(),
            vec![],
            vec![],
            None,
        ));
        let topology_hidden_state = activate_decision_a_input(generation(
            root.path().join("topology-hidden"),
            topology_base
                .clone()
                .into_iter()
                .chain([topology_hidden])
                .collect(),
            vec![],
            vec![],
            None,
        ));
        let topology_future_state = activate_decision_a_input(generation(
            root.path().join("topology-future"),
            topology_base
                .clone()
                .into_iter()
                .chain([topology_future])
                .collect(),
            vec![],
            vec![],
            None,
        ));
        let topology_bytes = BTreeMap::from([
            ("old.md".to_owned(), b"# Old\nPolicy old.\n".to_vec()),
            ("new.md".to_owned(), b"# New\nPolicy new.\n".to_vec()),
        ]);
        let topology_absent_reader = CountingReader {
            bytes: topology_bytes.clone(),
            calls: AtomicUsize::new(0),
        };
        let topology_hidden_reader = CountingReader {
            bytes: topology_bytes.clone(),
            calls: AtomicUsize::new(0),
        };
        let topology_future_reader = CountingReader {
            bytes: topology_bytes,
            calls: AtomicUsize::new(0),
        };
        let topology_baseline = search(
            &topology_absent_state,
            &allow_source,
            &topology_absent_reader,
        );
        assert_eq!(
            canonical_json(&search(
                &topology_hidden_state,
                &deny_secret,
                &topology_hidden_reader,
            ))
            .unwrap(),
            canonical_json(&topology_baseline).unwrap()
        );
        assert_eq!(
            canonical_json(&search(
                &topology_future_state,
                &allow_source,
                &topology_future_reader,
            ))
            .unwrap(),
            canonical_json(&topology_baseline).unwrap()
        );
        let topology_conflict_reader = CountingReader {
            bytes: BTreeMap::new(),
            calls: AtomicUsize::new(0),
        };
        assert_conflict_before_work(&topology_hidden_state, &topology_conflict_reader);
    }

    #[test]
    fn future_duplicate_identity_matches_physical_absence_through_complete_coordinator_result() {
        const FUTURE: &str = "018f0000-0000-7000-8000-000000000303";
        let root = tempdir().unwrap();
        let text = "# Current\nPolicy current.\n";
        let current = source(
            OLD,
            "current.md",
            text,
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let mut absent = generation(
            root.path().join("identity-absent"),
            vec![current.clone()],
            vec![],
            vec![],
            None,
        );
        enable_fixture_vectors(&mut absent);

        let future_placeholder = source(
            FUTURE,
            "future.md",
            "# Future\nPolicy future.\n",
            "2026-09-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let mut future = generation(
            root.path().join("identity-future"),
            vec![current, future_placeholder],
            vec![],
            vec![],
            None,
        );
        let future_key = future
            .candidate_sources
            .iter()
            .find(|source| source.source_id == FUTURE)
            .unwrap()
            .record_key
            .clone();
        replace_candidate_record_envelope(
            &mut future,
            &future_key,
            source(
                OLD,
                "current.md",
                text,
                "2026-09-01T00:00:00.000Z",
                GkxSensitivity::Public,
            )
            .0,
        );
        enable_fixture_vectors(&mut future);

        let absent_state = activate_decision_a_input(absent);
        let future_state = activate_decision_a_input(future);
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let absent_reader = CountingReader {
            bytes: BTreeMap::from([("current.md".to_owned(), text.as_bytes().to_vec())]),
            calls: AtomicUsize::new(0),
        };
        let future_reader = CountingReader {
            bytes: BTreeMap::from([("current.md".to_owned(), text.as_bytes().to_vec())]),
            calls: AtomicUsize::new(0),
        };
        let absent_provider = CountingProvider::new();
        let future_provider = CountingProvider::new();
        let absent_service = GkxRetrievalCoordinator::open_active(
            &absent_state,
            options(
                &allow_source,
                &allow_chunk,
                Some(&absent_provider),
                &absent_reader,
            ),
        )
        .unwrap();
        let future_service = GkxRetrievalCoordinator::open_active(
            &future_state,
            options(
                &allow_source,
                &allow_chunk,
                Some(&future_provider),
                &future_reader,
            ),
        )
        .unwrap();
        let absent_result = block_on(absent_service.search(&request())).unwrap();
        let future_result = block_on(future_service.search(&request())).unwrap();
        assert_eq!(
            canonical_json(&future_result).unwrap(),
            canonical_json(&absent_result).unwrap()
        );
        assert_eq!(absent_reader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(future_reader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(absent_provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(future_provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(absent_provider.texts.load(Ordering::SeqCst), 1);
        assert_eq!(future_provider.texts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_four_class_matrix_is_one_complete_insufficient_envelope_and_conflict_wins() {
        const THIRD: &str = "018f0000-0000-7000-8000-000000000303";
        const FOURTH: &str = "018f0000-0000-7000-8000-000000000404";
        let root = tempdir().unwrap();
        let a = source(
            OLD,
            "a.md",
            "# A\nPolicy A.\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let b = crate::lineage_store::test_support::source_with_lineage(
            NEW,
            "b.md",
            "# B\nPolicy B.\n",
            "2026-08-01T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Public,
        );
        let unknown = source(
            THIRD,
            "unknown.md",
            "# Unknown\nPolicy unknown.\n",
            "2026-07-02T00:00:00.000Z",
            GkxSensitivity::Public,
        );

        let make_base = |name: &str,
                         unknown_pair: (
            crate::contract::GkxRetrievalSource,
            GkxStoredSourceProvenance,
        )| {
            generation(
                root.path().join(name),
                vec![a.clone(), b.clone(), unknown_pair],
                vec![],
                vec![],
                None,
            )
        };

        let mut identity = make_base("unknown-identity", unknown.clone());
        let identity_unknown_key = identity
            .candidate_sources
            .iter()
            .find(|source| source.source_id == THIRD)
            .unwrap()
            .record_key
            .clone();
        replace_candidate_record_envelope(
            &mut identity,
            &identity_unknown_key,
            source(
                OLD,
                "a.md",
                "# A\nPolicy A.\n",
                "2026-07-02T00:00:00.000Z",
                GkxSensitivity::Public,
            )
            .0,
        );
        mark_candidate_unknown(&mut identity, &identity_unknown_key);

        let mut endpoint = make_base("unknown-endpoint", unknown.clone());
        let endpoint_unknown_key = endpoint
            .candidate_sources
            .iter()
            .find(|source| source.source_id == THIRD)
            .unwrap()
            .record_key
            .clone();
        let endpoint_a_key = endpoint
            .candidate_sources
            .iter()
            .find(|source| source.source_id == OLD)
            .unwrap()
            .record_key
            .clone();
        let endpoint_b_key = endpoint
            .candidate_sources
            .iter()
            .find(|source| source.source_id == NEW)
            .unwrap()
            .record_key
            .clone();
        endpoint.candidate_declarations = vec![decision_a_receipt(
            &endpoint_b_key,
            GkxCandidateCategory::Lineage,
            "supersedes",
            vec![endpoint_a_key, endpoint_unknown_key.clone()],
        )];
        mark_candidate_unknown(&mut endpoint, &endpoint_unknown_key);

        let unknown_declaration = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "unknown.md",
            "# Unknown\nPolicy unknown.\n",
            "2026-07-02T00:00:00.000Z",
            None,
            vec![],
            vec![OLD.to_owned()],
            GkxSensitivity::Public,
        );
        let mut declaration = make_base("unknown-declaration", unknown_declaration);
        let declaration_unknown_key = declaration
            .candidate_sources
            .iter()
            .find(|source| source.source_id == THIRD)
            .unwrap()
            .record_key
            .clone();
        mark_candidate_unknown(&mut declaration, &declaration_unknown_key);

        let unknown_topology = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "unknown.md",
            "# Unknown\nPolicy unknown.\n",
            "2026-07-02T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Public,
        );
        let mut topology = make_base("unknown-topology", unknown_topology);
        let topology_unknown_key = topology
            .candidate_sources
            .iter()
            .find(|source| source.source_id == THIRD)
            .unwrap()
            .record_key
            .clone();
        mark_candidate_unknown(&mut topology, &topology_unknown_key);

        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let mut expected = None;
        for mut input in [identity, endpoint, declaration, topology] {
            enable_fixture_vectors(&mut input);
            let state = activate_decision_a_input(input);
            let reader = CountingReader {
                bytes: BTreeMap::new(),
                calls: AtomicUsize::new(0),
            };
            let provider = CountingProvider::new();
            let reranker = counting_reranker();
            let mut coordinator_options =
                options(&allow_source, &allow_chunk, Some(&provider), &reader);
            coordinator_options.rerank_provider = Some(&reranker);
            let service =
                GkxRetrievalCoordinator::open_active(&state, coordinator_options).unwrap();
            let result = block_on(service.search(&request())).unwrap();
            assert_eq!(result.temporal.coverage, TemporalCoverage::Insufficient);
            assert_eq!(
                result.temporal.reason_codes,
                vec!["TEMPORAL_COVERAGE_INSUFFICIENT"]
            );
            assert!(result.hits.is_empty());
            assert_eq!(result.eligible_result_count, 0);
            assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
            assert_eq!(reranker.calls.load(Ordering::SeqCst), 0);
            let canonical = canonical_json(&result).unwrap();
            if let Some(expected) = &expected {
                assert_eq!(&canonical, expected);
            } else {
                expected = Some(canonical);
            }
        }

        let conflicting_branch = crate::lineage_store::test_support::source_with_lineage(
            THIRD,
            "branch.md",
            "# Branch\nPolicy branch.\n",
            "2026-08-02T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Public,
        );
        let fourth_unknown = source(
            FOURTH,
            "unknown-fourth.md",
            "# Unknown Fourth\nPolicy unknown.\n",
            "2026-07-03T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let mut conflict = generation(
            root.path().join("known-conflict-with-unknown"),
            vec![a, b, conflicting_branch, fourth_unknown],
            vec![],
            vec![],
            None,
        );
        let fourth_key = conflict
            .candidate_sources
            .iter()
            .find(|source| source.source_id == FOURTH)
            .unwrap()
            .record_key
            .clone();
        mark_candidate_unknown(&mut conflict, &fourth_key);
        enable_fixture_vectors(&mut conflict);
        let conflict_state = activate_decision_a_input(conflict);
        let reader = CountingReader {
            bytes: BTreeMap::new(),
            calls: AtomicUsize::new(0),
        };
        let provider = CountingProvider::new();
        let reranker = counting_reranker();
        let mut coordinator_options =
            options(&allow_source, &allow_chunk, Some(&provider), &reader);
        coordinator_options.rerank_provider = Some(&reranker);
        let service =
            GkxRetrievalCoordinator::open_active(&conflict_state, coordinator_options).unwrap();
        assert!(matches!(
            block_on(service.search(&request())),
            Err(RetrievalError::ProjectionMismatch(message))
                if message == "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT"
        ));
        assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
        assert_eq!(reranker.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unknown_validity_lineage_is_searchable_without_as_of_and_insufficient_with_as_of() {
        let root = tempdir().unwrap();
        let unknown_predecessor = source(
            OLD,
            "unknown-predecessor.md",
            "# Unknown predecessor\nPolicy predecessor.\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let known_successor = crate::lineage_store::test_support::source_with_lineage(
            NEW,
            "known-successor.md",
            "# Known successor\nPolicy successor.\n",
            "2026-08-01T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Public,
        );
        let mut predecessor_input = generation(
            root.path().join("unknown-predecessor"),
            vec![unknown_predecessor, known_successor],
            vec![],
            vec![],
            None,
        );
        let predecessor_key = predecessor_input
            .candidate_sources
            .iter()
            .find(|source| source.source_id == OLD)
            .unwrap()
            .record_key
            .clone();
        mark_candidate_unknown(&mut predecessor_input, &predecessor_key);

        let known_predecessor = source(
            OLD,
            "known-predecessor.md",
            "# Known predecessor\nPolicy predecessor.\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let unknown_declaration_source = crate::lineage_store::test_support::source_with_lineage(
            NEW,
            "unknown-successor.md",
            "# Unknown successor\nPolicy successor.\n",
            "2026-08-01T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
            GkxSensitivity::Public,
        );
        let mut declaration_source_input = generation(
            root.path().join("unknown-declaration-source"),
            vec![known_predecessor, unknown_declaration_source],
            vec![],
            vec![],
            None,
        );
        let declaration_source_key = declaration_source_input
            .candidate_sources
            .iter()
            .find(|source| source.source_id == NEW)
            .unwrap()
            .record_key
            .clone();
        mark_candidate_unknown(&mut declaration_source_input, &declaration_source_key);

        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        for (mut input, bytes, unknown_id, supersedes, superseded_by) in [
            (
                predecessor_input,
                BTreeMap::from([
                    (
                        "unknown-predecessor.md".to_owned(),
                        b"# Unknown predecessor\nPolicy predecessor.\n".to_vec(),
                    ),
                    (
                        "known-successor.md".to_owned(),
                        b"# Known successor\nPolicy successor.\n".to_vec(),
                    ),
                ]),
                OLD,
                Vec::<String>::new(),
                vec![NEW.to_owned()],
            ),
            (
                declaration_source_input,
                BTreeMap::from([
                    (
                        "known-predecessor.md".to_owned(),
                        b"# Known predecessor\nPolicy predecessor.\n".to_vec(),
                    ),
                    (
                        "unknown-successor.md".to_owned(),
                        b"# Unknown successor\nPolicy successor.\n".to_vec(),
                    ),
                ]),
                NEW,
                vec![OLD.to_owned()],
                Vec::<String>::new(),
            ),
        ] {
            enable_fixture_vectors(&mut input);
            let state = activate_decision_a_input(input);
            let current_reader = CountingReader {
                bytes,
                calls: AtomicUsize::new(0),
            };
            let current_service = GkxRetrievalCoordinator::open_active(
                &state,
                options(&allow_source, &allow_chunk, None, &current_reader),
            )
            .unwrap();
            let mut current_request = request();
            current_request.as_of = None;
            let current = block_on(current_service.search(&current_request)).unwrap();
            assert_eq!(current.temporal.coverage, TemporalCoverage::NotRequested);
            assert!(current.temporal.reason_codes.is_empty());
            assert_eq!(current.hits.len(), 2);
            assert_eq!(current.eligible_result_count, 2);
            let unknown_hit = current
                .hits
                .iter()
                .find(|hit| hit.chunk.source_id == unknown_id)
                .unwrap();
            assert_eq!(unknown_hit.provenance.valid_from, None);
            assert_eq!(unknown_hit.provenance.valid_to, None);
            assert_eq!(
                unknown_hit.provenance.temporal_state,
                crate::provenance::GkxTemporalState::Unknown
            );
            assert_eq!(unknown_hit.provenance.supersedes, supersedes);
            assert_eq!(unknown_hit.provenance.superseded_by, superseded_by);
            assert!(!unknown_hit.provenance.lineage_neutral);
            assert_eq!(current_reader.calls.load(Ordering::SeqCst), 2);

            let explicit_reader = CountingReader {
                bytes: BTreeMap::new(),
                calls: AtomicUsize::new(0),
            };
            let provider = CountingProvider::new();
            let reranker = counting_reranker();
            let mut coordinator_options = options(
                &allow_source,
                &allow_chunk,
                Some(&provider),
                &explicit_reader,
            );
            coordinator_options.rerank_provider = Some(&reranker);
            let explicit_service =
                GkxRetrievalCoordinator::open_active(&state, coordinator_options).unwrap();
            let explicit = block_on(explicit_service.search(&request())).unwrap();
            assert_eq!(explicit.temporal.coverage, TemporalCoverage::Insufficient);
            assert_eq!(
                explicit.temporal.reason_codes,
                vec!["TEMPORAL_COVERAGE_INSUFFICIENT"]
            );
            assert!(explicit.hits.is_empty());
            assert_eq!(explicit_reader.calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
            assert_eq!(reranker.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn schema3_malformed_runtime_provider_identities_fail_before_index_or_query_work() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let sources = vec![pair.0.clone()];
        let eligible = chunks(&sources)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(state.clone(), vec![pair], eligible, vec![], None);
        let malformed_vector = CountingProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::SqliteLexicalScan,
                provider_id: "custom-provider".to_owned(),
                model_id: "custom-model".to_owned(),
                dimensions: 2,
                timeout_ms: 1_000,
                configuration_digest: sha256(b"custom-config"),
            },
            identity_calls: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            texts: AtomicUsize::new(0),
        };
        assert!(matches!(
            block_on(index_gkx_retrieval_generation(
                input.clone(),
                Some(&malformed_vector),
            )),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("provider-capable family")
        ));
        assert_eq!(malformed_vector.calls.load(Ordering::SeqCst), 0);
        assert_eq!(malformed_vector.texts.load(Ordering::SeqCst), 0);
        assert!(!state.exists());

        let built = build_gkx_retrieval_generation(input).unwrap();
        activate_gkx_retrieval_generation(&state, &built).unwrap();
        let reader = MemoryReader(BTreeMap::from([(
            "old.md".to_owned(),
            b"# Old\nPolicy\n".to_vec(),
        )]));
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        assert!(matches!(
            GkxRetrievalCoordinator::open_active(
                &state,
                options(&allow_source, &allow_chunk, Some(&malformed_vector), &reader),
            ),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("provider-capable family")
        ));

        let malformed_reranker = CountingReranker {
            identity: RerankProviderIdentity {
                kind: RetrievalProviderStageKind::None,
                provider_id: "custom-reranker".to_owned(),
                model_id: "custom-rerank-model".to_owned(),
                timeout_ms: 1_000,
                configuration_digest: sha256(b"custom-rerank-config"),
            },
            calls: AtomicUsize::new(0),
        };
        let mut rerank_options = options(&allow_source, &allow_chunk, None, &reader);
        rerank_options.rerank_provider = Some(&malformed_reranker);
        assert!(matches!(
            GkxRetrievalCoordinator::open_active(&state, rerank_options),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("provider-capable family")
        ));
        assert_eq!(malformed_vector.calls.load(Ordering::SeqCst), 0);
        assert_eq!(malformed_reranker.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn schema3_full_phase3_authority_precedes_every_provider_trait_call() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        fs::create_dir(&state).unwrap();
        crate::sqlite_store::harden_directory_permissions(&state).unwrap();
        let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-ingest-validation-1.0.0-draft.1/",
            "storage-conformance-fixture.json"
        )))
        .unwrap();
        let status = format!(
            "{}\n",
            canonical_json(&fixture["valid_envelopes"]["attempt_status"]).unwrap()
        )
        .into_bytes();
        let status_path = state.join("ingest-attempt-status.json");
        crate::sqlite_store::write_owner_file(&status_path, &status).unwrap();
        crate::sqlite_store::harden_file_permissions(&status_path).unwrap();

        let pair = source(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let sources = vec![pair.0.clone()];
        let eligible = chunks(&sources)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(state.clone(), vec![pair], eligible, vec![], None);
        let provider = CountingProvider::new();
        assert!(block_on(index_gkx_retrieval_generation(input, Some(&provider))).is_err());
        assert_eq!(provider.identity_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
        assert_eq!(fs::read(&status_path).unwrap(), status);
        assert_eq!(fs::read_dir(&state).unwrap().count(), 1);
    }

    #[test]
    fn schema3_held_legacy_writer_precedes_every_provider_trait_call() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let sources = vec![pair.0.clone()];
        let eligible = chunks(&sources)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(state.clone(), vec![pair], eligible, vec![], None);
        let capability = acquire_legacy_retrieval_writer(&state).unwrap();
        let lock_before = fs::read(state.join("retrieval-writer.lock")).unwrap();
        let provider = CountingProvider::new();
        assert!(block_on(index_gkx_retrieval_generation(input, Some(&provider))).is_err());
        assert_eq!(provider.identity_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
        assert_eq!(
            fs::read(state.join("retrieval-writer.lock")).unwrap(),
            lock_before
        );
        drop(capability);
    }

    #[test]
    fn verified_active_cache_reuses_duplicate_content_without_provider_calls() {
        let root = tempdir().unwrap();
        let text = "# Same\nPolicy repeated content.\n";
        let pairs = vec![
            source(
                OLD,
                "old.md",
                text,
                "2026-07-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            source(
                NEW,
                "new.md",
                text,
                "2026-08-01T00:00:00.000Z",
                GkxSensitivity::Secret,
            ),
        ];
        let source_inputs = pairs.iter().map(|pair| pair.0.clone()).collect::<Vec<_>>();
        let eligible = chunks(&source_inputs)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(root.path().join("state"), pairs, eligible, vec![], None);
        let provider = CountingProvider::new();
        let first = block_on(index_gkx_retrieval_generation(
            input.clone(),
            Some(&provider),
        ))
        .unwrap();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            provider.texts.load(Ordering::SeqCst),
            1,
            "same content embeds once"
        );
        activate_gkx_retrieval_generation(&input.state_directory, &first.generation).unwrap();

        let second = block_on(index_gkx_retrieval_generation(input, Some(&provider))).unwrap();
        assert_eq!(second.vector_stage.state, RetrievalStageState::Active);
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "verified cache supplies every digest"
        );
        assert_eq!(provider.texts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_eligible_set_and_runtime_callbacks_are_the_only_policy_gates() {
        let root = tempdir().unwrap();
        let text =
            "# Secret label\nPolicy is locally searchable when the trusted runtime allows it.\n";
        let pair = source(
            OLD,
            "secret.md",
            text,
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Secret,
        );
        let inputs = vec![pair.0.clone()];
        let eligible = chunks(&inputs)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let state_fts = root.path().join("fts-state");
        let fts = build_gkx_retrieval_generation(generation(
            state_fts.clone(),
            vec![pair.clone()],
            eligible.clone(),
            vec![],
            None,
        ))
        .unwrap();
        activate_gkx_retrieval_generation(&state_fts, &fts).unwrap();

        let state_vector = root.path().join("vector-state");
        let provider = CountingProvider::new();
        let vector_input = generation(state_vector.clone(), vec![pair], eligible, vec![], None);
        let vector = block_on(index_gkx_retrieval_generation(
            vector_input,
            Some(&provider),
        ))
        .unwrap();
        activate_gkx_retrieval_generation(&state_vector, &vector.generation).unwrap();

        let reopened = open_active_gkx_retrieval_generation(&state_fts).unwrap();
        let persisted = reopened
            .list_candidate_chunks()
            .unwrap()
            .into_iter()
            .map(|item| item.chunk)
            .collect::<Vec<_>>();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].source_digest, sha256(text.as_bytes()));
        assert!(
            verified_citation(&persisted[0], "", text.as_bytes()).is_ok(),
            "persisted={} source={:?}",
            canonical_json(&persisted[0]).unwrap(),
            text.as_bytes()
        );
        drop(reopened);

        let reader = MemoryReader(BTreeMap::from([(
            "secret.md".to_owned(),
            text.as_bytes().to_vec(),
        )]));
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let deny_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Deny);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let fts_allow = GkxRetrievalCoordinator::open_active(
            &state_fts,
            options(&allow_source, &allow_chunk, None, &reader),
        )
        .unwrap();
        let vector_allow = GkxRetrievalCoordinator::open_active(
            &state_vector,
            options(&allow_source, &allow_chunk, Some(&provider), &reader),
        )
        .unwrap();
        let fts_result = block_on(fts_allow.search(&request())).unwrap();
        let vector_result = block_on(vector_allow.search(&request())).unwrap();
        assert_eq!(
            fts_result.hits.len(),
            1,
            "{}",
            canonical_json(&fts_result).unwrap()
        );
        assert_eq!(
            vector_result.hits.len(),
            1,
            "{}",
            canonical_json(&vector_result).unwrap()
        );

        let fts_deny = GkxRetrievalCoordinator::open_active(
            &state_fts,
            options(&deny_source, &allow_chunk, None, &reader),
        )
        .unwrap();
        let vector_deny = GkxRetrievalCoordinator::open_active(
            &state_vector,
            options(&deny_source, &allow_chunk, Some(&provider), &reader),
        )
        .unwrap();
        assert!(block_on(fts_deny.search(&request()))
            .unwrap()
            .hits
            .is_empty());
        assert!(block_on(vector_deny.search(&request()))
            .unwrap()
            .hits
            .is_empty());
    }

    #[test]
    fn complete_vector_eligibility_mismatch_precedes_stale_live_reads_and_query_provider() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let text = "# Visible\nPolicy text that must not be read.\n";
        let pair = source(
            OLD,
            "visible.md",
            text,
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        // The vector space is active, but the policy-bound persisted eligible
        // set deliberately omits the temporally eligible candidate chunk.
        let input = generation(
            state.clone(),
            vec![pair],
            vec![],
            vec![],
            Some(("fixture-provider", "fixture-2d", 2)),
        );
        let built = build_gkx_retrieval_generation(input).unwrap();
        activate_gkx_retrieval_generation(&state, &built).unwrap();

        let reader = CountingReader {
            // Wrong bytes would be stale if the live boundary were reached.
            bytes: BTreeMap::from([("visible.md".to_owned(), b"stale".to_vec())]),
            calls: AtomicUsize::new(0),
        };
        let provider = CountingProvider::new();
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let service = GkxRetrievalCoordinator::open_active(
            &state,
            options(&allow_source, &allow_chunk, Some(&provider), &reader),
        )
        .unwrap();
        assert!(matches!(
            block_on(service.search(&request())),
            Err(RetrievalError::ProjectionMismatch(message))
                if message == "RETRIEVAL_RUNTIME_VECTOR_ELIGIBILITY_MISMATCH"
        ));
        assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn schema3_aliased_cache_fails_before_provider_or_state_write() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let inputs = vec![pair.0.clone()];
        let eligible = chunks(&inputs)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(state.clone(), vec![pair], eligible, vec![], None);
        let built = build_gkx_retrieval_generation(input.clone()).unwrap();
        activate_gkx_retrieval_generation(&state, &built).unwrap();
        fs::hard_link(
            state.join("active-retrieval.json"),
            state.join("active-pointer-alias.json"),
        )
        .unwrap();
        let before = fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        let provider = CountingProvider::new();
        assert!(matches!(
            block_on(index_gkx_retrieval_generation(input, Some(&provider))),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("HARDLINK") || message.contains("ALIAS")
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
        let after = fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn schema3_corrupt_pointer_is_authority_failure_before_provider_or_state_work() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let inputs = vec![pair.0.clone()];
        let eligible = chunks(&inputs)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(state.clone(), vec![pair], eligible, vec![], None);
        let first = build_gkx_retrieval_generation(input.clone()).unwrap();
        activate_gkx_retrieval_generation(&state, &first).unwrap();
        fs::write(state.join("active-retrieval.json"), b"{broken").unwrap();
        let before = fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        let provider = CountingProvider::new();
        assert!(block_on(index_gkx_retrieval_generation(input, Some(&provider))).is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.texts.load(Ordering::SeqCst), 0);
        let after = fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn dropping_schema3_index_future_releases_exact_owned_writer_lock() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let inputs = vec![pair.0.clone()];
        let eligible = chunks(&inputs)
            .into_iter()
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let input = generation(state.clone(), vec![pair], eligible, vec![], None);
        let provider = PendingProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "fixture-provider".to_owned(),
                model_id: "fixture-2d".to_owned(),
                dimensions: 2,
                timeout_ms: 300_000,
                configuration_digest: crate::digest::sha256(b"pending-provider-config"),
            },
            calls: AtomicUsize::new(0),
        };
        let mut future = Box::pin(index_gkx_retrieval_generation(
            input.clone(),
            Some(&provider),
        ));
        let mut context = Context::from_waker(noop_waker_ref());
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(state.join("retrieval-writer.lock").exists());
        drop(future);
        assert!(!state.join("retrieval-writer.lock").exists());
        block_on(index_gkx_retrieval_generation(input, None)).unwrap();
    }

    #[test]
    fn frozen_full_draft2_fixture_executes_decision_a_without_gkx_authority() {
        let pin_bytes = include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
            "FULL-PIN.json"
        ));
        let pin: serde_json::Value = serde_json::from_slice(pin_bytes).unwrap();
        assert_eq!(
            pin["reference_commit"],
            "6e2df27d33ede62ee0d2e3cb7610df478a7d66ce"
        );
        assert_eq!(pin["reference_package_version"], "2.1.2");
        assert_eq!(pin["reference_state"], "full_phase2_published_hosted_green");
        assert_eq!(pin["pin_kind"], "exact_full_commit_and_frozen_file_sha256");
        assert_eq!(pin["publication_qualified"], true);
        assert!(pin_bytes.ends_with(b"\n"));
        assert!(!pin_bytes.ends_with(b"\n\n"));
        let pack = [
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "chunk.schema.json"
                ))
                .as_slice(),
                "sha256:d1ffd008bf360807d50494bc34610670732ee9fb61ed15fcd8f0aee7496a6fab",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "conformance-fixture.json"
                ))
                .as_slice(),
                "sha256:eb4b77590ae3d113a129f5f9baa7adb77737789d3fc1dfbcd8e9aa6ec353ae61",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "contract.json"
                ))
                .as_slice(),
                "sha256:203ba5d54e1eeecd88a4d706f0394f9b517667194bb78e4f197911ac0358d4d5",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "projection.schema.json"
                ))
                .as_slice(),
                "sha256:97ae4481f3780536de4ba743fc0f7067f342f6ca8ce69b8a976ed1894a5ee753",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "provenance.schema.json"
                ))
                .as_slice(),
                "sha256:bcad32df33e5fe3e28aa85f4674b7c9eec92bf6e05c14a7d8a1858808231fb84",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "README.md"
                ))
                .as_slice(),
                "sha256:cf983c2e6269a856aece443f9cf16c0fafbe024ebc00714154838c7e0a7618ec",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "result.schema.json"
                ))
                .as_slice(),
                "sha256:a12f1ba4a25ab746425fc279425e92579812beb5879d1b8d778571539198eb97",
            ),
            (
                include_bytes!(concat!(
                    "../../../contracts/gkos-retrieval-1.0.0-draft.2/",
                    "stored-provenance.schema.json"
                ))
                .as_slice(),
                "sha256:de3261a093e65cba11cc05490947c914a9a3f880183b0a1cf2f52ef1dfd5a861",
            ),
        ];
        for (bytes, expected_digest) in pack {
            assert!(bytes.ends_with(b"\n"));
            assert!(!bytes.ends_with(b"\n\n"));
            assert_eq!(crate::digest::sha256(bytes), expected_digest);
        }

        let fixture = final_fixture();
        let contract = final_contract();
        for case in fixture.as_of_grammar {
            let actual = normalize_retrieval_as_of(&case.value);
            if case.accepted {
                assert_eq!(actual.unwrap(), case.normalized.unwrap(), "{}", case.value);
            } else {
                assert!(actual.is_err(), "{}", case.value);
                assert!(case.normalized.is_none());
            }
        }
        assert_eq!(fixture.branched_lineage.status, "ratified_decision_a");
        assert!(fixture.branched_lineage.frozen);
        assert_eq!(
            fixture.authorization_dependent_diagnostics.status,
            "ratified_decision_a"
        );
        assert!(fixture.authorization_dependent_diagnostics.frozen);
        let pending = fixture
            .authorization_dependent_diagnostics
            .matrix
            .iter()
            .filter(|row| row.scope == "authorized_view_ratified_a")
            .collect::<Vec<_>>();
        assert_eq!(pending.len(), 4);
        assert!(pending
            .iter()
            .all(|row| !row.required_owner_decision && row.frozen));
        assert_eq!(
            contract.cross_record_topology_authority.status,
            "ratified_decision_a"
        );
        assert!(contract.cross_record_topology_authority.frozen);
        assert_eq!(
            contract.cross_record_topology_authority.matrix_ids,
            pending.iter().map(|row| row.id.clone()).collect::<Vec<_>>()
        );
        let intrinsic = fixture
            .authorization_dependent_diagnostics
            .matrix
            .iter()
            .find(|row| row.id == "source_intrinsic_validation")
            .unwrap();
        assert_eq!(intrinsic.scope, "source_intrinsic_existing_fail_closed");
        assert!(!intrinsic.required_owner_decision);
        assert!(intrinsic.frozen);
        assert!(fixture
            .authorization_dependent_diagnostics
            .differential_cases
            .iter()
            .all(|row| row.normative_retrieval_outcome.is_some() && row.frozen));

        for provenance in &fixture.executable_projection.expected_stored_provenance {
            provenance.validate().unwrap();
        }
        let assertion_template = &fixture.executable_projection.expected_stored_provenance[1];
        for case in &fixture.stored_provenance_assertion_validity.accepted {
            with_assertion_validity_case(assertion_template, case)
                .validate()
                .unwrap();
        }
        for case in &fixture.stored_provenance_assertion_validity.rejected {
            assert!(with_assertion_validity_case(assertion_template, case)
                .validate()
                .is_err());
        }
        let interval_sources = fixture
            .intervals
            .iter()
            .enumerate()
            .map(|(index, interval)| interval_provenance(assertion_template, interval, index))
            .collect::<Vec<_>>();
        for source in &interval_sources {
            source.validate().unwrap();
        }
        for point in &fixture.point_in_time {
            let normalized = normalize_retrieval_as_of(&point.as_of).unwrap();
            let projected =
                crate::provenance::project_stored_intervals_at(&interval_sources, &normalized)
                    .unwrap();
            assert_eq!(projected.valid, point.expected_valid, "{}", point.as_of);
            assert_eq!(
                projected.historical, point.expected_historical,
                "{}",
                point.as_of
            );
            assert_eq!(projected.future, point.expected_future, "{}", point.as_of);
            assert_eq!(projected.unknown, point.expected_unknown, "{}", point.as_of);
            for source in &interval_sources {
                assert_eq!(
                    crate::provenance::source_valid_at(source, &normalized).unwrap(),
                    point.expected_valid.contains(&source.source_id),
                    "{} {}",
                    point.as_of,
                    source.source_id
                );
            }
        }
        let root = tempdir().unwrap();
        let by_path = fixture
            .executable_projection
            .input_files
            .iter()
            .map(|file| (file.relative_path.as_str(), file.content.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut pairs = Vec::new();
        for provenance in &fixture.executable_projection.expected_stored_provenance {
            let text = by_path[provenance.source_path.as_str()];
            let input = crate::contract::GkxRetrievalSource {
                contract_version: crate::contract::RETRIEVAL_CONTRACT.to_owned(),
                vault_id: fixture
                    .executable_projection
                    .generation_input
                    .vault_id
                    .clone(),
                source_id: provenance.source_id.clone(),
                source_path: provenance.source_path.clone(),
                source_digest: provenance.source_digest.clone(),
                text: text.to_owned(),
                lineage: crate::contract::CanonicalLineageEnvelope {
                    lineage_id: None,
                    lineage_neutral: provenance.lineage_neutral,
                    supersedes: provenance.resolved_supersedes.clone(),
                    superseded_by: provenance.resolved_superseded_by.clone(),
                    reason_codes: vec![],
                },
                temporal: crate::contract::CanonicalTemporalEnvelope {
                    assertion_time: provenance.assertion_time.clone(),
                    valid_from: provenance.valid_from.clone(),
                    valid_to: provenance.valid_to.clone(),
                    valid_from_unix_ms: provenance
                        .valid_from
                        .as_deref()
                        .map(crate::provenance::normalized_timestamp_millis)
                        .transpose()
                        .unwrap(),
                    valid_to_unix_ms: provenance
                        .valid_to
                        .as_deref()
                        .map(crate::provenance::normalized_timestamp_millis)
                        .transpose()
                        .unwrap(),
                },
                metadata: provenance.source_metadata.clone(),
            };
            pairs.push((input, provenance.clone()));
        }
        let computed_chunks = chunks(&pairs.iter().map(|pair| pair.0.clone()).collect::<Vec<_>>());
        let record_by_source = fixture
            .executable_projection
            .expected_candidate_sources
            .iter()
            .map(|source| (source.source_id.as_str(), source.record_key.as_str()))
            .collect::<BTreeMap<_, _>>();
        let key_by_public = computed_chunks
            .iter()
            .map(|chunk| {
                let record_key = record_by_source[chunk.source_id.as_str()];
                (
                    chunk.chunk_id.clone(),
                    crate::candidate::candidate_chunk_key(record_key, &chunk.chunk_id).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let candidate_chunks = computed_chunks
            .iter()
            .map(|chunk| {
                let mut nested = chunk.clone();
                nested.valid_to = None;
                nested.lineage_id = None;
                nested.supersedes.clear();
                nested.superseded_by.clear();
                crate::candidate::GkxCandidateChunk {
                    candidate_chunk_key: key_by_public[&chunk.chunk_id].clone(),
                    record_key: record_by_source[chunk.source_id.as_str()].to_owned(),
                    parent_candidate_chunk_key: chunk
                        .parent_chunk_id
                        .as_ref()
                        .and_then(|parent| key_by_public.get(parent).cloned()),
                    chunk: nested,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            candidate_chunks
                .iter()
                .map(|chunk| chunk.candidate_chunk_key.clone())
                .collect::<BTreeSet<_>>(),
            fixture
                .executable_projection
                .generation_input
                .embedding_eligible_candidate_chunk_keys
                .iter()
                .cloned()
                .collect()
        );
        let state = root.path().join("state");
        let generation_input = GkxRetrievalGenerationInput {
            state_directory: state.clone(),
            engine_version: "2.1.2".to_owned(),
            vault_id: fixture
                .executable_projection
                .generation_input
                .vault_id
                .clone(),
            source_snapshot_digest: fixture
                .executable_projection
                .generation_input
                .source_snapshot_digest
                .clone(),
            configuration_digest: fixture
                .executable_projection
                .generation_input
                .configuration_digest
                .clone(),
            policy_digest: fixture
                .executable_projection
                .generation_input
                .policy_digest
                .clone(),
            candidate_sources: fixture
                .executable_projection
                .expected_candidate_sources
                .clone(),
            candidate_declarations: fixture
                .executable_projection
                .expected_candidate_declarations
                .clone(),
            candidate_chunks,
            embedding_eligible_candidate_chunk_keys: fixture
                .executable_projection
                .generation_input
                .embedding_eligible_candidate_chunk_keys
                .clone(),
            vectors: vec![],
            embedding_provider_id: None,
            embedding_model_id: None,
            embedding_dimensions: None,
        };
        let built = build_gkx_retrieval_generation(generation_input).unwrap();
        let manifest = built.manifest();
        assert_eq!(
            canonical_json(&serde_json::to_value(manifest).unwrap()).unwrap(),
            canonical_json(&fixture.executable_projection.expected_fts5_manifest).unwrap()
        );
        assert_eq!(
            manifest.lexical_backend,
            crate::contract::SqliteLexicalBackend::SqliteFts5
        );
        assert_eq!(
            fixture.executable_projection.expected_manifest["lexical_backend"],
            "sqlite_lexical_scan"
        );
        activate_gkx_retrieval_generation(&state, &built).unwrap();

        let reader = MemoryReader(
            fixture
                .executable_projection
                .input_files
                .iter()
                .map(|file| (file.relative_path.clone(), file.content.as_bytes().to_vec()))
                .collect(),
        );
        let allow_source = |_source: &GkxSourcePolicyRecord| Ok(DiscoverabilityDecision::Allow);
        let allow_chunk = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let service = GkxRetrievalCoordinator::open_active(
            &state,
            GkxRetrievalCoordinatorOptions {
                runtime_policy_digest: fixture
                    .executable_projection
                    .generation_input
                    .policy_digest
                    .clone(),
                source_discoverability_policy: &allow_source,
                discoverability_policy: &allow_chunk,
                vector_provider: None,
                rerank_provider: None,
                source_reader: &reader,
                lineage_view_freshness: GkxProjectionFreshness::Fresh,
                max_parent_bytes: 4_096,
                max_result_bytes: 131_072,
            },
        )
        .unwrap();
        let result =
            block_on(service.search(&fixture.executable_projection.search_request)).unwrap();
        let actual = serde_json::to_value(&result).unwrap();
        assert_eq!(
            canonical_json(&actual).unwrap(),
            canonical_json(&fixture.executable_projection.expected_fts5_result).unwrap()
        );
        assert_eq!(
            fixture.executable_projection.expected_result["stages"]["lexical"]["kind"],
            "sqlite_lexical_scan"
        );

        let unicode_source = crate::contract::RetrievalSource {
            contract_version: crate::contract::RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "fixture-vault".to_owned(),
            source_id: "018f0000-0000-7000-8000-000000000299".to_owned(),
            source_path: "unicode.md".to_owned(),
            source_digest: sha256(fixture.unicode_citation.source_text.as_bytes()),
            text: fixture.unicode_citation.source_text.clone(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: crate::contract::CanonicalLineageEnvelope::default(),
            temporal: crate::contract::CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        };
        let unicode_chunks = crate::chunker::chunk_source(
            &unicode_source,
            crate::chunker::ChunkingOptions::default(),
        )
        .unwrap();
        assert_eq!(unicode_chunks.len(), 1);
        let citation = verified_citation(
            &unicode_chunks[0],
            &fixture.unicode_citation.query,
            fixture.unicode_citation.source_text.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            citation.matched_spans,
            vec![fixture.unicode_citation.expected_span]
        );
    }
}
