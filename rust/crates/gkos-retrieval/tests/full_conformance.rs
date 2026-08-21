use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use gkos_retrieval_lite::chunker::{chunk_source, ChunkingOptions};
use gkos_retrieval_lite::confidence::assess_retrieval_confidence;
use gkos_retrieval_lite::contract::{
    is_valid_retrieval_source_path, CanonicalLineageEnvelope, CanonicalTemporalEnvelope,
    DiscoverabilityDecision, MatchedSpan, RankedCandidate, RetrievalChunk, RetrievalChunkMetadata,
    RetrievalConfidence, RetrievalProviderStageStatus, RetrievalSource, RetrievalStageScores,
    SourceCitation, SqliteLexicalBackend, PARENT_EXPANSION_MAX_CHILD_TOKENS,
    PROJECTION_SCHEMA_VERSION, RETRIEVAL_CONTRACT,
};
use gkos_retrieval_lite::coordinator::{
    deduplicate_overlap_evidence, exact_matched_spans, AcceptedCitationInterval,
};
use gkos_retrieval_lite::digest::{canonical_json, sha256};
use gkos_retrieval_lite::filters::{is_valid_retrieval_timestamp, matches_path_glob};
use gkos_retrieval_lite::fusion::{
    maximal_marginal_relevance, maximal_marginal_relevance_with_relevance, reciprocal_rank_fusion,
    RankedInput,
};
use gkos_retrieval_lite::{lexical_field_score, normalized_lexical_terms};

const CONTRACT_ROOT: &str = "../../../contracts/gkos-retrieval-1.0.0-draft.1";
const FIXTURE_BYTES: &[u8] = include_bytes!(concat!(
    "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
    "conformance-fixture.json"
));
const CANONICAL_FIXTURE_BYTES: &[u8] = include_bytes!(concat!(
    "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
    "canonical-fixture.json"
));
const TOML_LEXICAL_FIXTURE_BYTES: &[u8] = include_bytes!(concat!(
    "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
    "gkos-toml-lexical-fixture.json"
));
const FULL_PIN_BYTES: &[u8] = include_bytes!(concat!(
    "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
    "FULL-PIN.json"
));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    contract_version: String,
    input: FixtureInput,
    options: FixtureOptions,
    expected_chunks: Vec<RetrievalChunk>,
    rrf: RrfFixture,
    mmr: MmrFixture,
    mmr_negative_cosine: ExtendedMmrFixture,
    rerank_mmr: RerankMmrFixture,
    parent_expansion: ParentExpansionFixture,
    overlap_result_dedup: OverlapRuleFixture,
    path_glob_filters: Vec<PathGlobFixture>,
    citation_normalization: Vec<CitationNormalizationFixture>,
    lexical_backends: LexicalBackendsFixture,
    confidence_zero_signal: ConfidenceZeroSignalFixture,
    lexical: LexicalFixture,
}

#[derive(Deserialize)]
struct FixtureInput {
    source_id: String,
    source_path: String,
    text: String,
    metadata: RetrievalChunkMetadata,
}

#[derive(Deserialize)]
struct FixtureOptions {
    max_tokens: usize,
    overlap_tokens: usize,
}

#[derive(Deserialize)]
struct RrfFixture {
    k: u64,
    lexical: Vec<RankedInput>,
    semantic: Vec<RankedInput>,
    expected: Vec<RankedCandidate>,
}

#[derive(Deserialize)]
struct MmrFixture {
    lambda: f64,
    limit: usize,
    expected: Vec<RankedCandidate>,
}

#[derive(Deserialize)]
struct ExtendedMmrFixture {
    lambda: f64,
    limit: usize,
    input: Vec<RankedCandidate>,
    expected_chunk_ids: Vec<String>,
    expected_mmr_scores: Vec<f64>,
}

#[derive(Deserialize)]
struct RerankMmrFixture {
    lambda: f64,
    limit: usize,
    input: Vec<RankedCandidate>,
    reranker_ranks: Vec<RerankerRank>,
    expected_chunk_ids: Vec<String>,
    expected_mmr_scores: Vec<f64>,
}

#[derive(Deserialize)]
struct RerankerRank {
    chunk_id: String,
    rank: u32,
}

#[derive(Deserialize)]
struct ParentExpansionFixture {
    default_max_child_tokens: u32,
    examples: Vec<ParentExpansionExample>,
}

#[derive(Deserialize)]
struct ParentExpansionExample {
    child_token_count: u32,
    expand: bool,
}

#[derive(Deserialize)]
struct OverlapRuleFixture {
    intervals: String,
    rank_order: String,
    candidates: Vec<OverlapCandidate>,
    expected_chunk_ids: Vec<String>,
    expected_claimed_spans: Vec<ClaimedSpan>,
}

#[derive(Deserialize)]
struct OverlapCandidate {
    chunk_id: String,
    source_id: String,
    start_byte: u64,
    end_byte: u64,
    matched_spans: Vec<SpanCoordinates>,
}

#[derive(Deserialize)]
struct SpanCoordinates {
    start_byte: u64,
    end_byte: u64,
}

#[derive(Deserialize)]
struct ClaimedSpan {
    source_id: String,
    start_byte: u64,
    end_byte: u64,
}

#[derive(Deserialize)]
struct PathGlobFixture {
    path: String,
    glob: String,
    matches: bool,
}

#[derive(Deserialize)]
struct CitationNormalizationFixture {
    id: String,
    source_text: String,
    query: String,
    expected_spans: Vec<MatchedSpan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LexicalBackendsFixture {
    projection_schema_version: u32,
    preferred: PreferredLexicalBackendFixture,
    compatibility: CompatibilityLexicalBackendFixture,
    differential_rows: Vec<DifferentialLexicalRow>,
    differential_queries: Vec<DifferentialLexicalQuery>,
    rejected_queries: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreferredLexicalBackendFixture {
    manifest_value: SqliteLexicalBackend,
    stage: RetrievalProviderStageStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityLexicalBackendFixture {
    manifest_value: SqliteLexicalBackend,
    stage_without_fts5: RetrievalProviderStageStatus,
    stage_when_explicit_on_fts5_runtime: RetrievalProviderStageStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DifferentialLexicalRow {
    chunk_id: String,
    title: String,
    heading_path: String,
    tags: String,
    topic: String,
    category: String,
    text: String,
    token_count: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DifferentialLexicalQuery {
    query: String,
    expected: Vec<DifferentialLexicalExpected>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DifferentialLexicalExpected {
    chunk_id: String,
    score: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidenceZeroSignalFixture {
    scores: Vec<ConfidenceScoreFixture>,
    stages: ConfidenceStagesFixture,
    eligible_count: usize,
    expected: RetrievalConfidence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidenceScoreFixture {
    chunk_id: String,
    lexical_score: Option<f64>,
    semantic_score: Option<f64>,
    fusion_score: f64,
    reranker_score: Option<f64>,
    mmr_score: Option<f64>,
    lexical_rank: Option<u32>,
    semantic_rank: Option<u32>,
    fused_rank: u32,
    reranker_rank: Option<u32>,
    final_rank: u32,
}

impl ConfidenceScoreFixture {
    fn stage_scores(&self) -> RetrievalStageScores {
        RetrievalStageScores {
            lexical_score: self.lexical_score,
            semantic_score: self.semantic_score,
            fusion_score: self.fusion_score,
            reranker_score: self.reranker_score,
            mmr_score: self.mmr_score,
            lexical_rank: self.lexical_rank,
            semantic_rank: self.semantic_rank,
            fused_rank: self.fused_rank,
            reranker_rank: self.reranker_rank,
            final_rank: self.final_rank,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidenceStagesFixture {
    vector: RetrievalProviderStageStatus,
    reranker: RetrievalProviderStageStatus,
}

#[derive(Deserialize)]
struct LexicalFixture {
    query: String,
    fields: LexicalFields,
    expected_terms: Vec<String>,
    expected_score: f64,
}

#[derive(Deserialize)]
struct LexicalFields {
    title: String,
    heading_path: String,
    tags: String,
    topic: String,
    category: String,
    text: String,
    token_count: u32,
}

#[derive(Deserialize)]
struct CanonicalFixture {
    contract_version: String,
    cases: Vec<CanonicalCase>,
    rejected_numbers: Vec<String>,
    portable_source_paths: Vec<AcceptanceCase>,
    timestamp_filters: Vec<AcceptanceCase>,
}

#[derive(Deserialize)]
struct CanonicalCase {
    id: String,
    canonical_json: String,
    sha256: String,
}

#[derive(Deserialize)]
struct AcceptanceCase {
    #[serde(alias = "path", alias = "value")]
    input: String,
    accepted: bool,
}

fn fixture() -> Fixture {
    serde_json::from_slice(FIXTURE_BYTES).expect("Full conformance fixture must parse")
}

fn canonical_fixture() -> CanonicalFixture {
    serde_json::from_slice(CANONICAL_FIXTURE_BYTES).expect("canonical fixture must parse")
}

#[test]
fn imported_full_contract_pack_has_the_pinned_exact_bytes() {
    let pin: serde_json::Value =
        serde_json::from_slice(FULL_PIN_BYTES).expect("Full commit pin must parse");
    assert_eq!(
        pin["reference_commit"],
        "5b72aae1aad5b6416b8cb86a4137a7e536d8bb59"
    );
    assert_eq!(pin["reference_package_version"], "2.1.2");
    assert_eq!(pin["contract_version"], RETRIEVAL_CONTRACT);
    assert!(FULL_PIN_BYTES.ends_with(b"\n"));
    assert!(!FULL_PIN_BYTES.ends_with(b"\n\n"));
    let files = [
        (
            CANONICAL_FIXTURE_BYTES,
            "sha256:f30dd5c3e71407e6544b9c727ff5597c4809936dbbd14a5fdca87dcb99031db2",
        ),
        (
            include_bytes!(concat!(
                "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
                "README.md"
            ))
            .as_slice(),
            "sha256:2028882032f2292bd0bbc937016a128449babf5f9adc395824029ee0047cc942",
        ),
        (
            include_bytes!(concat!(
                "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
                "chunk.schema.json"
            ))
            .as_slice(),
            "sha256:2474a40e8abc930cbc6e713aa8966be41f0fba87c5b38c5868b42403e8f3f721",
        ),
        (
            FIXTURE_BYTES,
            "sha256:462de9f327585ec2eed019a4b728403b6c0bbfa3ade158ed618cafd214a4b009",
        ),
        (
            include_bytes!(concat!(
                "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
                "contract.json"
            ))
            .as_slice(),
            "sha256:418fffcf3954c634453c3f3e8dd756dd2636ee0030d9ecf6c4ccb5147b8d0c6e",
        ),
        (
            TOML_LEXICAL_FIXTURE_BYTES,
            "sha256:abdb26527fd5c047db96801c22ebf30efca544fe306744c666b858ba57bd039b",
        ),
        (
            include_bytes!(concat!(
                "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
                "gkos-config.schema.json"
            ))
            .as_slice(),
            "sha256:e42fe89d102ec602b0738aa01a3c8f98cc8fb55d9edc25265f6feb168a0ca8d6",
        ),
        (
            include_bytes!(concat!(
                "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
                "projection.schema.json"
            ))
            .as_slice(),
            "sha256:99f7eb70530dd44866c8c28b71f97d9d76af2e00b8ea399dc4f2e5f3e6467fa3",
        ),
        (
            include_bytes!(concat!(
                "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
                "result.schema.json"
            ))
            .as_slice(),
            "sha256:b9bb7e360fa04ee1e0b75984d313cd63488ef698149ff0b3f29b3c003126faa3",
        ),
    ];
    for (bytes, expected) in files {
        assert_eq!(sha256(bytes), expected);
        assert!(bytes.ends_with(b"\n"));
        assert!(!bytes.ends_with(b"\n\n"));
    }
    assert!(CONTRACT_ROOT.ends_with("gkos-retrieval-1.0.0-draft.1"));
}

#[test]
fn rust_chunker_matches_full_expected_chunks_exactly() {
    let fixture = fixture();
    assert_eq!(fixture.contract_version, RETRIEVAL_CONTRACT);
    let source = RetrievalSource {
        contract_version: fixture.contract_version,
        vault_id: "full-conformance-fixture".to_owned(),
        source_id: fixture.input.source_id,
        source_path: fixture.input.source_path,
        source_digest: sha256(fixture.input.text.as_bytes()),
        text: fixture.input.text,
        discoverability: DiscoverabilityDecision::Allow,
        lineage: CanonicalLineageEnvelope::default(),
        temporal: CanonicalTemporalEnvelope::default(),
        metadata: fixture.input.metadata,
    };
    let chunks = chunk_source(
        &source,
        ChunkingOptions {
            max_tokens: fixture.options.max_tokens,
            overlap_tokens: fixture.options.overlap_tokens,
        },
    )
    .unwrap();
    assert_eq!(chunks, fixture.expected_chunks);
}

#[test]
fn rust_rrf_matches_full_expected_binary64_values() {
    let fixture = fixture();
    let actual =
        reciprocal_rank_fusion(&fixture.rrf.lexical, &fixture.rrf.semantic, fixture.rrf.k).unwrap();
    assert_eq!(actual, fixture.rrf.expected);
}

#[test]
fn rust_mmr_matches_full_expected_binary64_values() {
    let fixture = fixture();
    let fused =
        reciprocal_rank_fusion(&fixture.rrf.lexical, &fixture.rrf.semantic, fixture.rrf.k).unwrap();
    let actual = maximal_marginal_relevance(&fused, fixture.mmr.limit, fixture.mmr.lambda).unwrap();
    assert_eq!(actual, fixture.mmr.expected);
}

#[test]
fn rust_mmr_matches_negative_cosine_and_reranker_rank_rules() {
    let fixture = fixture();
    let negative = maximal_marginal_relevance(
        &fixture.mmr_negative_cosine.input,
        fixture.mmr_negative_cosine.limit,
        fixture.mmr_negative_cosine.lambda,
    )
    .unwrap();
    assert_eq!(
        negative
            .iter()
            .map(|candidate| candidate.chunk_id.clone())
            .collect::<Vec<_>>(),
        fixture.mmr_negative_cosine.expected_chunk_ids
    );
    assert_eq!(
        negative
            .iter()
            .map(|candidate| candidate.mmr_score.unwrap())
            .collect::<Vec<_>>(),
        fixture.mmr_negative_cosine.expected_mmr_scores
    );

    let relevance = fixture
        .rerank_mmr
        .reranker_ranks
        .iter()
        .map(|item| (item.chunk_id.clone(), 1.0 / f64::from(item.rank)))
        .collect::<BTreeMap<_, _>>();
    let reranked = maximal_marginal_relevance_with_relevance(
        &fixture.rerank_mmr.input,
        fixture.rerank_mmr.limit,
        fixture.rerank_mmr.lambda,
        Some(&relevance),
    )
    .unwrap();
    assert_eq!(
        reranked
            .iter()
            .map(|candidate| candidate.chunk_id.clone())
            .collect::<Vec<_>>(),
        fixture.rerank_mmr.expected_chunk_ids
    );
    assert_eq!(
        reranked
            .iter()
            .map(|candidate| candidate.mmr_score.unwrap())
            .collect::<Vec<_>>(),
        fixture.rerank_mmr.expected_mmr_scores
    );
}

#[test]
fn rust_parent_and_lexical_rules_match_full_fixture() {
    let fixture = fixture();
    assert_eq!(
        fixture.parent_expansion.default_max_child_tokens,
        PARENT_EXPANSION_MAX_CHILD_TOKENS
    );
    for example in fixture.parent_expansion.examples {
        assert_eq!(
            example.child_token_count < PARENT_EXPANSION_MAX_CHILD_TOKENS,
            example.expand
        );
    }
    assert_eq!(
        normalized_lexical_terms(&fixture.lexical.query),
        fixture.lexical.expected_terms
    );
    let fields = fixture.lexical.fields;
    assert_eq!(
        lexical_field_score(
            &fixture.lexical.query,
            &fields.title,
            &fields.heading_path,
            &fields.tags,
            &fields.topic,
            &fields.category,
            &fields.text,
            fields.token_count,
        ),
        fixture.lexical.expected_score
    );
}

#[test]
fn rust_lexical_backend_and_zero_signal_envelopes_match_full_fixture() {
    let fixture = fixture();
    let backends = fixture.lexical_backends;
    assert_eq!(
        backends.projection_schema_version,
        PROJECTION_SCHEMA_VERSION
    );
    assert_eq!(
        backends.preferred.manifest_value,
        SqliteLexicalBackend::SqliteFts5
    );
    assert_eq!(
        serde_json::to_value(&backends.preferred.stage).unwrap(),
        serde_json::json!({"kind":"sqlite_fts5","state":"active","reason_codes":[]})
    );
    assert_eq!(
        backends.compatibility.manifest_value,
        SqliteLexicalBackend::SqliteLexicalScan
    );
    assert_eq!(
        serde_json::to_value(&backends.compatibility.stage_without_fts5).unwrap(),
        serde_json::json!({
            "kind":"sqlite_lexical_scan",
            "state":"degraded",
            "reason_codes":["SQLITE_FTS5_UNAVAILABLE","SQLITE_LEXICAL_SCAN_ACTIVE","SQLITE_LEXICAL_SCAN_APPROXIMATION"]
        })
    );
    assert_eq!(
        serde_json::to_value(&backends.compatibility.stage_when_explicit_on_fts5_runtime).unwrap(),
        serde_json::json!({
            "kind":"sqlite_lexical_scan",
            "state":"degraded",
            "reason_codes":["SQLITE_LEXICAL_SCAN_ACTIVE","SQLITE_LEXICAL_SCAN_APPROXIMATION"]
        })
    );
    for query in &backends.differential_queries {
        for expected in &query.expected {
            let row = backends
                .differential_rows
                .iter()
                .find(|row| row.chunk_id == expected.chunk_id)
                .expect("expected lexical row exists");
            assert_eq!(
                lexical_field_score(
                    &query.query,
                    &row.title,
                    &row.heading_path,
                    &row.tags,
                    &row.topic,
                    &row.category,
                    &row.text,
                    row.token_count,
                ),
                expected.score,
                "{} / {}",
                query.query,
                expected.chunk_id
            );
        }
    }
    assert_eq!(backends.rejected_queries.len(), 8);

    let zero = fixture.confidence_zero_signal;
    assert_eq!(zero.scores[0].chunk_id, "zero-signal");
    let scores = zero
        .scores
        .iter()
        .map(ConfidenceScoreFixture::stage_scores)
        .collect::<Vec<_>>();
    assert_eq!(
        assess_retrieval_confidence(
            &scores,
            &zero.stages.vector,
            &zero.stages.reranker,
            zero.eligible_count,
            false,
        ),
        zero.expected
    );
}

#[test]
fn rust_overlap_result_dedup_matches_full_fixture() {
    let fixture = fixture().overlap_result_dedup;
    assert_eq!(fixture.intervals, "half-open");
    assert_eq!(fixture.rank_order, "ascending final candidate rank");
    let mut claimed = BTreeSet::new();
    let mut accepted_intervals = Vec::new();
    let mut accepted_ids = Vec::new();
    let mut claimed_in_rank_order = Vec::new();
    for candidate in fixture.candidates {
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
        let Some(evidence) = deduplicate_overlap_evidence(citation, &claimed, &accepted_intervals)
        else {
            continue;
        };
        accepted_ids.push(candidate.chunk_id);
        claimed_in_rank_order.extend(evidence.span_keys.iter().cloned());
        claimed.extend(evidence.span_keys);
        accepted_intervals.push(AcceptedCitationInterval {
            source_id: candidate.source_id,
            start_byte: candidate.start_byte,
            end_byte: candidate.end_byte,
        });
    }
    assert_eq!(accepted_ids, fixture.expected_chunk_ids);
    assert_eq!(
        claimed_in_rank_order,
        fixture
            .expected_claimed_spans
            .into_iter()
            .map(|span| (span.source_id, span.start_byte, span.end_byte))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rust_unicode_scalar_path_globs_match_full_fixture() {
    for case in fixture().path_glob_filters {
        assert_eq!(
            matches_path_glob(&case.path, &case.glob),
            case.matches,
            "{} against {}",
            case.path,
            case.glob
        );
    }
}

#[test]
fn rust_normalized_citations_match_full_exact_utf8_fixture() {
    for case in fixture().citation_normalization {
        let source = RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "full-citation-conformance".to_owned(),
            source_id: "018f0000-0000-7000-8000-000000000227".to_owned(),
            source_path: "citation.md".to_owned(),
            source_digest: sha256(case.source_text.as_bytes()),
            text: case.source_text.clone(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        };
        let chunk = chunk_source(&source, ChunkingOptions::default())
            .unwrap()
            .remove(0);
        let spans = exact_matched_spans(&case.query, &chunk, case.source_text.as_bytes()).unwrap();
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
fn rust_canonical_path_and_timestamp_rules_match_full_fixture() {
    let fixture = canonical_fixture();
    assert_eq!(fixture.contract_version, RETRIEVAL_CONTRACT);
    for case in fixture.cases {
        let parsed: serde_json::Value = serde_json::from_str(&case.canonical_json).unwrap();
        assert_eq!(
            canonical_json(&parsed).unwrap(),
            case.canonical_json,
            "{}",
            case.id
        );
        assert_eq!(
            sha256(case.canonical_json.as_bytes()),
            case.sha256,
            "{}",
            case.id
        );
    }
    for expression in fixture.rejected_numbers {
        let rejected = match expression.as_str() {
            "NaN" => canonical_json(&f64::NAN).is_err(),
            "Infinity" => canonical_json(&f64::INFINITY).is_err(),
            _ => {
                let parsed: serde_json::Value = serde_json::from_str(&expression).unwrap();
                canonical_json(&parsed).is_err()
            }
        };
        assert!(
            rejected,
            "unsafe canonical number was accepted: {expression}"
        );
    }
    for case in fixture.portable_source_paths {
        assert_eq!(is_valid_retrieval_source_path(&case.input), case.accepted);
    }
    for case in fixture.timestamp_filters {
        assert_eq!(is_valid_retrieval_timestamp(&case.input), case.accepted);
    }
}
