use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::chunker::{chunk_source, line_number};
use crate::confidence::assess_retrieval_confidence;
use crate::contract::{
    DiscoverabilityDecision, MatchedSpan, ProjectionFreshness, RankedCandidate, RetrievalChunk,
    RetrievalHit, RetrievalParentContext, RetrievalProviderStageKind, RetrievalProviderStageStatus,
    RetrievalSearchRequest, RetrievalSearchResult, RetrievalSearchStages, RetrievalStageScores,
    RetrievalStageState, SourceCitation, SqliteLexicalBackend, MAX_RESULT_BYTES,
    MMR_DEFAULT_LAMBDA, PARENT_EXPANSION_MAX_CHILD_TOKENS, RETRIEVAL_CONTRACT, RRF_DEFAULT_K,
};
use crate::digest::sha256;
use crate::error::cache_value_or_miss;
use crate::filters::{applied_filter_names, matches_retrieval_filters, validate_retrieval_filters};
use crate::fusion::{
    code_unit_compare, maximal_marginal_relevance_with_relevance, reciprocal_rank_fusion,
};
use crate::path_security::{absolute_lexical_path, contained_path, equivalent_paths};
use crate::providers::{
    bounded_provider_call, validate_rerank_provider_identity, validate_vector_provider_identity,
    RerankProvider, VectorProvider, VectorProviderIdentity,
};
use crate::redaction::{authorize_search_result, AuthorizedRetrievalSearchResult};
#[cfg(test)]
use crate::sqlite_store::build_retrieval_generation;
use crate::sqlite_store::{
    build_retrieval_generation_with_writer, lexical_query_clauses,
    open_active_retrieval_generation, trim_ecmascript_whitespace,
    try_open_active_retrieval_generation, validate_full_engine_version, BuiltRetrievalGeneration,
    RetrievalGenerationInput, SqliteRetrievalStore, StoredVector,
};
use crate::writer_lock::{
    acquire_legacy_retrieval_writer, assert_legacy_writer_capability, assert_no_phase3_authority,
    finish_with_writer, LegacyRetrievalWriterCapability,
};
use crate::{RetrievalError, RetrievalResult};

pub trait SourceReader: Send + Sync {
    fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>>;
}

pub struct FileSourceReader {
    root: PathBuf,
}

impl FileSourceReader {
    pub fn new(root: &Path) -> RetrievalResult<Self> {
        let supplied = absolute_lexical_path(root)?;
        let metadata = fs::symlink_metadata(&supplied)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(RetrievalError::InvalidConfig(
                "vault root must be a real directory".to_owned(),
            ));
        }
        let canonical = fs::canonicalize(&supplied)?;
        if !equivalent_paths(&canonical, &supplied) {
            return Err(RetrievalError::InvalidConfig(
                "vault root aliases are not eligible for retrieval".to_owned(),
            ));
        }
        Ok(Self { root: canonical })
    }
}

impl SourceReader for FileSourceReader {
    fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>> {
        if !crate::contract::is_normalized_relative_path(source_path) {
            return Err(RetrievalError::InvalidEnvelope(
                "source path is outside the portable retrieval grammar".to_owned(),
            ));
        }
        let path = self.root.join(source_path);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || link_count(&path, &metadata)? > 1
        {
            return Err(RetrievalError::InvalidEnvelope(
                "source aliases are not eligible for retrieval".to_owned(),
            ));
        }
        let actual = fs::canonicalize(&path)?;
        if !equivalent_paths(&actual, &path) || !contained_path(&actual, &self.root) {
            return Err(RetrievalError::InvalidEnvelope(
                "source path escaped or traversed an alias".to_owned(),
            ));
        }
        Ok(fs::read(actual)?)
    }
}

#[cfg(unix)]
fn link_count(_path: &Path, metadata: &fs::Metadata) -> RetrievalResult<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink())
}

#[cfg(windows)]
fn link_count(path: &Path, _metadata: &fs::Metadata) -> RetrievalResult<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the std File owns a valid handle for the duration of the call,
    // and `information` points to writable storage of the required type.
    let success =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
    if success == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(u64::from(information.nNumberOfLinks))
}

#[cfg(not(any(unix, windows)))]
fn link_count(_path: &Path, _metadata: &fs::Metadata) -> RetrievalResult<u64> {
    Ok(1)
}

pub type DiscoverabilityPolicy =
    dyn Fn(&RetrievalChunk) -> RetrievalResult<DiscoverabilityDecision> + Send + Sync;

pub struct RetrievalCoordinatorOptions<'a> {
    pub discoverability_policy: &'a DiscoverabilityPolicy,
    pub vector_provider: Option<&'a dyn VectorProvider>,
    pub rerank_provider: Option<&'a dyn RerankProvider>,
    pub source_reader: &'a dyn SourceReader,
    pub stale: bool,
    pub max_parent_bytes: usize,
    pub max_result_bytes: usize,
}

impl<'a> RetrievalCoordinatorOptions<'a> {
    pub fn fts_only(
        discoverability_policy: &'a DiscoverabilityPolicy,
        source_reader: &'a dyn SourceReader,
    ) -> Self {
        Self {
            discoverability_policy,
            vector_provider: None,
            rerank_provider: None,
            source_reader,
            stale: false,
            max_parent_bytes: 8_192,
            max_result_bytes: MAX_RESULT_BYTES,
        }
    }

    fn validate(&self) -> RetrievalResult<()> {
        if !(256..=65_536).contains(&self.max_parent_bytes)
            || !(16_384..=1_048_576).contains(&self.max_result_bytes)
        {
            return Err(RetrievalError::InvalidConfig(
                "coordinator byte budgets are outside their contract bounds".to_owned(),
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

pub struct RetrievalCoordinator<'a> {
    store: SqliteRetrievalStore,
    options: RetrievalCoordinatorOptions<'a>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedCitationInterval {
    pub source_id: String,
    pub start_byte: u64,
    pub end_byte: u64,
}

pub type MatchedSpanKey = (String, u64, u64);

pub struct DeduplicatedCitationEvidence {
    pub citation: SourceCitation,
    pub span_keys: Vec<MatchedSpanKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRetrievalResult {
    pub generation: BuiltRetrievalGeneration,
    pub vector_stage: RetrievalProviderStageStatus,
}

pub async fn index_retrieval_generation(
    input: RetrievalGenerationInput,
    vector_provider: Option<&dyn VectorProvider>,
) -> RetrievalResult<IndexRetrievalResult> {
    validate_full_engine_version(&input.engine_version)?;
    if !input.vectors.is_empty()
        || input.embedding_provider_id.is_some()
        || input.embedding_model_id.is_some()
        || input.embedding_dimensions.is_some()
    {
        return Err(RetrievalError::InvalidConfig(
            "index coordinator accepts only an unembedded base generation".to_owned(),
        ));
    }
    let mut writer = acquire_legacy_retrieval_writer(&input.state_directory)?;
    let result = async {
        let identity = vector_provider.map(|provider| provider.identity().clone());
        if let Some(identity) = &identity {
            validate_vector_provider_identity(identity)?;
        }
        index_retrieval_generation_with_writer(input, vector_provider, identity, &writer).await
    }
    .await;
    finish_with_writer(result, &mut writer)
}

async fn index_retrieval_generation_with_writer(
    mut input: RetrievalGenerationInput,
    vector_provider: Option<&dyn VectorProvider>,
    identity: Option<VectorProviderIdentity>,
    writer: &LegacyRetrievalWriterCapability,
) -> RetrievalResult<IndexRetrievalResult> {
    assert_legacy_writer_capability(writer, &input.state_directory)?;
    assert_no_phase3_authority(writer.state_directory())?;
    let Some(provider) = vector_provider else {
        return Ok(IndexRetrievalResult {
            generation: build_retrieval_generation_with_writer(input, writer)?,
            vector_stage: stage(
                RetrievalProviderStageKind::None,
                RetrievalStageState::Disabled,
                &["VECTOR_DISABLED"],
                None,
            ),
        });
    };
    let identity = identity.expect("provider identity is preflighted with its provider");
    let mut chunks = Vec::new();
    for source in &input.sources {
        chunks.extend(chunk_source(source, input.chunking)?);
    }
    chunks.sort_by(|left, right| code_unit_compare(&left.chunk_id, &right.chunk_id));
    let cache =
        match cache_value_or_miss(try_open_active_retrieval_generation(&input.state_directory))? {
            Some(Some(store)) if store.manifest.vault_id == input.vault_id => {
                cache_value_or_miss(store.cached_vectors_by_content(
                    &identity.provider_id,
                    &identity.model_id,
                    identity.dimensions,
                ))?
                .unwrap_or_default()
            }
            _ => BTreeMap::new(),
        };
    let embedding_result = embed_chunks(provider, &chunks, &cache).await;
    match embedding_result {
        Ok(vectors) => {
            input.vectors = vectors;
            input.embedding_provider_id = Some(identity.provider_id.clone());
            input.embedding_model_id = Some(identity.model_id.clone());
            input.embedding_dimensions = Some(identity.dimensions);
            Ok(IndexRetrievalResult {
                generation: build_retrieval_generation_with_writer(input, writer)?,
                vector_stage: stage(
                    identity.kind,
                    RetrievalStageState::Active,
                    &[],
                    Some(&identity),
                ),
            })
        }
        Err(_) => Ok(IndexRetrievalResult {
            generation: build_retrieval_generation_with_writer(input, writer)?,
            vector_stage: stage(
                identity.kind,
                RetrievalStageState::Degraded,
                &["VECTOR_UNAVAILABLE"],
                Some(&identity),
            ),
        }),
    }
}

pub(crate) async fn embed_chunks(
    provider: &dyn VectorProvider,
    chunks: &[RetrievalChunk],
    cached_by_digest: &BTreeMap<String, Vec<f64>>,
) -> RetrievalResult<Vec<StoredVector>> {
    let identity = provider.identity();
    let mut unique = BTreeMap::<String, &RetrievalChunk>::new();
    for chunk in chunks {
        unique.entry(chunk.content_digest.clone()).or_insert(chunk);
    }
    let unique = unique
        .into_values()
        .filter(|chunk| !cached_by_digest.contains_key(&chunk.content_digest))
        .collect::<Vec<_>>();
    let mut vectors_by_digest = cached_by_digest.clone();
    let mut offset = 0;
    while offset < unique.len() {
        let mut batch = Vec::new();
        let mut batch_bytes = 0_usize;
        while offset + batch.len() < unique.len() && batch.len() < 32 {
            let candidate = unique[offset + batch.len()];
            if !batch.is_empty() && batch_bytes + candidate.text.len() > 262_144 {
                break;
            }
            batch_bytes += candidate.text.len();
            batch.push(candidate);
        }
        let texts = batch
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let request_id = sha256(
            format!(
                "index\0{offset}\0{}",
                batch
                    .iter()
                    .map(|chunk| chunk.content_digest.as_str())
                    .collect::<Vec<_>>()
                    .join("\0")
            )
            .as_bytes(),
        );
        let embedded = bounded_provider_call(
            provider.embed(&request_id, &texts),
            identity.timeout_ms,
            "EMBEDDING_TIMEOUT",
        )
        .await?;
        if embedded.len() != batch.len() {
            return Err(RetrievalError::ProviderResponse(
                "embedding response item count mismatch".to_owned(),
            ));
        }
        for (chunk, vector) in batch.iter().zip(embedded) {
            if vector.len() != identity.dimensions as usize
                || vector.iter().any(|value| !value.is_finite())
            {
                return Err(RetrievalError::ProviderResponse(
                    "embedding response dimensions or values are invalid".to_owned(),
                ));
            }
            vectors_by_digest.insert(
                chunk.content_digest.clone(),
                vector.into_iter().map(f64::from).collect(),
            );
        }
        offset += batch.len();
    }
    chunks
        .iter()
        .map(|chunk| {
            Ok(StoredVector {
                chunk_id: chunk.chunk_id.clone(),
                vector: vectors_by_digest
                    .get(&chunk.content_digest)
                    .cloned()
                    .ok_or_else(|| {
                        RetrievalError::ProviderResponse(
                            "deduplicated embedding response is incomplete".to_owned(),
                        )
                    })?,
            })
        })
        .collect()
}

impl<'a> RetrievalCoordinator<'a> {
    pub fn open_active(
        state_directory: &Path,
        options: RetrievalCoordinatorOptions<'a>,
    ) -> RetrievalResult<Self> {
        options.validate()?;
        Ok(Self {
            store: open_active_retrieval_generation(state_directory)?,
            options,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_verified_store(
        store: SqliteRetrievalStore,
        options: RetrievalCoordinatorOptions<'a>,
    ) -> RetrievalResult<Self> {
        options.validate()?;
        Ok(Self { store, options })
    }

    pub async fn search(
        &self,
        request: &RetrievalSearchRequest,
    ) -> RetrievalResult<AuthorizedRetrievalSearchResult> {
        validate_search_request(&request.query, request)?;
        let query = trim_ecmascript_whitespace(&request.query);
        let limit = request.limit.unwrap_or(5) as usize;
        let lexical_top_k = request
            .lexical_top_k
            .map_or(20_usize.max(limit * 4), |value| value as usize);
        let semantic_top_k = request
            .semantic_top_k
            .map_or(20_usize.max(limit * 4), |value| value as usize);
        let filters = request.filters.clone().unwrap_or_default();
        let chunks = self.store.list_chunks()?;
        let mut by_source = BTreeMap::<String, Vec<RetrievalChunk>>::new();
        for chunk in chunks {
            by_source
                .entry(chunk.source_id.clone())
                .or_default()
                .push(chunk);
        }
        let mut source_groups = by_source.into_values().collect::<Vec<_>>();
        source_groups
            .sort_by(|left, right| code_unit_compare(&left[0].source_id, &right[0].source_id));
        let mut policy_eligible = Vec::new();
        for group in source_groups {
            let mut allowed = true;
            for chunk in &group {
                if !matches_retrieval_filters(chunk, &filters, &self.store.manifest.vault_id)?
                    || !policy_allows(self.options.discoverability_policy, chunk)
                {
                    allowed = false;
                    break;
                }
            }
            if allowed {
                policy_eligible.push(group);
            }
        }
        let mut source_bytes = BTreeMap::<String, Vec<u8>>::new();
        let mut eligible = Vec::new();
        let mut stale_citation = false;
        for group in policy_eligible {
            let first = &group[0];
            let bytes = match self.options.source_reader.read(&first.source_path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    stale_citation = true;
                    continue;
                }
            };
            if sha256(&bytes) != first.source_digest
                || group.iter().any(|chunk| !chunk_round_trips(chunk, &bytes))
            {
                stale_citation = true;
                continue;
            }
            source_bytes.insert(first.source_path.clone(), bytes);
            eligible.extend(group);
        }
        let eligible_ids = eligible
            .iter()
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<BTreeSet<_>>();
        let eligible_id_list = eligible_ids.iter().cloned().collect::<Vec<_>>();
        let discoverable_sources = eligible
            .iter()
            .map(|chunk| chunk.source_id.clone())
            .collect::<BTreeSet<_>>();
        let lexical =
            self.store
                .lexical_search_eligible(query, &eligible_id_list, lexical_top_k)?;
        let lexical_stage = lexical_stage(self.store.manifest.lexical_backend, true);
        let (semantic, vector_stage) = self
            .semantic_search(query, &eligible_ids, semantic_top_k)
            .await?;
        let rrf_k = request.rrf_k.unwrap_or(RRF_DEFAULT_K);
        let fused = reciprocal_rank_fusion(&lexical, &semantic, rrf_k)?;
        let fused_ranks = fused
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.chunk_id.clone(), index as u32 + 1))
            .collect::<BTreeMap<_, _>>();
        let (ordered, reranker_stage, reranker_scores, reranker_ranks) =
            self.rerank(query, &eligible, &fused).await;
        let rerank_relevance = (reranker_stage.state == RetrievalStageState::Active).then(|| {
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
        let mut result_bytes = 0_usize;
        let mut claimed_matched_spans = BTreeSet::<MatchedSpanKey>::new();
        let mut accepted_intervals = Vec::<AcceptedCitationInterval>::new();
        let parent_expansion_max_child_tokens = request
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
                RetrievalError::ProjectionMismatch("verified source bytes are missing".to_owned())
            })?;
            let Some(evidence) = deduplicate_overlap_evidence(
                verified_citation(chunk, query, live)?,
                &claimed_matched_spans,
                &accepted_intervals,
            ) else {
                continue;
            };
            let citation = evidence.citation;
            let mut parent_context = if request.parent_expansion == Some(true)
                && chunk.token_count < parent_expansion_max_child_tokens
            {
                self.parent_context(chunk, &by_id, &source_bytes)?
            } else {
                None
            };
            if result_bytes.saturating_add(chunk.text.len()) > self.options.max_result_bytes {
                continue;
            }
            if parent_context.as_ref().is_some_and(|parent| {
                result_bytes
                    .saturating_add(chunk.text.len())
                    .saturating_add(parent.text.len())
                    > self.options.max_result_bytes
            }) {
                parent_context = None;
            }
            let hit_bytes = chunk.text.len()
                + parent_context
                    .as_ref()
                    .map_or(0, |parent| parent.text.len());
            let scores = RetrievalStageScores {
                lexical_score: candidate.lexical_score,
                semantic_score: candidate.semantic_score,
                fusion_score: candidate.fusion_score,
                reranker_score: reranker_scores.get(&candidate.chunk_id).copied(),
                mmr_score: candidate.mmr_score,
                lexical_rank: candidate.lexical_rank,
                semantic_rank: candidate.semantic_rank,
                fused_rank: *fused_ranks.get(&candidate.chunk_id).ok_or_else(|| {
                    RetrievalError::ProjectionMismatch("fused rank is missing".to_owned())
                })?,
                reranker_rank: reranker_ranks.get(&candidate.chunk_id).copied(),
                final_rank: hits.len() as u32 + 1,
            };
            hits.push(RetrievalHit {
                chunk: chunk.clone(),
                citation,
                stage_scores: scores,
                parent_context,
            });
            claimed_matched_spans.extend(evidence.span_keys);
            accepted_intervals.push(AcceptedCitationInterval {
                source_id: chunk.source_id.clone(),
                start_byte: chunk.start_byte,
                end_byte: chunk.end_byte,
            });
            result_bytes += hit_bytes;
        }
        let stale = self.options.stale || stale_citation;
        let confidence = assess_retrieval_confidence(
            &hits
                .iter()
                .map(|hit| hit.stage_scores.clone())
                .collect::<Vec<_>>(),
            &vector_stage,
            &reranker_stage,
            eligible.len(),
            stale,
        );
        let internal = RetrievalSearchResult {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            query_digest: sha256(query.as_bytes()),
            projection_id: self.store.manifest.projection_id.clone(),
            projection_digest: self.store.manifest.projection_digest.clone(),
            projection_freshness: if stale {
                ProjectionFreshness::Stale
            } else {
                ProjectionFreshness::Fresh
            },
            hits,
            confidence,
            applied_filters: applied_filter_names(&filters),
            eligible_result_count: u32::try_from(eligible.len()).map_err(|_| {
                RetrievalError::InvalidConfig("too many eligible chunks".to_owned())
            })?,
            stages: RetrievalSearchStages {
                lexical: lexical_stage,
                vector: vector_stage,
                reranker: reranker_stage,
            },
        };
        authorize_search_result(&internal, &discoverable_sources, &by_id)
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
                Vec::new(),
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
                Vec::new(),
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
        let texts = [query.to_owned()];
        let request_id = sha256(query.as_bytes());
        let attempt = bounded_provider_call(
            provider.embed(&request_id, &texts),
            identity.timeout_ms,
            "EMBEDDING_QUERY_TIMEOUT",
        )
        .await;
        let Ok(vectors) = attempt else {
            return Ok((
                Vec::new(),
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
                Vec::new(),
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
        for score in response {
            if !expected.contains(score.chunk_id.as_str())
                || scores.insert(score.chunk_id, score.score).is_some()
                || !score.score.is_finite()
            {
                return degraded_rerank(fused, identity);
            }
        }
        let mut ordered = fused.to_vec();
        ordered.sort_by(|left, right| {
            scores[&right.chunk_id]
                .total_cmp(&scores[&left.chunk_id])
                .then_with(|| right.fusion_score.total_cmp(&left.fusion_score))
                .then_with(|| code_unit_compare(&left.chunk_id, &right.chunk_id))
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
    ) -> RetrievalResult<Option<RetrievalParentContext>> {
        let Some(parent_id) = winner.parent_chunk_id.as_deref() else {
            return Ok(None);
        };
        let Some(parent) = eligible_by_id.get(parent_id).copied() else {
            return Ok(None);
        };
        if parent.text.len() > self.options.max_parent_bytes {
            return Ok(None);
        }
        let bytes = source_bytes.get(&parent.source_path).ok_or_else(|| {
            RetrievalError::ProjectionMismatch(
                "verified parent source bytes are missing".to_owned(),
            )
        })?;
        Ok(Some(RetrievalParentContext {
            chunk_id: parent.chunk_id.clone(),
            text: parent.text.clone(),
            citation: verified_citation(parent, "", bytes)?,
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

pub(crate) fn stage(
    kind: RetrievalProviderStageKind,
    state: RetrievalStageState,
    reasons: &[&str],
    provider: Option<&dyn crate::providers::ProviderStageIdentity>,
) -> RetrievalProviderStageStatus {
    let mut reason_codes = reasons
        .iter()
        .map(|reason| (*reason).to_owned())
        .collect::<Vec<_>>();
    reason_codes.sort_by(|left, right| code_unit_compare(left, right));
    RetrievalProviderStageStatus {
        kind,
        state,
        provider_id: provider.map(|provider| provider.provider_id().to_owned()),
        model_id: provider.map(|provider| provider.model_id().to_owned()),
        reason_codes,
    }
}

pub(crate) fn lexical_stage(
    backend: SqliteLexicalBackend,
    fts5_available: bool,
) -> RetrievalProviderStageStatus {
    match backend {
        SqliteLexicalBackend::SqliteFts5 => stage(
            RetrievalProviderStageKind::SqliteFts5,
            RetrievalStageState::Active,
            &[],
            None,
        ),
        SqliteLexicalBackend::SqliteLexicalScan => {
            let reasons = if fts5_available {
                vec![
                    "SQLITE_LEXICAL_SCAN_ACTIVE",
                    "SQLITE_LEXICAL_SCAN_APPROXIMATION",
                ]
            } else {
                vec![
                    "SQLITE_FTS5_UNAVAILABLE",
                    "SQLITE_LEXICAL_SCAN_ACTIVE",
                    "SQLITE_LEXICAL_SCAN_APPROXIMATION",
                ]
            };
            stage(
                RetrievalProviderStageKind::SqliteLexicalScan,
                RetrievalStageState::Degraded,
                &reasons,
                None,
            )
        }
    }
}

fn policy_allows(policy: &DiscoverabilityPolicy, chunk: &RetrievalChunk) -> bool {
    catch_unwind(AssertUnwindSafe(|| policy(chunk)))
        .ok()
        .and_then(Result::ok)
        == Some(DiscoverabilityDecision::Allow)
}

pub(crate) fn validate_search_request(
    query: &str,
    request: &RetrievalSearchRequest,
) -> RetrievalResult<()> {
    if trim_ecmascript_whitespace(query).is_empty() || query.len() > 4_096 {
        return Err(RetrievalError::InvalidConfig(
            "query must contain from 1 through 4096 UTF-8 bytes".to_owned(),
        ));
    }
    let clauses = lexical_query_clauses(query)?;
    if clauses.len() > 64 || clauses.iter().any(|clause| clause.value.len() > 256) {
        return Err(RetrievalError::InvalidConfig(
            "query term bounds exceeded".to_owned(),
        ));
    }
    let limit = request.limit.unwrap_or(5);
    let lexical = request
        .lexical_top_k
        .unwrap_or(20_u32.max(limit.saturating_mul(4)));
    let semantic = request
        .semantic_top_k
        .unwrap_or(20_u32.max(limit.saturating_mul(4)));
    if !(1..=100).contains(&limit)
        || lexical < limit
        || lexical > 10_000
        || semantic < limit
        || semantic > 10_000
    {
        return Err(RetrievalError::InvalidConfig(
            "search limit or candidate bounds are invalid".to_owned(),
        ));
    }
    if request
        .rrf_k
        .is_some_and(|value| value == 0 || value > 9_007_199_254_740_991)
        || request
            .mmr_lambda
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err(RetrievalError::InvalidConfig(
            "RRF or MMR configuration is invalid".to_owned(),
        ));
    }
    if request
        .parent_expansion_max_child_tokens
        .is_some_and(|value| !(1..=4096).contains(&value))
    {
        return Err(RetrievalError::InvalidConfig(
            "parent expansion child-token threshold is invalid".to_owned(),
        ));
    }
    if request.filters.as_ref().is_some_and(|filters| {
        filters.moc_relationships.is_some() || filters.author_agent_ids.is_some()
    }) {
        // Phase 1 has no endpoint-aware MOC resolver or discoverable agent
        // registry. Even a redacted result would become an existence oracle if
        // callers could probe exact hidden target/agent identifiers in filters.
        return Err(RetrievalError::InvalidConfig(
            "PHASE_1_ENDPOINT_FILTER_AUTHORIZATION_UNAVAILABLE".to_owned(),
        ));
    }
    if let Some(filters) = request.filters.as_ref() {
        validate_retrieval_filters(filters)?;
    }
    Ok(())
}

struct CitationTermGroup {
    primary: String,
    fallback: Vec<String>,
}

fn citation_term_groups(query: &str) -> RetrievalResult<Vec<CitationTermGroup>> {
    let mut groups = Vec::new();
    for clause in lexical_query_clauses(query)? {
        if clause.quoted {
            groups.push(CitationTermGroup {
                primary: normalized_for_fts(&clause.value),
                fallback: clause.tokens,
            });
        } else {
            groups.extend(clause.tokens.into_iter().map(|term| CitationTermGroup {
                primary: term,
                fallback: Vec::new(),
            }));
        }
    }
    Ok(groups)
}

fn chunk_round_trips(chunk: &RetrievalChunk, bytes: &[u8]) -> bool {
    let Ok(start) = usize::try_from(chunk.start_byte) else {
        return false;
    };
    let Ok(end) = usize::try_from(chunk.end_byte) else {
        return false;
    };
    if bytes.get(start..end) != Some(chunk.text.as_bytes()) {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let expected_start_line = line_number(text, start);
    let expected_end_line = line_number(text, end.saturating_sub(1).max(start));
    chunk.start_line == expected_start_line && chunk.end_line == expected_end_line
}

pub(crate) fn verified_citation(
    chunk: &RetrievalChunk,
    query: &str,
    live_bytes: &[u8],
) -> RetrievalResult<SourceCitation> {
    if sha256(live_bytes) != chunk.source_digest || !chunk_round_trips(chunk, live_bytes) {
        return Err(RetrievalError::ProjectionMismatch(
            "STALE_CITATION".to_owned(),
        ));
    }
    let matched_spans = if query.is_empty() {
        Vec::new()
    } else {
        exact_matched_spans(query, chunk, live_bytes)?
    };
    Ok(SourceCitation {
        source_id: chunk.source_id.clone(),
        path: chunk.source_path.clone(),
        source_digest: chunk.source_digest.clone(),
        heading_path: chunk.heading_path.clone(),
        start_byte: chunk.start_byte,
        end_byte: chunk.end_byte,
        start_line: chunk.start_line,
        end_line: chunk.end_line,
        verified: true,
        stale: false,
        matched_spans,
    })
}

/// Remove duplicate evidence introduced by deterministic overlap chunks.
/// Ranked order wins, and evidence is claimed by the caller only after the
/// corresponding hit passes the result-text budget.
pub fn deduplicate_overlap_evidence(
    mut citation: SourceCitation,
    claimed_spans: &BTreeSet<MatchedSpanKey>,
    accepted_intervals: &[AcceptedCitationInterval],
) -> Option<DeduplicatedCitationEvidence> {
    if !citation.matched_spans.is_empty() {
        let source_id = citation.source_id.clone();
        citation.matched_spans.retain(|span| {
            !claimed_spans.contains(&(source_id.clone(), span.start_byte, span.end_byte))
        });
        if citation.matched_spans.is_empty() {
            return None;
        }
        let span_keys = citation
            .matched_spans
            .iter()
            .map(|span| (source_id.clone(), span.start_byte, span.end_byte))
            .collect();
        return Some(DeduplicatedCitationEvidence {
            citation,
            span_keys,
        });
    }
    let overlaps_accepted = accepted_intervals.iter().any(|accepted| {
        accepted.source_id == citation.source_id
            && accepted.start_byte.max(citation.start_byte)
                < accepted.end_byte.min(citation.end_byte)
    });
    (!overlaps_accepted).then_some(DeduplicatedCitationEvidence {
        citation,
        span_keys: Vec::new(),
    })
}

pub fn exact_matched_spans(
    query: &str,
    chunk: &RetrievalChunk,
    live_bytes: &[u8],
) -> RetrievalResult<Vec<MatchedSpan>> {
    if sha256(live_bytes) != chunk.source_digest || !chunk_round_trips(chunk, live_bytes) {
        return Err(RetrievalError::ProjectionMismatch(
            "STALE_CITATION".to_owned(),
        ));
    }
    let mut candidates = Vec::<MatchedSpan>::new();
    let mut returned_bytes = 0_usize;
    let normalized_text = normalized_text_with_original_ranges(&chunk.text);
    let mut add_matches = |normalized_term: &str| -> RetrievalResult<usize> {
        if normalized_term.is_empty() {
            return Ok(0);
        }
        let mut added = 0_usize;
        let mut search_from = 0_usize;
        while let Some(relative) = normalized_text.text[search_from..].find(normalized_term) {
            if candidates.len() >= 16 {
                break;
            }
            let normalized_start = search_from + relative;
            let normalized_end = normalized_start + normalized_term.len();
            search_from = normalized_end.max(normalized_start + 1);
            let Some((original_start, original_end)) =
                normalized_text.original_range(normalized_start, normalized_end)
            else {
                continue;
            };
            let start = usize::try_from(chunk.start_byte)
                .ok()
                .and_then(|base| base.checked_add(original_start))
                .ok_or_else(|| {
                    RetrievalError::ProjectionMismatch("citation offset overflow".to_owned())
                })?;
            let end = usize::try_from(chunk.start_byte)
                .ok()
                .and_then(|base| base.checked_add(original_end))
                .ok_or_else(|| {
                    RetrievalError::ProjectionMismatch("citation offset overflow".to_owned())
                })?;
            let exact = std::str::from_utf8(live_bytes.get(start..end).ok_or_else(|| {
                RetrievalError::ProjectionMismatch("citation slice is outside source".to_owned())
            })?)
            .map_err(|_| {
                RetrievalError::ProjectionMismatch("citation slice is not UTF-8".to_owned())
            })?;
            if exact.len() > 256 || returned_bytes + exact.len() > 1_024 {
                continue;
            }
            candidates.push(MatchedSpan {
                start_byte: start as u64,
                end_byte: end as u64,
                text: exact.to_owned(),
            });
            returned_bytes += exact.len();
            added += 1;
        }
        Ok(added)
    };
    for group in citation_term_groups(query)? {
        if add_matches(&group.primary)? == 0 {
            for fallback in group.fallback {
                add_matches(&fallback)?;
            }
        }
    }
    candidates.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then_with(|| left.end_byte.cmp(&right.end_byte))
    });
    candidates.dedup_by(|left, right| {
        left.start_byte == right.start_byte && left.end_byte == right.end_byte
    });
    candidates.truncate(8);
    Ok(candidates)
}

#[derive(Clone, Debug)]
struct NormalizedRange {
    normalized_start: usize,
    normalized_end: usize,
    original_start: usize,
    original_end: usize,
}

#[derive(Clone, Debug)]
struct NormalizedText {
    text: String,
    ranges: Vec<NormalizedRange>,
}

impl NormalizedText {
    fn original_range(
        &self,
        normalized_start: usize,
        normalized_end: usize,
    ) -> Option<(usize, usize)> {
        if normalized_start >= normalized_end || normalized_end > self.text.len() {
            return None;
        }
        let first = self.ranges.iter().find(|range| {
            range.normalized_start <= normalized_start && normalized_start < range.normalized_end
        })?;
        let last = self.ranges.iter().rev().find(|range| {
            range.normalized_start < normalized_end && normalized_end <= range.normalized_end
        })?;
        Some((first.original_start, last.original_end))
    }
}

fn normalized_for_fts(value: &str) -> String {
    value
        .nfd()
        .filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character == '\u{03c2}' {
                '\u{03c3}'
            } else {
                character
            }
        })
        .collect()
}

fn normalized_text_with_original_ranges(value: &str) -> NormalizedText {
    let mut text = String::new();
    let mut ranges = Vec::<NormalizedRange>::new();
    for (original_start, character) in value.char_indices() {
        let original_end = original_start + character.len_utf8();
        let before = text.len();
        for normalized in character
            .to_string()
            .nfd()
            .filter(|decomposed| !is_combining_mark(*decomposed))
            .flat_map(char::to_lowercase)
            .map(|character| {
                if character == '\u{03c2}' {
                    '\u{03c3}'
                } else {
                    character
                }
            })
        {
            let normalized_start = text.len();
            text.push(normalized);
            ranges.push(NormalizedRange {
                normalized_start,
                normalized_end: text.len(),
                original_start,
                original_end,
            });
        }
        if text.len() == before && is_combining_mark(character) {
            for range in ranges
                .iter_mut()
                .rev()
                .take_while(|range| range.original_end == original_start)
            {
                range.original_end = original_end;
            }
        }
    }
    NormalizedText { text, ranges }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::future::{ready, Future};
    use std::marker::PhantomData;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use std::time::{Duration, Instant};

    use futures_executor::block_on;
    use futures_util::task::noop_waker_ref;
    use serde::Deserialize;
    use tempfile::tempdir;

    use crate::chunker::ChunkingOptions;
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, RetrievalChunkMetadata,
        RetrievalFilters, RetrievalProviderStageKind,
    };
    use crate::digest::canonical_json;
    use crate::providers::{
        ProviderFuture, RerankProviderIdentity, RerankScore, VectorProviderIdentity,
    };
    use crate::sqlite_store::{
        activate_retrieval_generation, harden_directory_permissions, harden_file_permissions,
        write_owner_file,
    };

    use super::*;

    struct MemoryReader(BTreeMap<String, Vec<u8>>);
    impl SourceReader for MemoryReader {
        fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>> {
            self.0
                .get(source_path)
                .cloned()
                .ok_or_else(|| RetrievalError::InvalidEnvelope("source is absent".to_owned()))
        }
    }

    struct CountingReader {
        values: BTreeMap<String, Vec<u8>>,
        reads: AtomicUsize,
    }

    impl SourceReader for CountingReader {
        fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.values
                .get(source_path)
                .cloned()
                .ok_or_else(|| RetrievalError::InvalidEnvelope("source is absent".to_owned()))
        }
    }

    struct FixedVectorProvider {
        identity: VectorProviderIdentity,
    }

    struct CountingVectorProvider {
        identity: VectorProviderIdentity,
        calls: AtomicUsize,
        items: AtomicUsize,
    }

    struct BoundaryCountingVectorProvider {
        identity: VectorProviderIdentity,
        identity_calls: AtomicUsize,
        embed_calls: AtomicUsize,
    }

    struct RecordingVectorProvider {
        identity: VectorProviderIdentity,
        requests: Mutex<Vec<Vec<String>>>,
    }

    struct CountingRerankProvider {
        identity: RerankProviderIdentity,
        calls: AtomicUsize,
    }

    impl VectorProvider for RecordingVectorProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            self.requests.lock().unwrap().push(texts.to_vec());
            Box::pin(ready(Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())))
        }
    }

    impl VectorProvider for CountingVectorProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.items.fetch_add(texts.len(), Ordering::SeqCst);
            Box::pin(ready(Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())))
        }
    }

    impl VectorProvider for BoundaryCountingVectorProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            self.identity_calls.fetch_add(1, Ordering::SeqCst);
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            self.embed_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(ready(Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())))
        }
    }

    impl RerankProvider for CountingRerankProvider {
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
            Box::pin(ready(Ok(Vec::new())))
        }
    }

    struct DropPending<T> {
        dropped: Arc<AtomicBool>,
        output: PhantomData<T>,
    }

    impl<T> DropPending<T> {
        fn new(dropped: Arc<AtomicBool>) -> Self {
            Self {
                dropped,
                output: PhantomData,
            }
        }
    }

    impl<T> Future for DropPending<T> {
        type Output = RetrievalResult<T>;

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    impl<T> Drop for DropPending<T> {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    struct HangingVectorProvider {
        identity: VectorProviderIdentity,
        dropped: Arc<AtomicBool>,
    }

    impl VectorProvider for HangingVectorProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            _texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            Box::pin(DropPending::new(self.dropped.clone()))
        }
    }

    struct HangingRerankProvider {
        identity: RerankProviderIdentity,
        dropped: Arc<AtomicBool>,
    }

    impl RerankProvider for HangingRerankProvider {
        fn identity(&self) -> &RerankProviderIdentity {
            &self.identity
        }

        fn rerank<'a>(
            &'a self,
            _request_id: &'a str,
            _query: &'a str,
            _inputs: &'a [crate::providers::RerankInput],
        ) -> ProviderFuture<'a, Vec<RerankScore>> {
            Box::pin(DropPending::new(self.dropped.clone()))
        }
    }

    impl VectorProvider for FixedVectorProvider {
        fn identity(&self) -> &VectorProviderIdentity {
            &self.identity
        }

        fn embed<'a>(
            &'a self,
            _request_id: &'a str,
            texts: &'a [String],
        ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
            Box::pin(ready(Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())))
        }
    }

    fn source(id: &str, path: &str, text: &str) -> crate::contract::RetrievalSource {
        crate::contract::RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: id.to_owned(),
            source_path: path.to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        }
    }

    fn generation(path: PathBuf) -> RetrievalGenerationInput {
        if path.exists() {
            harden_directory_permissions(&path).unwrap();
        }
        RetrievalGenerationInput {
            state_directory: path,
            engine_version: "2.1.2".to_owned(),
            vault_id: "vault-a".to_owned(),
            source_snapshot_digest: sha256(b"snapshot"),
            configuration_digest: sha256(b"config"),
            policy_digest: sha256(b"policy"),
            sources: vec![source(
                "019b2d14-4230-7db7-87d4-7d81cfaeca01",
                "policy.md",
                "# Policy\nAgents cite canonical decisions.",
            )],
            chunking: ChunkingOptions::default(),
            vectors: Vec::new(),
            embedding_provider_id: None,
            embedding_model_id: None,
            embedding_dimensions: None,
        }
    }

    fn phase3_storage_envelope(name: &str) -> Vec<u8> {
        let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-ingest-validation-1.0.0-draft.1/",
            "storage-conformance-fixture.json"
        )))
        .unwrap();
        let envelope = fixture["valid_envelopes"]
            .get(name)
            .unwrap_or_else(|| panic!("missing Phase-3 fixture envelope {name}"));
        format!("{}\n", canonical_json(envelope).unwrap()).into_bytes()
    }

    fn write_controlled_fixture(path: &Path, bytes: &[u8]) {
        write_owner_file(path, bytes).unwrap();
        harden_file_permissions(path).unwrap();
    }

    fn request() -> RetrievalSearchRequest {
        RetrievalSearchRequest {
            query: "canonical decisions".to_owned(),
            limit: Some(5),
            lexical_top_k: None,
            semantic_top_k: None,
            filters: Some(RetrievalFilters::default()),
            rrf_k: None,
            mmr: Some(true),
            mmr_lambda: None,
            parent_expansion: Some(true),
            parent_expansion_max_child_tokens: None,
        }
    }

    #[test]
    fn malformed_runtime_provider_identities_fail_before_index_or_query_provider_work() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let malformed_vector = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::SqliteFts5,
                provider_id: "custom-provider".to_owned(),
                model_id: "custom-model".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"custom-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        assert!(matches!(
            block_on(index_retrieval_generation(
                generation(state.clone()),
                Some(&malformed_vector),
            )),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("provider-capable family")
        ));
        assert_eq!(malformed_vector.calls.load(Ordering::SeqCst), 0);
        assert_eq!(malformed_vector.items.load(Ordering::SeqCst), 0);
        assert!(!state.exists());

        let input = generation(state.clone());
        let reader = CountingReader {
            values: BTreeMap::from([(
                input.sources[0].source_path.clone(),
                input.sources[0].text.as_bytes().to_vec(),
            )]),
            reads: AtomicUsize::new(0),
        };
        let built = build_retrieval_generation(input).unwrap();
        activate_retrieval_generation(&state, &built).unwrap();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        assert!(matches!(
            RetrievalCoordinator::open_active(
                &state,
                RetrievalCoordinatorOptions {
                    discoverability_policy: &policy,
                    vector_provider: Some(&malformed_vector),
                    rerank_provider: None,
                    source_reader: &reader,
                    stale: false,
                    max_parent_bytes: 8_192,
                    max_result_bytes: MAX_RESULT_BYTES,
                },
            ),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("provider-capable family")
        ));

        let malformed_reranker = CountingRerankProvider {
            identity: RerankProviderIdentity {
                kind: RetrievalProviderStageKind::None,
                provider_id: "custom-reranker".to_owned(),
                model_id: "custom-rerank-model".to_owned(),
                timeout_ms: 15_000,
                configuration_digest: sha256(b"custom-rerank-config"),
            },
            calls: AtomicUsize::new(0),
        };
        assert!(matches!(
            RetrievalCoordinator::open_active(
                &state,
                RetrievalCoordinatorOptions {
                    discoverability_policy: &policy,
                    vector_provider: None,
                    rerank_provider: Some(&malformed_reranker),
                    source_reader: &reader,
                    stale: false,
                    max_parent_bytes: 8_192,
                    max_result_bytes: MAX_RESULT_BYTES,
                },
            ),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("provider-capable family")
        ));
        assert_eq!(malformed_vector.calls.load(Ordering::SeqCst), 0);
        assert_eq!(malformed_reranker.calls.load(Ordering::SeqCst), 0);
        assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn fts_only_search_filters_before_scoring_and_verifies_exact_citations() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let reader = MemoryReader(BTreeMap::from([(
            input.sources[0].source_path.clone(),
            input.sources[0].text.as_bytes().to_vec(),
        )]));
        let built = build_retrieval_generation(input).unwrap();
        activate_retrieval_generation(directory.path(), &built).unwrap();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let coordinator = RetrievalCoordinator::open_active(
            directory.path(),
            RetrievalCoordinatorOptions::fts_only(&policy, &reader),
        )
        .unwrap();
        let result = block_on(coordinator.search(&request())).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.hits[0].citation.verified);
        assert!(!result.hits[0].citation.stale);
        assert_eq!(result.stages.vector.reason_codes, ["VECTOR_DISABLED"]);
        assert_eq!(
            result.stages.lexical.kind,
            RetrievalProviderStageKind::SqliteFts5
        );
        assert_eq!(result.eligible_result_count, 1);
    }

    #[test]
    fn phase_one_hidden_endpoint_filters_are_uniformly_rejected_without_an_oracle() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        input.sources[0].metadata.moc_relationships = Some(vec!["hidden-moc-target".to_owned()]);
        input.sources[0].metadata.author_agent_id = Some("hidden-agent".to_owned());
        let reader = MemoryReader(BTreeMap::from([(
            input.sources[0].source_path.clone(),
            input.sources[0].text.as_bytes().to_vec(),
        )]));
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let coordinator = RetrievalCoordinator::from_verified_store(
            store,
            RetrievalCoordinatorOptions::fts_only(&policy, &reader),
        )
        .unwrap();

        let mut messages = Vec::new();
        for guessed_target in ["hidden-moc-target", "wrong-moc-target"] {
            let mut probe = request();
            probe.filters = Some(RetrievalFilters {
                moc_relationships: Some(vec![guessed_target.to_owned()]),
                ..RetrievalFilters::default()
            });
            messages.push(
                block_on(coordinator.search(&probe))
                    .unwrap_err()
                    .to_string(),
            );
        }
        for guessed_agent in ["hidden-agent", "wrong-agent"] {
            let mut probe = request();
            probe.filters = Some(RetrievalFilters {
                author_agent_ids: Some(vec![guessed_agent.to_owned()]),
                ..RetrievalFilters::default()
            });
            messages.push(
                block_on(coordinator.search(&probe))
                    .unwrap_err()
                    .to_string(),
            );
        }
        assert!(messages
            .iter()
            .all(|message| message.contains("PHASE_1_ENDPOINT_FILTER_AUTHORIZATION_UNAVAILABLE")));
        assert!(messages.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn nested_extension_metadata_is_preserved_in_projection_but_suppressed_from_results() {
        let directory = tempdir().unwrap();
        let mut serialized = serde_json::to_value(source(
            "019b2d14-4230-7db7-87d4-7d81cfaeca01",
            "policy.md",
            "# Policy\nAgents cite canonical decisions.",
        ))
        .unwrap();
        let extension = serde_json::json!({
            "owner": "GKOS-Engine",
            "nested": ["safe", { "depth": 2, "enabled": true }]
        });
        serialized["metadata"]["owner_extension"] = extension.clone();
        let decoded: crate::contract::RetrievalSource = serde_json::from_value(serialized).unwrap();
        assert_eq!(decoded.metadata.extra["owner_extension"], extension);

        let chunks = chunk_source(&decoded, ChunkingOptions::default()).unwrap();
        assert_eq!(chunks[0].metadata.extra["owner_extension"], extension);

        let reader = MemoryReader(BTreeMap::from([(
            decoded.source_path.clone(),
            decoded.text.as_bytes().to_vec(),
        )]));
        let mut input = generation(directory.path().to_path_buf());
        input.sources = vec![decoded];
        input.source_snapshot_digest = sha256(b"metadata-round-trip");
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let stored = store.list_chunks().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].metadata.extra["owner_extension"], extension);
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let coordinator = RetrievalCoordinator::from_verified_store(
            store,
            RetrievalCoordinatorOptions::fts_only(&policy, &reader),
        )
        .unwrap();
        let result = block_on(coordinator.search(&request())).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.hits[0].chunk.metadata.extra.is_empty());
    }

    #[test]
    fn unsafe_numeric_extension_metadata_is_rejected_before_sqlite_publication() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        input.sources[0].metadata.extra.insert(
            "unsafe_integer".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(9_007_199_254_740_992_u64)),
        );
        assert!(matches!(
            build_retrieval_generation(input),
            Err(RetrievalError::InvalidEnvelope(message))
                if message.contains("safe range")
        ));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn result_text_budget_drops_parent_before_retaining_a_fitting_winner() {
        let directory = tempdir().unwrap();
        let text = format!("{}\n# Child\ncanonical", "p".repeat(16_370));
        let mut input = generation(directory.path().to_path_buf());
        input.sources = vec![source(
            "019b2d14-4230-7db7-87d4-7d81cfaeca01",
            "policy.md",
            &text,
        )];
        input.source_snapshot_digest = sha256(text.as_bytes());
        let reader = MemoryReader(BTreeMap::from([(
            "policy.md".to_owned(),
            text.as_bytes().to_vec(),
        )]));
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let mut options = RetrievalCoordinatorOptions::fts_only(&policy, &reader);
        options.max_parent_bytes = 65_536;
        options.max_result_bytes = 16_384;
        let coordinator = RetrievalCoordinator::from_verified_store(store, options).unwrap();
        let mut bounded = request();
        bounded.query = "canonical".to_owned();
        bounded.limit = Some(1);
        let result = block_on(coordinator.search(&bounded)).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.hits[0].parent_context.is_none());
        let returned_text_bytes = result
            .hits
            .iter()
            .map(|hit| {
                hit.chunk.text.len()
                    + hit
                        .parent_context
                        .as_ref()
                        .map_or(0, |parent| parent.text.len())
            })
            .sum::<usize>();
        assert!(returned_text_bytes <= 16_384);
    }

    #[test]
    fn stale_source_bytes_are_removed_before_scoring_and_mark_projection_stale() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let reader = MemoryReader(BTreeMap::from([(
            input.sources[0].source_path.clone(),
            b"modified after indexing".to_vec(),
        )]));
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let coordinator = RetrievalCoordinator::from_verified_store(
            store,
            RetrievalCoordinatorOptions::fts_only(&policy, &reader),
        )
        .unwrap();
        let result = block_on(coordinator.search(&request())).unwrap();
        assert!(result.hits.is_empty());
        assert_eq!(result.eligible_result_count, 0);
        assert_eq!(result.projection_freshness, ProjectionFreshness::Stale);
        assert!(result
            .confidence
            .reason_codes
            .contains(&"STALE_PROJECTION".to_owned()));
    }

    #[test]
    fn embedding_space_mismatch_is_a_hard_rebuild_error() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        let chunks = chunk_source(&input.sources[0], input.chunking).unwrap();
        input.vectors = chunks
            .iter()
            .map(|chunk| StoredVector {
                chunk_id: chunk.chunk_id.clone(),
                vector: vec![1.0, 0.0],
            })
            .collect();
        input.embedding_provider_id = Some("provider-a".to_owned());
        input.embedding_model_id = Some("model-a".to_owned());
        input.embedding_dimensions = Some(2);
        let reader = MemoryReader(BTreeMap::from([(
            input.sources[0].source_path.clone(),
            input.sources[0].text.as_bytes().to_vec(),
        )]));
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let provider = FixedVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-b".to_owned(),
                model_id: "model-b".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"provider-config"),
            },
        };
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let mut options = RetrievalCoordinatorOptions::fts_only(&policy, &reader);
        options.vector_provider = Some(&provider);
        let coordinator = RetrievalCoordinator::from_verified_store(store, options).unwrap();
        assert!(matches!(
            block_on(coordinator.search(&request())),
            Err(RetrievalError::ProjectionMismatch(message))
                if message == "VECTOR_SPACE_MISMATCH_REBUILD_REQUIRED"
        ));
    }

    #[test]
    fn indexing_timeout_drops_provider_work_and_publishes_fts_only() {
        let directory = tempdir().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        let provider = HangingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 20,
                configuration_digest: sha256(b"provider-timeout-config"),
            },
            dropped: dropped.clone(),
        };
        let started = Instant::now();
        let result = block_on(index_retrieval_generation(
            generation(directory.path().to_path_buf()),
            Some(&provider),
        ))
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(result.vector_stage.state, RetrievalStageState::Degraded);
        assert_eq!(result.vector_stage.reason_codes, ["VECTOR_UNAVAILABLE"]);
        assert!(result.generation.manifest.embedding_provider_id.is_none());
    }

    #[test]
    fn dropping_index_future_releases_its_exact_legacy_writer_lock() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let dropped = Arc::new(AtomicBool::new(false));
        let provider = HangingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 300_000,
                configuration_digest: sha256(b"provider-cancellation-config"),
            },
            dropped: dropped.clone(),
        };
        let mut future = Box::pin(index_retrieval_generation(
            generation(state.clone()),
            Some(&provider),
        ));
        let mut context = Context::from_waker(noop_waker_ref());
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        assert!(state.join("retrieval-writer.lock").exists());
        drop(future);
        assert!(dropped.load(Ordering::SeqCst));
        assert!(!state.join("retrieval-writer.lock").exists());
        block_on(index_retrieval_generation(generation(state), None)).unwrap();
    }

    #[test]
    fn mismatched_engine_version_fails_before_provider_or_state() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"provider-engine-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let mut input = generation(state.clone());
        input.engine_version = "forged".to_owned();
        let result = block_on(index_retrieval_generation(input, Some(&provider)));
        calls.store(provider.calls.load(Ordering::SeqCst), Ordering::SeqCst);
        assert!(matches!(result, Err(RetrievalError::InvalidConfig(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!state.exists());
    }

    #[test]
    fn exact_full_phase3_authority_envelopes_block_before_provider_or_state_work() {
        for (filename, envelope) in [
            ("ingest-authority.lock", "authority_lock"),
            ("ingest-attempt-status.json", "attempt_status"),
            ("active-retrieval.json", "legacy_tombstone"),
            ("Ingest-Attempt-Status.json", "attempt_status"),
        ] {
            let directory = tempdir().unwrap();
            let input = generation(directory.path().to_path_buf());
            let path = directory.path().join(filename);
            let bytes = phase3_storage_envelope(envelope);
            write_controlled_fixture(&path, &bytes);
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            let provider = BoundaryCountingVectorProvider {
                identity: VectorProviderIdentity {
                    kind: RetrievalProviderStageKind::Mcp,
                    provider_id: "provider-a".to_owned(),
                    model_id: "model-a".to_owned(),
                    dimensions: 2,
                    timeout_ms: 15_000,
                    configuration_digest: sha256(b"phase3-authority-provider"),
                },
                identity_calls: AtomicUsize::new(0),
                embed_calls: AtomicUsize::new(0),
            };
            assert!(block_on(index_retrieval_generation(input, Some(&provider),)).is_err());
            assert_eq!(provider.identity_calls.load(Ordering::SeqCst), 0);
            assert_eq!(provider.embed_calls.load(Ordering::SeqCst), 0);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn held_legacy_writer_lock_precedes_every_provider_trait_call() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let capability = acquire_legacy_retrieval_writer(&state).unwrap();
        let lock_before = fs::read(state.join("retrieval-writer.lock")).unwrap();
        let provider = BoundaryCountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"held-writer-provider"),
            },
            identity_calls: AtomicUsize::new(0),
            embed_calls: AtomicUsize::new(0),
        };
        assert!(block_on(index_retrieval_generation(
            generation(state.clone()),
            Some(&provider),
        ))
        .is_err());
        assert_eq!(provider.identity_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.embed_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            fs::read(state.join("retrieval-writer.lock")).unwrap(),
            lock_before
        );
        drop(capability);
    }

    #[test]
    fn phase3_evidence_appearing_after_legacy_lock_still_blocks_before_provider() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let provider = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"phase3-interleave-provider"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let identity = provider.identity.clone();
        let mut writer = acquire_legacy_retrieval_writer(directory.path()).unwrap();
        let evidence = directory.path().join("ingest-attempt-status.json");
        write_controlled_fixture(&evidence, &phase3_storage_envelope("attempt_status"));
        let result = block_on(index_retrieval_generation_with_writer(
            input,
            Some(&provider),
            Some(identity),
            &writer,
        ));
        assert!(result.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.items.load(Ordering::SeqCst), 0);
        fs::remove_file(evidence).unwrap();
        assert!(finish_with_writer(result, &mut writer).is_err());
        assert!(!directory.path().join("retrieval-writer.lock").exists());
    }

    #[cfg(unix)]
    #[test]
    fn widened_phase3_state_is_not_repaired_or_read_before_provider_rejection() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let input = generation(state.clone());
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o777)).unwrap();
        let evidence = state.join("ingest-activation-root.json");
        fs::write(&evidence, b"sealed phase3 sentinel\n").unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::read(&evidence).unwrap();
        let provider = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"provider-mode-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        assert!(block_on(index_retrieval_generation(input, Some(&provider))).is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.items.load(Ordering::SeqCst), 0);
        assert_eq!(fs::read(&evidence).unwrap(), before);
        assert_eq!(
            fs::metadata(&state).unwrap().permissions().mode() & 0o777,
            0o777
        );
        assert_eq!(fs::read_dir(&state).unwrap().count(), 1);
    }

    #[test]
    fn query_and_rerank_timeouts_drop_work_and_degrade_without_losing_fts() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        let chunks = chunk_source(&input.sources[0], input.chunking).unwrap();
        input.vectors = chunks
            .iter()
            .map(|chunk| StoredVector {
                chunk_id: chunk.chunk_id.clone(),
                vector: vec![1.0, 0.0],
            })
            .collect();
        input.embedding_provider_id = Some("provider-a".to_owned());
        input.embedding_model_id = Some("model-a".to_owned());
        input.embedding_dimensions = Some(2);
        let reader = MemoryReader(BTreeMap::from([(
            input.sources[0].source_path.clone(),
            input.sources[0].text.as_bytes().to_vec(),
        )]));
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let vector_dropped = Arc::new(AtomicBool::new(false));
        let vector = HangingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::OpenaiCompatible,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 20,
                configuration_digest: sha256(b"vector-timeout-config"),
            },
            dropped: vector_dropped.clone(),
        };
        let rerank_dropped = Arc::new(AtomicBool::new(false));
        let reranker = HangingRerankProvider {
            identity: RerankProviderIdentity {
                kind: RetrievalProviderStageKind::LocalOnnx,
                provider_id: "reranker-a".to_owned(),
                model_id: "reranker-model-a".to_owned(),
                timeout_ms: 20,
                configuration_digest: sha256(b"reranker-timeout-config"),
            },
            dropped: rerank_dropped.clone(),
        };
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let mut options = RetrievalCoordinatorOptions::fts_only(&policy, &reader);
        options.vector_provider = Some(&vector);
        options.rerank_provider = Some(&reranker);
        let coordinator = RetrievalCoordinator::from_verified_store(store, options).unwrap();
        let started = Instant::now();
        let result = block_on(coordinator.search(&request())).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.stages.vector.state, RetrievalStageState::Degraded);
        assert_eq!(result.stages.vector.reason_codes, ["VECTOR_UNAVAILABLE"]);
        assert_eq!(result.stages.reranker.state, RetrievalStageState::Degraded);
        assert_eq!(
            result.stages.reranker.reason_codes,
            ["RERANKER_UNAVAILABLE"]
        );
        assert!(vector_dropped.load(Ordering::SeqCst));
        assert!(rerank_dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn request_bounds_count_quoted_phrases_and_reject_punctuation_only_clauses() {
        let clauses = lexical_query_clauses(r#""two words" plain"#).unwrap();
        assert_eq!(
            clauses
                .iter()
                .map(|clause| clause.value.as_str())
                .collect::<Vec<_>>(),
            ["two words", "plain"]
        );
        assert!(lexical_query_clauses("!!!").is_err());
        let mut request = request();
        request.query = vec!["word"; 65].join(" ");
        assert!(validate_search_request(&request.query, &request).is_err());
        request.query = "a".repeat(257);
        assert!(validate_search_request(&request.query, &request).is_err());
    }

    #[test]
    fn ecmascript_whitespace_controls_exact_provider_query_bytes() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let reader = MemoryReader(BTreeMap::from([(
            input.sources[0].source_path.clone(),
            input.sources[0].text.as_bytes().to_vec(),
        )]));
        let provider = RecordingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "recording-provider".to_owned(),
                model_id: "recording-model".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"recording-provider-config"),
            },
            requests: Mutex::new(Vec::new()),
        };
        let built = block_on(index_retrieval_generation(input, Some(&provider)))
            .unwrap()
            .generation;
        activate_retrieval_generation(directory.path(), &built).unwrap();
        provider.requests.lock().unwrap().clear();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let coordinator = RetrievalCoordinator::open_active(
            directory.path(),
            RetrievalCoordinatorOptions {
                discoverability_policy: &policy,
                vector_provider: Some(&provider),
                rerank_provider: None,
                source_reader: &reader,
                stale: false,
                max_parent_bytes: 8_192,
                max_result_bytes: MAX_RESULT_BYTES,
            },
        )
        .unwrap();

        let mut request = request();
        request.query = "\u{feff}canonical decisions\u{feff}".to_owned();
        block_on(coordinator.search(&request)).unwrap();
        request.query = "canonical\u{0085}decisions".to_owned();
        block_on(coordinator.search(&request)).unwrap();
        assert_eq!(
            *provider.requests.lock().unwrap(),
            vec![
                vec!["canonical decisions".to_owned()],
                vec!["canonical\u{0085}decisions".to_owned()],
            ]
        );
    }

    #[test]
    fn filesystem_reader_rejects_hard_link_source_aliases() {
        let directory = tempdir().unwrap();
        let original = directory.path().join("original.md");
        let alias = directory.path().join("alias.md");
        fs::write(&original, b"# Canonical\ntext").unwrap();
        fs::hard_link(&original, &alias).unwrap();
        let reader = FileSourceReader::new(directory.path()).unwrap();
        assert!(matches!(
            reader.read("original.md"),
            Err(RetrievalError::InvalidEnvelope(message))
                if message.contains("aliases")
        ));
    }

    #[test]
    fn filesystem_reader_normalizes_ordinary_dot_and_parent_components() {
        let directory = tempdir().unwrap();
        let vault = directory.path().join("vault");
        fs::create_dir(&vault).unwrap();
        let spelling = vault.join(".").join("..").join("vault");
        FileSourceReader::new(&spelling).unwrap();
    }

    #[test]
    fn filesystem_reader_rejects_symlink_or_junction_root_alias() {
        let directory = tempdir().unwrap();
        let vault = directory.path().join("vault");
        let alias = directory.path().join("vault-alias");
        fs::create_dir(&vault).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&vault, &alias).unwrap();

        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(&vault, &alias) {
            // Windows may deny symlink creation when Developer Mode or the
            // create-symbolic-link privilege is unavailable locally. Hosted
            // qualification sets GKOS_REQUIRE_ALIAS_FIXTURE=1, which turns
            // that environment limitation into a required test failure.
            if matches!(error.raw_os_error(), Some(5 | 1314)) {
                if std::env::var_os("GKOS_REQUIRE_ALIAS_FIXTURE").as_deref()
                    == Some(std::ffi::OsStr::new("1"))
                {
                    panic!("required Windows root-alias fixture unavailable: {error}");
                }
                return;
            }
            panic!("failed to construct root alias fixture: {error}");
        }

        #[cfg(not(any(unix, windows)))]
        return;

        assert!(matches!(
            FileSourceReader::new(&alias),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("real directory") || message.contains("aliases")
        ));
    }

    #[test]
    fn verified_active_cache_embeds_zero_unchanged_and_only_one_changed_section() {
        let directory = tempdir().unwrap();
        let provider = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::LocalOnnx,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"provider-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let mut first_input = generation(directory.path().to_path_buf());
        first_input.sources = vec![source(
            "019b2d14-4230-7db7-87d4-7d81cfaeca01",
            "policy.md",
            "# Stable\nunchanged section.\n# Mutable\nversion one.",
        )];
        let first = block_on(index_retrieval_generation(
            first_input.clone(),
            Some(&provider),
        ))
        .unwrap()
        .generation;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.items.load(Ordering::SeqCst), 2);
        activate_retrieval_generation(directory.path(), &first).unwrap();

        block_on(index_retrieval_generation(
            first_input.clone(),
            Some(&provider),
        ))
        .unwrap();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.items.load(Ordering::SeqCst), 2);

        let other_vault_provider = CountingVectorProvider {
            identity: provider.identity.clone(),
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let mut other_vault = first_input.clone();
        other_vault.vault_id = "vault-b".to_owned();
        other_vault.sources[0].vault_id = "vault-b".to_owned();
        other_vault.source_snapshot_digest = sha256(b"vault-b-snapshot");
        block_on(index_retrieval_generation(
            other_vault,
            Some(&other_vault_provider),
        ))
        .unwrap();
        assert_eq!(other_vault_provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(other_vault_provider.items.load(Ordering::SeqCst), 2);

        let changed_text = "# Stable\nunchanged section.\n# Mutable\nversion two.";
        first_input.sources[0].text = changed_text.to_owned();
        first_input.sources[0].source_digest = sha256(changed_text.as_bytes());
        first_input.source_snapshot_digest = sha256(b"changed-snapshot");
        block_on(index_retrieval_generation(first_input, Some(&provider))).unwrap();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(provider.items.load(Ordering::SeqCst), 3);

        let other_space = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-b".to_owned(),
                model_id: "model-b".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"other-provider-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        block_on(index_retrieval_generation(
            {
                let mut input = generation(directory.path().to_path_buf());
                input.sources = vec![source(
                    "019b2d14-4230-7db7-87d4-7d81cfaeca01",
                    "policy.md",
                    "# Stable\nunchanged section.\n# Mutable\nversion one.",
                )];
                input
            },
            Some(&other_space),
        ))
        .unwrap();
        assert_eq!(other_space.calls.load(Ordering::SeqCst), 1);
        assert_eq!(other_space.items.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn corrupt_active_pointer_fails_authority_preflight_before_provider_or_generation_work() {
        let directory = tempdir().unwrap();
        let working = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"provider-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let input = generation(directory.path().to_path_buf());
        let first = block_on(index_retrieval_generation(input.clone(), Some(&working)))
            .unwrap()
            .generation;
        activate_retrieval_generation(directory.path(), &first).unwrap();
        fs::write(directory.path().join("active-retrieval.json"), b"{broken").unwrap();
        let provider = CountingVectorProvider {
            identity: working.identity.clone(),
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let before = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        assert!(matches!(
            block_on(index_retrieval_generation(input, Some(&provider))),
            Err(RetrievalError::InvalidConfig(message))
                if message == "RETRIEVAL_STATE_POINTER_JSON_INVALID"
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.items.load(Ordering::SeqCst), 0);
        let after = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn aliased_active_cache_fails_before_provider_or_state_write() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let built = build_retrieval_generation(input.clone()).unwrap();
        activate_retrieval_generation(directory.path(), &built).unwrap();
        fs::hard_link(
            directory.path().join("active-retrieval.json"),
            directory.path().join("active-pointer-alias.json"),
        )
        .unwrap();
        let before = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        let provider = CountingVectorProvider {
            identity: VectorProviderIdentity {
                kind: RetrievalProviderStageKind::Mcp,
                provider_id: "provider-a".to_owned(),
                model_id: "model-a".to_owned(),
                dimensions: 2,
                timeout_ms: 15_000,
                configuration_digest: sha256(b"provider-config"),
            },
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        assert!(matches!(
            block_on(index_retrieval_generation(input, Some(&provider))),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("HARDLINK") || message.contains("ALIAS")
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.items.load(Ordering::SeqCst), 0);
        let after = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn normalized_citation_matching_returns_exact_full_fixture_utf8() {
        #[derive(Deserialize)]
        struct Root {
            citation_normalization: Vec<CitationCase>,
        }
        #[derive(Deserialize)]
        struct CitationCase {
            id: String,
            source_text: String,
            query: String,
            expected_spans: Vec<MatchedSpan>,
        }
        let fixture: Root = serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
            "conformance-fixture.json"
        )))
        .unwrap();
        for case in fixture.citation_normalization {
            let source = source(
                "018f0000-0000-7000-8000-000000000226",
                "accent.md",
                &case.source_text,
            );
            let chunk = chunk_source(&source, ChunkingOptions::default())
                .unwrap()
                .remove(0);
            let spans =
                exact_matched_spans(&case.query, &chunk, case.source_text.as_bytes()).unwrap();
            assert_eq!(spans, case.expected_spans, "{}", case.id);
            for span in spans {
                assert_eq!(
                    case.source_text
                        .as_bytes()
                        .get(span.start_byte as usize..span.end_byte as usize),
                    Some(span.text.as_bytes()),
                    "{}",
                    case.id
                );
            }
        }
    }

    #[test]
    fn malformed_filter_and_raw_query_bounds_fail_before_policy_source_or_provider_work() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let reader = CountingReader {
            values: BTreeMap::from([(
                input.sources[0].source_path.clone(),
                input.sources[0].text.as_bytes().to_vec(),
            )]),
            reads: AtomicUsize::new(0),
        };
        let identity = VectorProviderIdentity {
            kind: RetrievalProviderStageKind::Mcp,
            provider_id: "bounded-filter-provider".to_owned(),
            model_id: "bounded-filter-model".to_owned(),
            dimensions: 2,
            timeout_ms: 15_000,
            configuration_digest: sha256(b"bounded-filter-provider-config"),
        };
        let indexing_provider = FixedVectorProvider {
            identity: identity.clone(),
        };
        let built = block_on(index_retrieval_generation(input, Some(&indexing_provider)))
            .unwrap()
            .generation;
        activate_retrieval_generation(directory.path(), &built).unwrap();

        let query_provider = CountingVectorProvider {
            identity,
            calls: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        };
        let policy_calls = Arc::new(AtomicUsize::new(0));
        let recorded_policy_calls = policy_calls.clone();
        let policy = move |_chunk: &RetrievalChunk| {
            recorded_policy_calls.fetch_add(1, Ordering::SeqCst);
            Ok(DiscoverabilityDecision::Allow)
        };
        let coordinator = RetrievalCoordinator::open_active(
            directory.path(),
            RetrievalCoordinatorOptions {
                discoverability_policy: &policy,
                vector_provider: Some(&query_provider),
                rerank_provider: None,
                source_reader: &reader,
                stale: false,
                max_parent_bytes: 8_192,
                max_result_bytes: MAX_RESULT_BYTES,
            },
        )
        .unwrap();

        let invalid_filters = [
            (
                "too-many-tags",
                RetrievalFilters {
                    tags_any: Some(vec!["tag".to_owned(); 257]),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "empty-tag",
                RetrievalFilters {
                    tags_all: Some(vec![String::new()]),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "oversized-topic",
                RetrievalFilters {
                    topics: Some(vec!["x".repeat(1_025)]),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "nul-vault",
                RetrievalFilters {
                    vault: Some("vault\0guess".to_owned()),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "repeated-slash-glob",
                RetrievalFilters {
                    path_include: Some(vec!["notes//hidden/**".to_owned()]),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "drive-glob",
                RetrievalFilters {
                    path_exclude: Some(vec!["C:/hidden/**".to_owned()]),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "uppercase-digest",
                RetrievalFilters {
                    source_digests: Some(vec![format!("sha256:{}", "A".repeat(64))]),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "invalid-from",
                RetrievalFilters {
                    authored_from: Some("2026-13-01T00:00:00Z".to_owned()),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "invalid-to",
                RetrievalFilters {
                    authored_to: Some("not-a-timestamp".to_owned()),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "nan-quality",
                RetrievalFilters {
                    minimum_quality: Some(f64::NAN),
                    ..RetrievalFilters::default()
                },
            ),
            (
                "quality-over-one",
                RetrievalFilters {
                    minimum_quality: Some(1.000_001),
                    ..RetrievalFilters::default()
                },
            ),
        ];
        for (case_id, filters) in invalid_filters {
            let mut probe = request();
            probe.filters = Some(filters);
            let result = block_on(coordinator.search(&probe));
            assert!(
                matches!(
                    &result,
                    Err(RetrievalError::InvalidConfig(message))
                        if message.starts_with("RETRIEVAL_FILTER_INVALID:")
                ),
                "{case_id}: {result:?}"
            );
        }

        let mut oversized_raw_query = request();
        oversized_raw_query.query = format!("{}x", " ".repeat(4_096));
        assert!(matches!(
            block_on(coordinator.search(&oversized_raw_query)),
            Err(RetrievalError::InvalidConfig(message))
                if message == "query must contain from 1 through 4096 UTF-8 bytes"
        ));

        assert_eq!(policy_calls.load(Ordering::SeqCst), 0);
        assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
        assert_eq!(query_provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(query_provider.items.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn citation_verification_recomputes_live_line_coordinates_for_lf_crlf_and_utf8() {
        let cases = [
            ("lf-precomposed", "preface\n# H\nCafé 😀 needle\n", "café"),
            (
                "crlf-combining",
                "preface\r\n# H\r\nCafe\u{301} 😀 needle\r\n",
                "cafe",
            ),
        ];

        for (case_id, text, query) in cases {
            let source = source(
                "018f0000-0000-7000-8000-000000000227",
                "line-coordinates.md",
                text,
            );
            let chunk = chunk_source(&source, ChunkingOptions::default())
                .unwrap()
                .into_iter()
                .find(|chunk| chunk.heading_path.last().map(String::as_str) == Some("H"))
                .unwrap();

            assert_eq!(chunk.start_line, 2, "{case_id}");
            assert_eq!(chunk.end_line, 3, "{case_id}");
            let citation = verified_citation(&chunk, query, text.as_bytes()).unwrap();
            assert!(citation.verified, "{case_id}");
            assert!(!citation.stale, "{case_id}");
            assert_eq!(citation.start_line, 2, "{case_id}");
            assert_eq!(citation.end_line, 3, "{case_id}");
            assert_eq!(
                &text.as_bytes()[citation.start_byte as usize..citation.end_byte as usize],
                chunk.text.as_bytes(),
                "{case_id}"
            );
            assert!(!citation.matched_spans.is_empty(), "{case_id}");
            for span in &citation.matched_spans {
                assert_eq!(
                    &text.as_bytes()[span.start_byte as usize..span.end_byte as usize],
                    span.text.as_bytes(),
                    "{case_id}"
                );
            }

            let mut forged_start = chunk.clone();
            forged_start.start_line += 1;
            assert!(matches!(
                verified_citation(&forged_start, query, text.as_bytes()),
                Err(RetrievalError::ProjectionMismatch(message)) if message == "STALE_CITATION"
            ));

            let mut forged_end = chunk.clone();
            forged_end.end_line += 1;
            assert!(matches!(
                verified_citation(&forged_end, query, text.as_bytes()),
                Err(RetrievalError::ProjectionMismatch(message)) if message == "STALE_CITATION"
            ));
        }
    }

    #[test]
    fn overlap_evidence_rule_matches_the_frozen_full_fixture() {
        #[derive(Deserialize)]
        struct Root {
            overlap_result_dedup: OverlapFixture,
        }
        #[derive(Deserialize)]
        struct OverlapFixture {
            candidates: Vec<OverlapCandidate>,
            expected_chunk_ids: Vec<String>,
            expected_claimed_spans: Vec<ExpectedClaim>,
        }
        #[derive(Deserialize)]
        struct OverlapCandidate {
            chunk_id: String,
            source_id: String,
            start_byte: u64,
            end_byte: u64,
            matched_spans: Vec<FixtureSpan>,
        }
        #[derive(Deserialize)]
        struct FixtureSpan {
            start_byte: u64,
            end_byte: u64,
        }
        #[derive(Deserialize)]
        struct ExpectedClaim {
            source_id: String,
            start_byte: u64,
            end_byte: u64,
        }
        let fixture: Root = serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
            "conformance-fixture.json"
        )))
        .unwrap();
        let mut claimed = BTreeSet::new();
        let mut intervals = Vec::new();
        let mut accepted_ids = Vec::new();
        let mut claimed_in_rank_order = Vec::new();
        for candidate in fixture.overlap_result_dedup.candidates {
            let citation = SourceCitation {
                source_id: candidate.source_id.clone(),
                path: "fixture.md".to_owned(),
                source_digest: sha256(b"fixture"),
                heading_path: Vec::new(),
                start_byte: candidate.start_byte,
                end_byte: candidate.end_byte,
                start_line: 1,
                end_line: 1,
                verified: true,
                stale: false,
                matched_spans: candidate
                    .matched_spans
                    .iter()
                    .map(|span| MatchedSpan {
                        start_byte: span.start_byte,
                        end_byte: span.end_byte,
                        text: "fixture".to_owned(),
                    })
                    .collect(),
            };
            let Some(evidence) = deduplicate_overlap_evidence(citation, &claimed, &intervals)
            else {
                continue;
            };
            accepted_ids.push(candidate.chunk_id);
            claimed_in_rank_order.extend(evidence.span_keys.iter().cloned());
            claimed.extend(evidence.span_keys);
            intervals.push(AcceptedCitationInterval {
                source_id: candidate.source_id,
                start_byte: candidate.start_byte,
                end_byte: candidate.end_byte,
            });
        }
        assert_eq!(
            accepted_ids,
            fixture.overlap_result_dedup.expected_chunk_ids
        );
        assert_eq!(
            claimed_in_rank_order,
            fixture
                .overlap_result_dedup
                .expected_claimed_spans
                .into_iter()
                .map(|span| (span.source_id, span.start_byte, span.end_byte))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn overlapping_chunks_return_one_stable_exact_needle_span() {
        let directory = tempdir().unwrap();
        let text = "# Overlap\none two three four five six seven eight nine ten eleven twelve thirteen NEEDLE fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive\n";
        let retrieval_source = source("018f0000-0000-7000-8000-000000000225", "overlap.md", text);
        let chunking = ChunkingOptions {
            max_tokens: 16,
            overlap_tokens: 4,
        };
        let chunks = chunk_source(&retrieval_source, chunking).unwrap();
        assert_eq!(
            chunks
                .iter()
                .filter(|chunk| chunk.text.contains("NEEDLE"))
                .count(),
            2
        );
        let reader = MemoryReader(BTreeMap::from([(
            retrieval_source.source_path.clone(),
            text.as_bytes().to_vec(),
        )]));
        let mut input = generation(directory.path().to_path_buf());
        input.sources = vec![retrieval_source];
        input.chunking = chunking;
        input.source_snapshot_digest = sha256(text.as_bytes());
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let policy = |_chunk: &RetrievalChunk| Ok(DiscoverabilityDecision::Allow);
        let coordinator = RetrievalCoordinator::from_verified_store(
            store,
            RetrievalCoordinatorOptions::fts_only(&policy, &reader),
        )
        .unwrap();
        let mut search = request();
        search.query = "NEEDLE".to_owned();
        search.mmr = Some(false);
        let first = block_on(coordinator.search(&search)).unwrap();
        let second = block_on(coordinator.search(&search)).unwrap();
        assert_eq!(first.hits.len(), 1);
        assert_eq!(
            first
                .hits
                .iter()
                .map(|hit| hit.chunk.chunk_id.clone())
                .collect::<Vec<_>>(),
            second
                .hits
                .iter()
                .map(|hit| hit.chunk.chunk_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first.hits[0]
                .citation
                .matched_spans
                .iter()
                .map(|span| (span.start_byte, span.end_byte, span.text.as_str()))
                .collect::<Vec<_>>(),
            [(82, 88, "NEEDLE")]
        );
    }
}
