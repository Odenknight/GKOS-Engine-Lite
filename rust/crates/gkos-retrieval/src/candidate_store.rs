//! Immutable schema-3 store for Full-owned candidate envelopes.
//!
//! This module deliberately has no GKX parser or resolver. Opaque record keys,
//! declaration receipts, and candidate chunks arrive sealed from the trusted
//! host and are only validated, persisted, and authorization-scoped here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::candidate::{
    GkxCandidateChunk, GkxCandidateDeclaration, GkxCandidateSource, GkxCandidateVector,
};
use crate::contract::{
    GkxRetrievalProjectionManifest, RetrievalChunk, SqliteLexicalBackend, CHUNKER_VERSION,
    GKX_PROJECTION_PROFILE, GKX_STANDARD_COMMIT, LINEAGE_PROJECTION_SCHEMA_VERSION,
    RETRIEVAL_LINEAGE_CONTRACT, RETRIEVAL_PROVENANCE_CONTRACT, TOKENIZER_VERSION,
};
use crate::digest::{canonical_digest, canonical_json};
use crate::fusion::{code_unit_compare, cosine_similarity, RankedInput};
use crate::path_security::{absolute_lexical_path, contained_path, equivalent_paths};
use crate::sqlite_store::{
    assert_owner_file_permissions, ensure_fts5, fts_expression, harden_file_permissions,
    path_entry_exists, quarantine_generation_files, quarantine_orphan_sidecars, reject_file_alias,
    reject_generation_sidecars, remove_writer_generation_temporary, sync_directory,
    validate_existing_state_directory, validate_full_engine_version, validate_persisted_chunks,
    validate_state_directory, weighted_lexical_score,
};
use crate::writer_lock::{
    acquire_legacy_retrieval_writer, assert_legacy_writer_capability, assert_legacy_writer_commit,
    assert_legacy_writer_directory_permissions, assert_no_phase3_authority,
    bind_legacy_writer_target, finish_with_writer, remove_uncommitted_writer_temporary,
    verify_legacy_writer_target_published, LegacyRetrievalWriterCapability,
};
use crate::{RetrievalError, RetrievalResult};

const ACTIVE_POINTER_NAME: &str = "active-retrieval.json";

const MIGRATION_V3: &str = r#"
CREATE TABLE projection_manifest (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  manifest_json TEXT NOT NULL,
  contract_version TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  projection_id TEXT NOT NULL UNIQUE,
  projection_digest TEXT NOT NULL UNIQUE,
  lexical_backend TEXT NOT NULL
);
CREATE TABLE candidate_sources (
  record_key TEXT PRIMARY KEY,
  source_id TEXT NOT NULL,
  source_path TEXT NOT NULL,
  source_digest TEXT NOT NULL,
  candidate_json TEXT NOT NULL,
  candidate_digest TEXT NOT NULL UNIQUE
);
CREATE TABLE candidate_declarations (
  declaration_digest TEXT PRIMARY KEY,
  source_record_key TEXT NOT NULL REFERENCES candidate_sources(record_key) ON DELETE CASCADE,
  declaration_json TEXT NOT NULL
);
CREATE TABLE candidate_chunks (
  candidate_chunk_key TEXT PRIMARY KEY,
  record_key TEXT NOT NULL REFERENCES candidate_sources(record_key) ON DELETE CASCADE,
  public_chunk_id TEXT NOT NULL,
  parent_candidate_chunk_key TEXT REFERENCES candidate_chunks(candidate_chunk_key) DEFERRABLE INITIALLY DEFERRED,
  chunk_json TEXT NOT NULL,
  chunk_digest TEXT NOT NULL,
  UNIQUE(record_key, public_chunk_id)
);
CREATE VIRTUAL TABLE chunk_fts USING fts5(
  chunk_id UNINDEXED,
  title,
  heading_path,
  tags,
  topic,
  category,
  text,
  tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TABLE candidate_chunk_vectors (
  candidate_chunk_key TEXT PRIMARY KEY REFERENCES candidate_chunks(candidate_chunk_key) ON DELETE CASCADE,
  provider_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  dimensions INTEGER NOT NULL,
  vector_json TEXT NOT NULL
);
CREATE TABLE embedding_eligible_candidate_chunks (
  candidate_chunk_key TEXT PRIMARY KEY REFERENCES candidate_chunks(candidate_chunk_key) ON DELETE CASCADE
);
CREATE INDEX candidate_sources_uid_idx ON candidate_sources(source_id);
CREATE INDEX candidate_sources_path_idx ON candidate_sources(source_path);
CREATE INDEX candidate_chunks_record_idx ON candidate_chunks(record_key);
CREATE INDEX candidate_chunks_public_idx ON candidate_chunks(public_chunk_id);
CREATE INDEX candidate_chunks_parent_idx ON candidate_chunks(parent_candidate_chunk_key);
"#;

#[derive(Clone)]
pub(crate) struct GkxRetrievalGenerationInput {
    pub(crate) state_directory: PathBuf,
    pub(crate) engine_version: String,
    pub(crate) vault_id: String,
    pub(crate) source_snapshot_digest: String,
    pub(crate) configuration_digest: String,
    pub(crate) policy_digest: String,
    pub(crate) candidate_sources: Vec<GkxCandidateSource>,
    pub(crate) candidate_declarations: Vec<GkxCandidateDeclaration>,
    pub(crate) candidate_chunks: Vec<GkxCandidateChunk>,
    pub(crate) embedding_eligible_candidate_chunk_keys: Vec<String>,
    pub(crate) vectors: Vec<GkxCandidateVector>,
    pub(crate) embedding_provider_id: Option<String>,
    pub(crate) embedding_model_id: Option<String>,
    pub(crate) embedding_dimensions: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BuiltGkxRetrievalGeneration {
    database_path: PathBuf,
    manifest: GkxRetrievalProjectionManifest,
}

impl BuiltGkxRetrievalGeneration {
    pub(crate) fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub(crate) fn manifest(&self) -> &GkxRetrievalProjectionManifest {
        &self.manifest
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ActivePointer {
    database_file: String,
    manifest: GkxRetrievalProjectionManifest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoricalLiteActivePointer {
    contract_version: String,
    database_file: String,
    manifest: GkxRetrievalProjectionManifest,
}

#[derive(Serialize)]
struct ProjectionDigestEnvelope<'a> {
    contract_version: &'a str,
    projection_schema_version: u32,
    provenance_contract_version: &'a str,
    gkx_standard_commit: &'a str,
    gkx_projection_profile: &'a str,
    engine_version: &'a str,
    vault_id: &'a str,
    source_snapshot_digest: &'a str,
    configuration_digest: &'a str,
    policy_digest: &'a str,
    chunker_version: &'a str,
    tokenizer_version: &'a str,
    lexical_backend: SqliteLexicalBackend,
    embedding_provider_id: &'a Option<String>,
    embedding_model_id: &'a Option<String>,
    embedding_dimensions: Option<u32>,
    candidate_source_count: u32,
    candidate_declaration_count: u32,
    represented_candidate_source_count: u32,
    candidate_chunk_count: u32,
    embedding_eligible_candidate_chunk_count: u32,
    candidate_sources: &'a [GkxCandidateSource],
    candidate_declarations: &'a [GkxCandidateDeclaration],
    embedding_eligible_candidate_chunk_keys: &'a [String],
    candidate_chunks: &'a [GkxCandidateChunk],
    vectors: &'a [GkxCandidateVector],
}

struct PreparedGeneration {
    sources: Vec<GkxCandidateSource>,
    declarations: Vec<GkxCandidateDeclaration>,
    chunks: Vec<GkxCandidateChunk>,
    eligible: Vec<String>,
    vectors: Vec<GkxCandidateVector>,
    manifest: GkxRetrievalProjectionManifest,
}

pub(crate) fn prepare_chunks_for_embedding(
    input: &GkxRetrievalGenerationInput,
) -> RetrievalResult<Vec<GkxCandidateChunk>> {
    Ok(prepare_generation(input)?.chunks)
}

fn prepare_generation(input: &GkxRetrievalGenerationInput) -> RetrievalResult<PreparedGeneration> {
    validate_full_engine_version(&input.engine_version)?;
    if input.vault_id.trim().is_empty() {
        return Err(RetrievalError::InvalidConfig(
            "vault_id must not be empty".to_owned(),
        ));
    }
    for digest in [
        &input.source_snapshot_digest,
        &input.configuration_digest,
        &input.policy_digest,
    ] {
        if !crate::contract::is_sha256_digest(digest) {
            return Err(RetrievalError::InvalidConfig(
                "schema-3 input digest is invalid".to_owned(),
            ));
        }
    }

    let mut sources = input.candidate_sources.clone();
    sources.sort_by(|a, b| code_unit_compare(&a.record_key, &b.record_key));
    let mut source_by_key = BTreeMap::new();
    for source in &sources {
        source.validate()?;
        if source_by_key
            .insert(source.record_key.as_str(), source)
            .is_some()
        {
            return Err(mismatch("DUPLICATE_CANDIDATE_SOURCE_KEY"));
        }
    }

    let mut declarations = input.candidate_declarations.clone();
    declarations.sort_by(|a, b| {
        let left = a.digest().unwrap_or_default();
        let right = b.digest().unwrap_or_default();
        code_unit_compare(&left, &right)
    });
    let mut declaration_digests = BTreeSet::new();
    let mut coordinates = BTreeSet::new();
    let mut sequences = BTreeMap::<String, Vec<u64>>::new();
    for declaration in &declarations {
        declaration.validate()?;
        if !source_by_key.contains_key(declaration.source_record_key.as_str()) {
            return Err(mismatch("ORPHAN_CANDIDATE_DECLARATION_SOURCE"));
        }
        let digest = declaration.digest()?;
        if !declaration_digests.insert(digest) {
            return Err(mismatch("DUPLICATE_CANDIDATE_DECLARATION"));
        }
        let coordinate = format!(
            "{}\0{:?}\0{}\0{}",
            declaration.source_record_key,
            declaration.category,
            declaration.field,
            declaration.declaration_index
        );
        if !coordinates.insert(coordinate) {
            return Err(mismatch("DUPLICATE_CANDIDATE_DECLARATION_COORDINATE"));
        }
        let sequence = match declaration.category {
            crate::candidate::GkxCandidateCategory::Lineage => format!(
                "{}\0lineage\0{}",
                declaration.source_record_key, declaration.field
            ),
            crate::candidate::GkxCandidateCategory::Relationship => {
                format!("{}\0relationship\0*", declaration.source_record_key)
            }
            crate::candidate::GkxCandidateCategory::Link => {
                format!("{}\0link\0*", declaration.source_record_key)
            }
        };
        sequences
            .entry(sequence)
            .or_default()
            .push(declaration.declaration_index);
    }
    for indices in sequences.values_mut() {
        indices.sort_unstable();
        if indices
            .iter()
            .enumerate()
            .any(|(index, value)| *value != index as u64)
        {
            return Err(mismatch("CANDIDATE_DECLARATION_INDEX_SEQUENCE_INVALID"));
        }
    }

    let mut chunks = input.candidate_chunks.clone();
    chunks.sort_by(|a, b| code_unit_compare(&a.candidate_chunk_key, &b.candidate_chunk_key));
    let mut chunk_by_key = BTreeMap::new();
    let mut public_by_record = BTreeSet::new();
    let mut represented = BTreeSet::new();
    for candidate in &chunks {
        candidate.validate()?;
        let source = source_by_key
            .get(candidate.record_key.as_str())
            .ok_or_else(|| mismatch("ORPHAN_CANDIDATE_CHUNK_SOURCE"))?;
        if candidate.chunk.source_id != source.source_id
            || candidate.chunk.source_path != source.source_path
            || candidate.chunk.source_digest != source.source_digest
            || candidate.chunk.metadata != source.source_metadata
            || candidate.chunk.valid_from != source.valid_from
        {
            return Err(mismatch("CANDIDATE_SOURCE_CHUNK_BINDING_MISMATCH"));
        }
        if chunk_by_key
            .insert(candidate.candidate_chunk_key.as_str(), candidate)
            .is_some()
        {
            return Err(mismatch("DUPLICATE_CANDIDATE_CHUNK_KEY"));
        }
        if !public_by_record.insert((
            candidate.record_key.as_str(),
            candidate.chunk.chunk_id.as_str(),
        )) {
            return Err(mismatch("DUPLICATE_CANDIDATE_PUBLIC_CHUNK_ID"));
        }
        represented.insert(candidate.record_key.as_str());
    }
    for candidate in &chunks {
        match candidate.parent_candidate_chunk_key.as_deref() {
            None if candidate.chunk.parent_chunk_id.is_none() => {}
            Some(parent_key) => {
                let parent = chunk_by_key
                    .get(parent_key)
                    .ok_or_else(|| mismatch("CANDIDATE_PARENT_BINDING_MISMATCH"))?;
                if parent.record_key != candidate.record_key
                    || candidate.chunk.parent_chunk_id.as_deref()
                        != Some(parent.chunk.chunk_id.as_str())
                {
                    return Err(mismatch("CANDIDATE_PARENT_BINDING_MISMATCH"));
                }
            }
            _ => return Err(mismatch("CANDIDATE_PARENT_BINDING_MISMATCH")),
        }
    }
    for record_key in &represented {
        let source_chunks = chunks
            .iter()
            .filter(|item| item.record_key.as_str() == *record_key)
            .map(|item| item.chunk.clone())
            .collect::<Vec<_>>();
        validate_persisted_chunks(&source_chunks)?;
    }

    let mut eligible = input.embedding_eligible_candidate_chunk_keys.clone();
    eligible.sort_by(|a, b| code_unit_compare(a, b));
    if eligible.windows(2).any(|pair| pair[0] == pair[1])
        || eligible
            .iter()
            .any(|key| !chunk_by_key.contains_key(key.as_str()))
    {
        return Err(mismatch("VECTOR_ELIGIBILITY_INVALID"));
    }
    let mut vectors = input.vectors.clone();
    vectors.sort_by(|a, b| code_unit_compare(&a.candidate_chunk_key, &b.candidate_chunk_key));
    validate_vectors(input, &eligible, &vectors, &chunk_by_key)?;

    let base = GkxRetrievalProjectionManifest {
        contract_version: RETRIEVAL_LINEAGE_CONTRACT.to_owned(),
        projection_schema_version: LINEAGE_PROJECTION_SCHEMA_VERSION,
        provenance_contract_version: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
        gkx_standard_commit: GKX_STANDARD_COMMIT.to_owned(),
        gkx_projection_profile: GKX_PROJECTION_PROFILE.to_owned(),
        projection_id: String::new(),
        engine_version: input.engine_version.clone(),
        vault_id: input.vault_id.clone(),
        source_snapshot_digest: input.source_snapshot_digest.clone(),
        configuration_digest: input.configuration_digest.clone(),
        policy_digest: input.policy_digest.clone(),
        chunker_version: CHUNKER_VERSION.to_owned(),
        tokenizer_version: TOKENIZER_VERSION.to_owned(),
        lexical_backend: SqliteLexicalBackend::SqliteFts5,
        embedding_provider_id: input.embedding_provider_id.clone(),
        embedding_model_id: input.embedding_model_id.clone(),
        embedding_dimensions: input.embedding_dimensions,
        candidate_source_count: count(sources.len(), "candidate sources")?,
        candidate_declaration_count: count(declarations.len(), "candidate declarations")?,
        represented_candidate_source_count: count(represented.len(), "represented sources")?,
        candidate_chunk_count: count(chunks.len(), "candidate chunks")?,
        embedding_eligible_candidate_chunk_count: count(eligible.len(), "eligible chunks")?,
        projection_digest: String::new(),
    };
    let digest = projection_digest(&base, &sources, &declarations, &chunks, &eligible, &vectors)?;
    let mut manifest = base;
    manifest.projection_id = format!("retrieval:{}", &digest[7..31]);
    manifest.projection_digest = digest;
    manifest.validate()?;
    Ok(PreparedGeneration {
        sources,
        declarations,
        chunks,
        eligible,
        vectors,
        manifest,
    })
}

fn count(value: usize, label: &str) -> RetrievalResult<u32> {
    u32::try_from(value).map_err(|_| RetrievalError::InvalidConfig(format!("too many {label}")))
}

fn validate_vectors(
    input: &GkxRetrievalGenerationInput,
    eligible: &[String],
    vectors: &[GkxCandidateVector],
    chunks: &BTreeMap<&str, &GkxCandidateChunk>,
) -> RetrievalResult<()> {
    let dimensions = match (
        input.embedding_provider_id.as_deref(),
        input.embedding_model_id.as_deref(),
        input.embedding_dimensions,
    ) {
        (None, None, None) if vectors.is_empty() => return Ok(()),
        (Some(provider), Some(model), Some(dimensions))
            if !provider.is_empty() && !model.is_empty() && dimensions > 0 =>
        {
            dimensions
        }
        _ => return Err(mismatch("VECTOR_MANIFEST_IDENTITY_INVALID")),
    };
    if vectors.len() != eligible.len() {
        return Err(mismatch("VECTOR_GENERATION_PARTIAL"));
    }
    let mut by_content = BTreeMap::<&str, &Vec<f64>>::new();
    for (expected, vector) in eligible.iter().zip(vectors) {
        vector.validate()?;
        if &vector.candidate_chunk_key != expected || vector.vector.len() != dimensions as usize {
            return Err(mismatch("VECTOR_GENERATION_PARTIAL"));
        }
        let content = chunks
            .get(expected.as_str())
            .ok_or_else(|| mismatch("VECTOR_GENERATION_PARTIAL"))?
            .chunk
            .content_digest
            .as_str();
        if by_content
            .insert(content, &vector.vector)
            .is_some_and(|prior| prior != &vector.vector)
        {
            return Err(mismatch("CONTENT_VECTOR_CACHE_CONFLICT"));
        }
    }
    Ok(())
}

fn projection_digest(
    manifest: &GkxRetrievalProjectionManifest,
    sources: &[GkxCandidateSource],
    declarations: &[GkxCandidateDeclaration],
    chunks: &[GkxCandidateChunk],
    eligible: &[String],
    vectors: &[GkxCandidateVector],
) -> RetrievalResult<String> {
    canonical_digest(&ProjectionDigestEnvelope {
        contract_version: &manifest.contract_version,
        projection_schema_version: manifest.projection_schema_version,
        provenance_contract_version: &manifest.provenance_contract_version,
        gkx_standard_commit: &manifest.gkx_standard_commit,
        gkx_projection_profile: &manifest.gkx_projection_profile,
        engine_version: &manifest.engine_version,
        vault_id: &manifest.vault_id,
        source_snapshot_digest: &manifest.source_snapshot_digest,
        configuration_digest: &manifest.configuration_digest,
        policy_digest: &manifest.policy_digest,
        chunker_version: &manifest.chunker_version,
        tokenizer_version: &manifest.tokenizer_version,
        lexical_backend: manifest.lexical_backend,
        embedding_provider_id: &manifest.embedding_provider_id,
        embedding_model_id: &manifest.embedding_model_id,
        embedding_dimensions: manifest.embedding_dimensions,
        candidate_source_count: manifest.candidate_source_count,
        candidate_declaration_count: manifest.candidate_declaration_count,
        represented_candidate_source_count: manifest.represented_candidate_source_count,
        candidate_chunk_count: manifest.candidate_chunk_count,
        embedding_eligible_candidate_chunk_count: manifest.embedding_eligible_candidate_chunk_count,
        candidate_sources: sources,
        candidate_declarations: declarations,
        embedding_eligible_candidate_chunk_keys: eligible,
        candidate_chunks: chunks,
        vectors,
    })
}

pub(crate) fn build_gkx_retrieval_generation(
    input: GkxRetrievalGenerationInput,
) -> RetrievalResult<BuiltGkxRetrievalGeneration> {
    let prepared = prepare_generation(&input)?;
    let mut writer = acquire_legacy_retrieval_writer(&input.state_directory)?;
    let result = build_prepared_gkx_retrieval_generation(input, prepared, &writer);
    finish_with_writer(result, &mut writer)
}

pub(crate) fn build_gkx_retrieval_generation_with_writer(
    input: GkxRetrievalGenerationInput,
    writer: &LegacyRetrievalWriterCapability,
) -> RetrievalResult<BuiltGkxRetrievalGeneration> {
    let prepared = prepare_generation(&input)?;
    build_prepared_gkx_retrieval_generation(input, prepared, writer)
}

fn build_prepared_gkx_retrieval_generation(
    input: GkxRetrievalGenerationInput,
    prepared: PreparedGeneration,
    writer: &LegacyRetrievalWriterCapability,
) -> RetrievalResult<BuiltGkxRetrievalGeneration> {
    assert_legacy_writer_capability(writer, &input.state_directory)?;
    assert_no_phase3_authority(writer.state_directory())?;
    let state_directory = validate_existing_state_directory(writer.state_directory())?;
    assert_legacy_writer_directory_permissions(&state_directory)?;
    let suffix = prepared
        .manifest
        .projection_digest
        .trim_start_matches("sha256:");
    let final_path = state_directory.join(format!("retrieval-{suffix}.sqlite"));
    if path_entry_exists(&final_path)? {
        assert_owner_file_permissions(&final_path, "schema-3 retrieval generation")?;
        match GkxSqliteRetrievalStore::open(&final_path) {
            Ok(store) if store.manifest == prepared.manifest => {
                drop(store);
                return Ok(BuiltGkxRetrievalGeneration {
                    database_path: final_path,
                    manifest: prepared.manifest,
                });
            }
            _ => quarantine_generation_files(&final_path)?,
        }
    } else {
        quarantine_orphan_sidecars(&final_path)?;
    }
    let temporary = state_directory.join(format!(
        "retrieval-{suffix}.sqlite.{}.tmp",
        std::process::id()
    ));
    remove_writer_generation_temporary(&temporary)?;
    if let Err(error) = insert_generation(&temporary, &prepared) {
        let _ = remove_writer_generation_temporary(&temporary);
        return Err(error);
    }
    harden_file_permissions(&temporary)?;
    let verified = GkxSqliteRetrievalStore::open(&temporary)?;
    if verified.manifest != prepared.manifest {
        drop(verified);
        let _ = remove_writer_generation_temporary(&temporary);
        return Err(mismatch("new schema-3 generation did not verify"));
    }
    drop(verified);
    assert_legacy_writer_capability(writer, &state_directory)?;
    assert_no_phase3_authority(&state_directory)?;
    fs::rename(&temporary, &final_path)?;
    harden_file_permissions(&final_path)?;
    sync_directory(&state_directory)?;
    let final_verified = GkxSqliteRetrievalStore::open(&final_path)?;
    if final_verified.manifest != prepared.manifest {
        drop(final_verified);
        quarantine_generation_files(&final_path)?;
        return Err(mismatch("final schema-3 generation did not verify"));
    }
    drop(final_verified);
    Ok(BuiltGkxRetrievalGeneration {
        database_path: final_path,
        manifest: prepared.manifest,
    })
}

pub(crate) fn activate_gkx_retrieval_generation(
    state_directory: &Path,
    generation: &BuiltGkxRetrievalGeneration,
) -> RetrievalResult<PathBuf> {
    validate_full_engine_version(&generation.manifest.engine_version)?;
    let mut writer = acquire_legacy_retrieval_writer(state_directory)?;
    let result = activate_gkx_retrieval_generation_with_writer(
        state_directory,
        generation,
        &mut writer,
        || {},
    );
    finish_with_writer(result, &mut writer)
}

fn activate_gkx_retrieval_generation_with_writer<F>(
    state_directory: &Path,
    generation: &BuiltGkxRetrievalGeneration,
    writer: &mut LegacyRetrievalWriterCapability,
    before_pointer_write: F,
) -> RetrievalResult<PathBuf>
where
    F: FnOnce(),
{
    assert_legacy_writer_capability(writer, state_directory)?;
    assert_no_phase3_authority(writer.state_directory())?;
    let state_directory = validate_existing_state_directory(writer.state_directory())?;
    let lexical_database = absolute_lexical_path(&generation.database_path)?;
    reject_file_alias(&lexical_database, "schema-3 retrieval generation")?;
    let canonical_database = fs::canonicalize(&lexical_database)?;
    if !canonical_database
        .parent()
        .is_some_and(|parent| equivalent_paths(parent, &state_directory))
        || !contained_path(&canonical_database, &state_directory)
    {
        return Err(mismatch("generation is outside its state directory"));
    }
    reject_file_alias(&canonical_database, "schema-3 retrieval generation")?;
    assert_owner_file_permissions(&canonical_database, "schema-3 retrieval generation")?;
    let verified = GkxSqliteRetrievalStore::open(&canonical_database)?;
    if verified.manifest != generation.manifest {
        return Err(mismatch(
            "activation manifest differs from verified generation",
        ));
    }
    drop(verified);
    let database_file = canonical_database
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| mismatch("generation filename is invalid"))?
        .to_owned();
    let pointer = ActivePointer {
        database_file,
        manifest: generation.manifest.clone(),
    };
    let bytes = format!("{}\n", canonical_json(&pointer)?).into_bytes();
    bind_legacy_writer_target(writer, &bytes)?;
    before_pointer_write();
    assert_legacy_writer_commit(writer, &bytes)?;
    let pointer_path = state_directory.join(ACTIVE_POINTER_NAME);
    let temporary =
        state_directory.join(format!("{ACTIVE_POINTER_NAME}.{}.tmp", std::process::id()));
    if path_entry_exists(&temporary)? {
        return Err(mismatch("RETRIEVAL_STATE_WRITER_POINTER_TEMP_CONFLICT"));
    }
    if let Err(error) = crate::sqlite_store::write_owner_file(&temporary, &bytes) {
        let _ = remove_uncommitted_writer_temporary(&temporary, &state_directory);
        return Err(error);
    }
    if path_entry_exists(&pointer_path)? {
        reject_file_alias(&pointer_path, "active schema-3 retrieval pointer")?;
    }
    if let Err(error) = assert_legacy_writer_commit(writer, &bytes) {
        if fs::read(&temporary).ok().as_deref() == Some(bytes.as_slice()) {
            let _ = fs::remove_file(&temporary);
            let _ = sync_directory(&state_directory);
        }
        return Err(error);
    }
    crate::sqlite_store::atomic_replace(&temporary, &pointer_path)?;
    harden_file_permissions(&pointer_path)?;
    sync_directory(&state_directory)?;
    verify_legacy_writer_target_published(writer, &bytes)?;
    let reopened = open_active_gkx_retrieval_generation(&state_directory)?;
    if reopened.manifest != generation.manifest {
        return Err(mismatch("activated schema-3 pointer did not reopen"));
    }
    Ok(pointer_path)
}

pub(crate) fn open_active_gkx_retrieval_generation(
    state_directory: &Path,
) -> RetrievalResult<GkxSqliteRetrievalStore> {
    let state_directory = validate_existing_state_directory(state_directory)?;
    let pointer_path = state_directory.join(ACTIVE_POINTER_NAME);
    reject_file_alias(&pointer_path, "active retrieval pointer")?;
    if fs::metadata(&pointer_path)?.len() > 1_048_576 {
        return Err(mismatch("active retrieval pointer exceeds 1 MiB"));
    }
    let bytes = fs::read(&pointer_path)?;
    let pointer = parse_active_pointer(&bytes)?;
    pointer.manifest.validate()?;
    if pointer.database_file.contains('/')
        || pointer.database_file.contains('\\')
        || Path::new(&pointer.database_file)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(pointer.database_file.as_str())
    {
        return Err(mismatch("active schema-3 pointer is invalid"));
    }
    let expected_file = format!(
        "retrieval-{}.sqlite",
        pointer
            .manifest
            .projection_digest
            .trim_start_matches("sha256:")
    );
    if pointer.database_file != expected_file {
        return Err(mismatch("active schema-3 filename is not digest-bound"));
    }
    let database_path = state_directory.join(&pointer.database_file);
    reject_file_alias(&database_path, "active schema-3 retrieval generation")?;
    let store = GkxSqliteRetrievalStore::open(&database_path)?;
    if store.manifest != pointer.manifest {
        return Err(mismatch("active schema-3 pointer manifest mismatch"));
    }
    Ok(store)
}

fn parse_active_pointer(bytes: &[u8]) -> RetrievalResult<ActivePointer> {
    if let Ok(pointer) = serde_json::from_slice::<ActivePointer>(bytes) {
        if bytes == format!("{}\n", canonical_json(&pointer)?).as_bytes() {
            return Ok(pointer);
        }
    }
    let historical: HistoricalLiteActivePointer = serde_json::from_slice(bytes)?;
    if historical.contract_version != historical.manifest.contract_version
        || historical.contract_version != RETRIEVAL_LINEAGE_CONTRACT
        || bytes != format!("{}\n", canonical_json(&historical)?).as_bytes()
    {
        return Err(mismatch("active schema-3 pointer is invalid"));
    }
    Ok(ActivePointer {
        database_file: historical.database_file,
        manifest: historical.manifest,
    })
}

pub(crate) fn try_open_active_gkx_retrieval_generation(
    state_directory: &Path,
) -> RetrievalResult<Option<GkxSqliteRetrievalStore>> {
    let state_directory = validate_state_directory(state_directory)?;
    let pointer_path = state_directory.join(ACTIVE_POINTER_NAME);
    if !path_entry_exists(&pointer_path)? {
        return Ok(None);
    }
    open_active_gkx_retrieval_generation(&state_directory).map(Some)
}

fn mismatch(message: &str) -> RetrievalError {
    RetrievalError::ProjectionMismatch(message.to_owned())
}

fn insert_generation(path: &Path, prepared: &PreparedGeneration) -> RetrievalResult<()> {
    let mut database = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    database.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    ensure_fts5(&database)?;
    let tx = database.transaction()?;
    tx.execute_batch(MIGRATION_V3)?;
    tx.pragma_update(None, "user_version", LINEAGE_PROJECTION_SCHEMA_VERSION)?;
    tx.execute(
        "INSERT INTO projection_manifest VALUES (1,?1,?2,?3,?4,?5,?6)",
        params![
            canonical_json(&prepared.manifest)?,
            prepared.manifest.contract_version,
            prepared.manifest.projection_schema_version,
            prepared.manifest.projection_id,
            prepared.manifest.projection_digest,
            prepared.manifest.lexical_backend.as_str(),
        ],
    )?;
    for source in &prepared.sources {
        tx.execute(
            "INSERT INTO candidate_sources VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                source.record_key,
                source.source_id,
                source.source_path,
                source.source_digest,
                canonical_json(source)?,
                source.candidate_digest,
            ],
        )?;
    }
    for declaration in &prepared.declarations {
        tx.execute(
            "INSERT INTO candidate_declarations VALUES (?1,?2,?3)",
            params![
                declaration.digest()?,
                declaration.source_record_key,
                canonical_json(declaration)?,
            ],
        )?;
    }
    for candidate in &prepared.chunks {
        tx.execute(
            "INSERT INTO candidate_chunks VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                candidate.candidate_chunk_key,
                candidate.record_key,
                candidate.chunk.chunk_id,
                candidate.parent_candidate_chunk_key,
                canonical_json(&candidate.chunk)?,
                candidate.digest()?,
            ],
        )?;
        let chunk = &candidate.chunk;
        tx.execute(
            "INSERT INTO chunk_fts VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                candidate.candidate_chunk_key,
                chunk.metadata.title.as_deref().unwrap_or(""),
                chunk.heading_path.join(" / "),
                chunk
                    .metadata
                    .tags
                    .as_ref()
                    .map_or_else(String::new, |values| values.join(" ")),
                chunk.metadata.topic.as_deref().unwrap_or(""),
                chunk.metadata.category.as_deref().unwrap_or(""),
                chunk.text,
            ],
        )?;
    }
    let vectors = prepared
        .vectors
        .iter()
        .map(|item| (item.candidate_chunk_key.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for key in &prepared.eligible {
        tx.execute(
            "INSERT INTO embedding_eligible_candidate_chunks VALUES (?1)",
            [key],
        )?;
        if let Some(vector) = vectors.get(key.as_str()) {
            tx.execute(
                "INSERT INTO candidate_chunk_vectors VALUES (?1,?2,?3,?4,?5)",
                params![
                    key,
                    prepared.manifest.embedding_provider_id,
                    prepared.manifest.embedding_model_id,
                    prepared.manifest.embedding_dimensions,
                    canonical_json(&vector.vector)?,
                ],
            )?;
        }
    }
    tx.commit()?;
    database.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
    Ok(())
}

pub(crate) struct GkxSqliteRetrievalStore {
    database: Connection,
    pub(crate) manifest: GkxRetrievalProjectionManifest,
}

impl GkxSqliteRetrievalStore {
    pub(crate) fn open(path: &Path) -> RetrievalResult<Self> {
        reject_file_alias(path, "schema-3 retrieval generation")?;
        reject_generation_sidecars(path)?;
        let database = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        database.execute_batch("PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;")?;
        ensure_fts5(&database)?;
        let version: u32 = database.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != LINEAGE_PROJECTION_SCHEMA_VERSION {
            return Err(mismatch("schema-3 SQLite version is incompatible"));
        }
        let integrity: String =
            database.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(mismatch("schema-3 SQLite integrity check failed"));
        }
        let failures: i64 =
            database.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if failures != 0 {
            return Err(mismatch("schema-3 foreign key check failed"));
        }
        let (json, contract, schema, id, digest, backend): (
            String,
            String,
            u32,
            String,
            String,
            String,
        ) = database.query_row(
            "SELECT manifest_json,contract_version,schema_version,projection_id,projection_digest,lexical_backend FROM projection_manifest WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
        )?;
        let manifest: GkxRetrievalProjectionManifest = serde_json::from_str(&json)?;
        manifest.validate()?;
        if manifest.lexical_backend != SqliteLexicalBackend::SqliteFts5
            || contract != manifest.contract_version
            || schema != manifest.projection_schema_version
            || id != manifest.projection_id
            || digest != manifest.projection_digest
            || backend != manifest.lexical_backend.as_str()
            || json != canonical_json(&manifest)?
        {
            return Err(mismatch(
                "schema-3 manifest columns or canonical bytes disagree",
            ));
        }
        let store = Self { database, manifest };
        store.verify_schema_shape()?;
        store.verify()?;
        Ok(store)
    }

    pub(crate) fn list_candidate_sources(&self) -> RetrievalResult<Vec<GkxCandidateSource>> {
        let mut statement = self.database.prepare(
            "SELECT record_key,source_id,source_path,source_digest,candidate_json,candidate_digest FROM candidate_sources ORDER BY record_key",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (record_key, source_id, source_path, source_digest, json, digest) = row?;
            let source: GkxCandidateSource = serde_json::from_str(&json)?;
            source.validate()?;
            if record_key != source.record_key
                || source_id != source.source_id
                || source_path != source.source_path
                || source_digest != source.source_digest
                || digest != source.candidate_digest
                || json != canonical_json(&source)?
            {
                return Err(mismatch("candidate source columns or bytes disagree"));
            }
            Ok(source)
        })
        .collect()
    }

    pub(crate) fn list_candidate_declarations(
        &self,
    ) -> RetrievalResult<Vec<GkxCandidateDeclaration>> {
        self.list_candidate_declarations_sql(
            "SELECT declaration_digest,source_record_key,declaration_json FROM candidate_declarations ORDER BY declaration_digest",
        )
    }

    pub(crate) fn list_candidate_declarations_for_record_keys(
        &self,
        record_keys: &[String],
    ) -> RetrievalResult<Vec<GkxCandidateDeclaration>> {
        if record_keys.is_empty() {
            return Ok(vec![]);
        }
        self.replace_records(record_keys.iter().map(String::as_str))?;
        self.list_candidate_declarations_sql(
            "SELECT d.declaration_digest,d.source_record_key,d.declaration_json FROM candidate_declarations AS d JOIN retrieval_candidate_records AS e ON e.record_key=d.source_record_key ORDER BY d.declaration_digest",
        )
    }

    fn list_candidate_declarations_sql(
        &self,
        sql: &str,
    ) -> RetrievalResult<Vec<GkxCandidateDeclaration>> {
        let mut statement = self.database.prepare(sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (digest, source_record_key, json) = row?;
            let declaration: GkxCandidateDeclaration = serde_json::from_str(&json)?;
            declaration.validate()?;
            if digest != declaration.digest()?
                || source_record_key != declaration.source_record_key
                || json != canonical_json(&declaration)?
            {
                return Err(mismatch("candidate declaration columns or bytes disagree"));
            }
            Ok(declaration)
        })
        .collect()
    }

    pub(crate) fn list_candidate_chunks(&self) -> RetrievalResult<Vec<GkxCandidateChunk>> {
        self.list_candidate_chunks_sql(
            "SELECT candidate_chunk_key,record_key,public_chunk_id,parent_candidate_chunk_key,chunk_json,chunk_digest FROM candidate_chunks ORDER BY candidate_chunk_key",
        )
    }

    pub(crate) fn list_candidate_chunks_for_record_keys(
        &self,
        record_keys: &[String],
    ) -> RetrievalResult<Vec<GkxCandidateChunk>> {
        if record_keys.is_empty() {
            return Ok(vec![]);
        }
        self.replace_records(record_keys.iter().map(String::as_str))?;
        self.list_candidate_chunks_sql(
            "SELECT c.candidate_chunk_key,c.record_key,c.public_chunk_id,c.parent_candidate_chunk_key,c.chunk_json,c.chunk_digest FROM candidate_chunks AS c JOIN retrieval_candidate_records AS e ON e.record_key=c.record_key ORDER BY c.candidate_chunk_key",
        )
    }

    pub(crate) fn list_candidate_chunks_for_keys(
        &self,
        keys: &[String],
    ) -> RetrievalResult<Vec<GkxCandidateChunk>> {
        if keys.is_empty() {
            return Ok(vec![]);
        }
        self.replace_eligible(keys.iter().map(String::as_str))?;
        self.list_candidate_chunks_sql(
            "SELECT c.candidate_chunk_key,c.record_key,c.public_chunk_id,c.parent_candidate_chunk_key,c.chunk_json,c.chunk_digest FROM candidate_chunks AS c JOIN retrieval_eligible AS e ON e.chunk_id=c.candidate_chunk_key ORDER BY c.candidate_chunk_key",
        )
    }

    fn list_candidate_chunks_sql(&self, sql: &str) -> RetrievalResult<Vec<GkxCandidateChunk>> {
        let mut statement = self.database.prepare(sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (key, record_key, public_id, parent, json, digest) = row?;
            let chunk: RetrievalChunk = serde_json::from_str(&json)?;
            let candidate = GkxCandidateChunk {
                candidate_chunk_key: key.clone(),
                record_key: record_key.clone(),
                parent_candidate_chunk_key: parent.clone(),
                chunk,
            };
            candidate.validate()?;
            if public_id != candidate.chunk.chunk_id
                || digest != candidate.digest()?
                || json != canonical_json(&candidate.chunk)?
            {
                return Err(mismatch("candidate chunk columns or bytes disagree"));
            }
            Ok(candidate)
        })
        .collect()
    }

    pub(crate) fn candidate_vector_eligibility_covers(
        &self,
        keys: &BTreeSet<String>,
    ) -> RetrievalResult<()> {
        if self.manifest.embedding_provider_id.is_none() {
            return Ok(());
        }
        let mut contains = self.database.prepare(
            "SELECT 1 FROM embedding_eligible_candidate_chunks WHERE candidate_chunk_key=?1",
        )?;
        for key in keys {
            if !contains.exists([key])? {
                return Err(mismatch("RETRIEVAL_RUNTIME_VECTOR_ELIGIBILITY_MISMATCH"));
            }
        }
        Ok(())
    }

    pub(crate) fn lexical_search_eligible(
        &self,
        query: &str,
        candidate_keys: &[String],
        limit: usize,
    ) -> RetrievalResult<Vec<RankedInput>> {
        if candidate_keys.is_empty() {
            return Ok(vec![]);
        }
        self.replace_eligible(candidate_keys.iter().map(String::as_str))?;
        let mut statement = self.database.prepare(
            "SELECT c.chunk_json,f.title,f.heading_path,f.tags,f.topic,f.category FROM chunk_fts AS f JOIN retrieval_eligible AS e ON e.chunk_id=f.chunk_id JOIN candidate_chunks AS c ON c.candidate_chunk_key=f.chunk_id WHERE chunk_fts MATCH ?1",
        )?;
        let mut rows = statement.query([fts_expression(query)?])?;
        let mut output = vec![];
        while let Some(row) = rows.next()? {
            let chunk: RetrievalChunk = serde_json::from_str(&row.get::<_, String>(0)?)?;
            let fields = [
                (row.get::<_, String>(1)?, 3.0),
                (row.get::<_, String>(2)?, 2.0),
                (row.get::<_, String>(3)?, 1.5),
                (row.get::<_, String>(4)?, 2.0),
                (row.get::<_, String>(5)?, 2.0),
                (chunk.text.clone(), 1.0),
            ];
            output.push(RankedInput {
                chunk_id: chunk.chunk_id,
                source_id: chunk.source_id,
                score: weighted_lexical_score(query, &fields)
                    / f64::from(chunk.token_count.saturating_add(1)).sqrt(),
                vector: None,
            });
        }
        output.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| code_unit_compare(&a.chunk_id, &b.chunk_id))
        });
        output.truncate(limit);
        Ok(output)
    }

    pub(crate) fn vector_search(
        &self,
        query: &[f64],
        candidate_keys: &BTreeSet<String>,
        limit: usize,
        provider: &str,
        model: &str,
    ) -> RetrievalResult<Vec<RankedInput>> {
        if self.manifest.embedding_provider_id.as_deref() != Some(provider)
            || self.manifest.embedding_model_id.as_deref() != Some(model)
            || self.manifest.embedding_dimensions != u32::try_from(query.len()).ok()
            || query.iter().any(|value| !value.is_finite())
        {
            return Err(mismatch("VECTOR_SPACE_MISMATCH"));
        }
        self.candidate_vector_eligibility_covers(candidate_keys)?;
        if candidate_keys.is_empty() {
            return Ok(vec![]);
        }
        self.replace_eligible(candidate_keys.iter().map(String::as_str))?;
        let mut statement = self.database.prepare(
            "SELECT v.provider_id,v.model_id,v.dimensions,v.vector_json,c.chunk_json FROM candidate_chunk_vectors AS v JOIN retrieval_eligible AS e ON e.chunk_id=v.candidate_chunk_key JOIN candidate_chunks AS c ON c.candidate_chunk_key=v.candidate_chunk_key ORDER BY v.candidate_chunk_key",
        )?;
        let mut rows = statement.query([])?;
        let mut output = vec![];
        while let Some(row) = rows.next()? {
            let dimensions: u32 = row.get(2)?;
            let vector_json: String = row.get(3)?;
            let vector: Vec<f64> = serde_json::from_str(&vector_json)?;
            let chunk: RetrievalChunk = serde_json::from_str(&row.get::<_, String>(4)?)?;
            if row.get::<_, String>(0)? != provider
                || row.get::<_, String>(1)? != model
                || dimensions as usize != vector.len()
                || vector_json != canonical_json(&vector)?
            {
                return Err(mismatch("VECTOR_SPACE_MISMATCH"));
            }
            output.push(RankedInput {
                chunk_id: chunk.chunk_id,
                source_id: chunk.source_id,
                score: cosine_similarity(query, &vector)?,
                vector: Some(vector),
            });
        }
        output.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| code_unit_compare(&a.chunk_id, &b.chunk_id))
        });
        output.truncate(limit);
        Ok(output)
    }

    pub(crate) fn cached_vectors_by_content(
        &self,
        provider: &str,
        model: &str,
        dimensions: u32,
        runtime_eligible: &BTreeSet<String>,
    ) -> RetrievalResult<BTreeMap<String, Vec<f64>>> {
        if self.manifest.embedding_provider_id.as_deref() != Some(provider)
            || self.manifest.embedding_model_id.as_deref() != Some(model)
            || self.manifest.embedding_dimensions != Some(dimensions)
        {
            return Ok(BTreeMap::new());
        }
        self.candidate_vector_eligibility_covers(runtime_eligible)?;
        if runtime_eligible.is_empty() {
            return Ok(BTreeMap::new());
        }
        self.replace_eligible(runtime_eligible.iter().map(String::as_str))?;
        let mut statement = self.database.prepare(
            "SELECT c.chunk_json,v.vector_json FROM candidate_chunks AS c JOIN candidate_chunk_vectors AS v ON v.candidate_chunk_key=c.candidate_chunk_key JOIN retrieval_eligible AS e ON e.chunk_id=c.candidate_chunk_key ORDER BY c.candidate_chunk_key",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut output = BTreeMap::new();
        for row in rows {
            let (chunk_json, vector_json) = row?;
            let chunk: RetrievalChunk = serde_json::from_str(&chunk_json)?;
            let vector: Vec<f64> = serde_json::from_str(&vector_json)?;
            if vector.len() != dimensions as usize || vector.iter().any(|value| !value.is_finite())
            {
                return Err(mismatch("VECTOR_GENERATION_INVALID"));
            }
            if output
                .insert(chunk.content_digest, vector.clone())
                .is_some_and(|prior| prior != vector)
            {
                return Err(mismatch("CONTENT_VECTOR_CACHE_CONFLICT"));
            }
        }
        Ok(output)
    }

    fn replace_eligible<'a>(&self, keys: impl IntoIterator<Item = &'a str>) -> RetrievalResult<()> {
        self.database.execute_batch(
            "DROP TABLE IF EXISTS temp.retrieval_eligible; CREATE TEMP TABLE retrieval_eligible(chunk_id TEXT PRIMARY KEY);",
        )?;
        let mut insert = self
            .database
            .prepare("INSERT OR IGNORE INTO retrieval_eligible VALUES (?1)")?;
        for key in keys {
            insert.execute([key])?;
        }
        Ok(())
    }

    fn replace_records<'a>(&self, keys: impl IntoIterator<Item = &'a str>) -> RetrievalResult<()> {
        self.database.execute_batch(
            "DROP TABLE IF EXISTS temp.retrieval_candidate_records; CREATE TEMP TABLE retrieval_candidate_records(record_key TEXT PRIMARY KEY);",
        )?;
        let mut insert = self
            .database
            .prepare("INSERT OR IGNORE INTO retrieval_candidate_records VALUES (?1)")?;
        for key in keys {
            insert.execute([key])?;
        }
        Ok(())
    }

    fn vectors(&self) -> RetrievalResult<Vec<GkxCandidateVector>> {
        let mut statement = self.database.prepare(
            "SELECT candidate_chunk_key,provider_id,model_id,dimensions,vector_json FROM candidate_chunk_vectors ORDER BY candidate_chunk_key",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (key, provider, model, dimensions, json) = row?;
            let item = GkxCandidateVector {
                candidate_chunk_key: key,
                vector: serde_json::from_str(&json)?,
            };
            item.validate()?;
            if self.manifest.embedding_provider_id.as_deref() != Some(provider.as_str())
                || self.manifest.embedding_model_id.as_deref() != Some(model.as_str())
                || self.manifest.embedding_dimensions != Some(dimensions)
                || item.vector.len() != dimensions as usize
                || json != canonical_json(&item.vector)?
            {
                return Err(mismatch("VECTOR_GENERATION_INVALID"));
            }
            Ok(item)
        })
        .collect()
    }

    fn verify_fts(&self, chunks: &[GkxCandidateChunk]) -> RetrievalResult<()> {
        let mut statement = self.database.prepare(
            "SELECT chunk_id,title,heading_path,tags,topic,category,text FROM chunk_fts ORDER BY chunk_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() != chunks.len() {
            return Err(mismatch("RETRIEVAL_LEXICAL_PROJECTION_MISMATCH"));
        }
        let expected = chunks
            .iter()
            .map(|item| {
                let chunk = &item.chunk;
                (
                    item.candidate_chunk_key.clone(),
                    chunk.metadata.title.clone().unwrap_or_default(),
                    chunk.heading_path.join(" / "),
                    chunk
                        .metadata
                        .tags
                        .as_ref()
                        .map_or_else(String::new, |v| v.join(" ")),
                    chunk.metadata.topic.clone().unwrap_or_default(),
                    chunk.metadata.category.clone().unwrap_or_default(),
                    chunk.text.clone(),
                )
            })
            .collect::<Vec<_>>();
        if rows != expected {
            return Err(mismatch("RETRIEVAL_LEXICAL_PROJECTION_MISMATCH"));
        }
        Ok(())
    }

    fn verify_schema_shape(&self) -> RetrievalResult<()> {
        fn compact(value: &str) -> String {
            value
                .chars()
                .filter(|character| !character.is_ascii_whitespace())
                .flat_map(char::to_lowercase)
                .collect()
        }
        let fts_sql: String = self.database.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='chunk_fts'",
            [],
            |row| row.get(0),
        )?;
        let expected="createvirtualtablechunk_ftsusingfts5(chunk_idunindexed,title,heading_path,tags,topic,category,text,tokenize='unicode61remove_diacritics2')";
        if compact(&fts_sql) != expected {
            return Err(mismatch("RETRIEVAL_LEXICAL_DDL_MISMATCH"));
        }
        for (table,expected_columns) in [
            ("candidate_sources","record_key,source_id,source_path,source_digest,candidate_json,candidate_digest"),
            ("candidate_declarations","declaration_digest,source_record_key,declaration_json"),
            ("candidate_chunks","candidate_chunk_key,record_key,public_chunk_id,parent_candidate_chunk_key,chunk_json,chunk_digest"),
            ("candidate_chunk_vectors","candidate_chunk_key,provider_id,model_id,dimensions,vector_json"),
            ("embedding_eligible_candidate_chunks","candidate_chunk_key"),
        ] {
            let sql=format!("SELECT group_concat(name, ',') FROM pragma_table_info('{table}') ORDER BY cid");
            let actual:String=self.database.query_row(&sql,[],|row|row.get(0))?;
            if actual!=expected_columns { return Err(mismatch("schema-3 SQLite columns mismatch")); }
        }
        Ok(())
    }

    fn verify(&self) -> RetrievalResult<()> {
        let sources = self.list_candidate_sources()?;
        let declarations = self.list_candidate_declarations()?;
        let chunks = self.list_candidate_chunks()?;
        let eligible = self.embedding_eligible_keys()?;
        let vectors = self.vectors()?;
        let input = GkxRetrievalGenerationInput {
            state_directory: PathBuf::new(),
            engine_version: self.manifest.engine_version.clone(),
            vault_id: self.manifest.vault_id.clone(),
            source_snapshot_digest: self.manifest.source_snapshot_digest.clone(),
            configuration_digest: self.manifest.configuration_digest.clone(),
            policy_digest: self.manifest.policy_digest.clone(),
            candidate_sources: sources.clone(),
            candidate_declarations: declarations.clone(),
            candidate_chunks: chunks.clone(),
            embedding_eligible_candidate_chunk_keys: eligible.clone(),
            vectors: vectors.clone(),
            embedding_provider_id: self.manifest.embedding_provider_id.clone(),
            embedding_model_id: self.manifest.embedding_model_id.clone(),
            embedding_dimensions: self.manifest.embedding_dimensions,
        };
        let prepared = prepare_generation(&input)?;
        if prepared.manifest != self.manifest {
            return Err(mismatch("schema-3 projection digest mismatch"));
        }
        self.verify_fts(&chunks)?;
        Ok(())
    }

    fn embedding_eligible_keys(&self) -> RetrievalResult<Vec<String>> {
        let mut statement=self.database.prepare(
            "SELECT candidate_chunk_key FROM embedding_eligible_candidate_chunks ORDER BY candidate_chunk_key",
        )?;
        let values = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::candidate::{
        candidate_chunk_key, GkxCandidateCategory, GkxCandidateOrigin, GkxResolutionBasis,
        GkxResolutionTier, RETRIEVAL_CANDIDATE_SOURCE_CONTRACT,
    };
    use crate::chunker::{chunk_lineage_source, ChunkingOptions};
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, GkxRetrievalSource, GkxSensitivity,
        RetrievalChunkMetadata, RETRIEVAL_CONTRACT,
    };
    use crate::provenance::{
        GkxAssertionOrigin, GkxStoredSourceProvenance, GkxTemporalState, GkxValidityOrigin,
    };
    use crate::sqlite_store::StoredVector;

    pub(crate) const OLD: &str = "018f0000-0000-7000-8000-000000000301";
    pub(crate) const NEW: &str = "018f0000-0000-7000-8000-000000000302";

    fn seal_stored(mut value: GkxStoredSourceProvenance) -> GkxStoredSourceProvenance {
        let mut json = serde_json::to_value(&value).unwrap();
        json.as_object_mut().unwrap().remove("provenance_digest");
        value.provenance_digest = canonical_digest(&json).unwrap();
        value.validate().unwrap();
        value
    }

    pub(crate) fn source(
        id: &str,
        path: &str,
        text: &str,
        valid_from: &str,
        sensitivity: GkxSensitivity,
    ) -> (GkxRetrievalSource, GkxStoredSourceProvenance) {
        source_with_lineage(
            id,
            path,
            text,
            valid_from,
            None,
            vec![],
            vec![],
            sensitivity,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn source_with_lineage(
        id: &str,
        path: &str,
        text: &str,
        valid_from: &str,
        valid_to: Option<&str>,
        supersedes: Vec<String>,
        superseded_by: Vec<String>,
        sensitivity: GkxSensitivity,
    ) -> (GkxRetrievalSource, GkxStoredSourceProvenance) {
        let neutral = supersedes.is_empty() && superseded_by.is_empty();
        let metadata = RetrievalChunkMetadata {
            title: Some(path.to_owned()),
            authored_at: Some(valid_from.to_owned()),
            sensitivity: Some(sensitivity),
            authoritative: Some(true),
            ..Default::default()
        };
        let temporal = CanonicalTemporalEnvelope {
            assertion_time: Some(valid_from.to_owned()),
            valid_from: Some(valid_from.to_owned()),
            valid_to: valid_to.map(str::to_owned),
            valid_from_unix_ms: Some(
                crate::provenance::normalized_timestamp_millis(valid_from).unwrap(),
            ),
            valid_to_unix_ms: valid_to
                .map(|v| crate::provenance::normalized_timestamp_millis(v).unwrap()),
        };
        let source = GkxRetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "fixture-vault".to_owned(),
            source_id: id.to_owned(),
            source_path: path.to_owned(),
            source_digest: crate::digest::sha256(text.as_bytes()),
            text: text.to_owned(),
            lineage: CanonicalLineageEnvelope {
                lineage_id: None,
                lineage_neutral: neutral,
                supersedes: supersedes.clone(),
                superseded_by: superseded_by.clone(),
                reason_codes: vec![],
            },
            temporal: temporal.clone(),
            metadata: metadata.clone(),
        };
        let mut reasons = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            "VALIDITY_FROM_GKX_AUTHORED_TIMESTAMP".to_owned(),
            if neutral {
                "LINEAGE_NEUTRAL"
            } else {
                "LINEAGE_PARTICIPANT"
            }
            .to_owned(),
        ];
        reasons.sort_by(|a, b| code_unit_compare(a, b));
        let stored = seal_stored(GkxStoredSourceProvenance {
            contract_version: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
            source_id: id.to_owned(),
            source_path: path.to_owned(),
            source_digest: source.source_digest.clone(),
            source_metadata: metadata,
            assertion_time: Some(valid_from.to_owned()),
            assertion_origin: Some(GkxAssertionOrigin::GkxCreatedAt),
            valid_from: Some(valid_from.to_owned()),
            valid_to: valid_to.map(str::to_owned),
            validity_origin: GkxValidityOrigin::GkxAuthoredTimestamp,
            lineage_id: (),
            authored_supersedes: supersedes.clone(),
            authored_superseded_by: superseded_by.clone(),
            resolved_supersedes: supersedes,
            resolved_superseded_by: superseded_by,
            lineage_neutral: neutral,
            temporal_state: if valid_to.is_some() {
                GkxTemporalState::Historical
            } else {
                GkxTemporalState::Current
            },
            ledger_binding_verified: false,
            reason_codes: reasons,
            provenance_digest: String::new(),
        });
        (source, stored)
    }

    pub(crate) fn chunks(sources: &[GkxRetrievalSource]) -> Vec<RetrievalChunk> {
        let mut output = sources
            .iter()
            .flat_map(|source| chunk_lineage_source(source, ChunkingOptions::default()).unwrap())
            .collect::<Vec<_>>();
        output.sort_by(|a, b| code_unit_compare(&a.chunk_id, &b.chunk_id));
        output
    }

    pub(crate) fn generation(
        state_directory: PathBuf,
        pairs: Vec<(GkxRetrievalSource, GkxStoredSourceProvenance)>,
        eligible: Vec<String>,
        vectors: Vec<StoredVector>,
        identity: Option<(&str, &str, u32)>,
    ) -> GkxRetrievalGenerationInput {
        let record_by_id = pairs
            .iter()
            .enumerate()
            .map(|(index, (source, _))| {
                (
                    source.source_id.clone(),
                    format!(
                        "gkx-record:{}:{index}",
                        crate::digest::sha256(source.source_path.as_bytes())
                            .trim_start_matches("sha256:")
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut candidate_sources = Vec::new();
        let mut declarations = Vec::new();
        let mut candidate_chunks = Vec::new();
        for (ordinal, (source, _stored)) in pairs.iter().enumerate() {
            let record_key = record_by_id[&source.source_id].clone();
            let validity_origin = if source.temporal.valid_from.is_some() {
                GkxValidityOrigin::GkxAuthoredTimestamp
            } else {
                GkxValidityOrigin::Unknown
            };
            let mut reasons = vec![
                "LEDGER_BINDING_UNAVAILABLE".to_owned(),
                "LINEAGE_ID_UNAVAILABLE".to_owned(),
                validity_origin.reason().to_owned(),
            ];
            if source.temporal.assertion_time.is_none() {
                reasons.push("ASSERTION_TIME_UNAVAILABLE".to_owned());
            }
            reasons.sort_by(|a, b| code_unit_compare(a, b));
            let mut candidate = GkxCandidateSource {
                contract_version: RETRIEVAL_CANDIDATE_SOURCE_CONTRACT.to_owned(),
                assertion_origin: source
                    .temporal
                    .assertion_time
                    .as_ref()
                    .map(|_| GkxAssertionOrigin::GkxCreatedAt),
                assertion_time: source.temporal.assertion_time.clone(),
                lineage_id: (),
                parser_content_fingerprint: format!("test:{ordinal}"),
                reason_codes: reasons,
                record_key: record_key.clone(),
                source_digest: source.source_digest.clone(),
                source_id: source.source_id.clone(),
                source_metadata: source.metadata.clone(),
                source_path: source.source_path.clone(),
                valid_from: source.temporal.valid_from.clone(),
                validity_origin,
                candidate_digest: String::new(),
            };
            candidate.candidate_digest = candidate.expected_digest().unwrap();
            candidate.validate().unwrap();
            candidate_sources.push(candidate);
            for (field, refs) in [
                ("supersedes", &source.lineage.supersedes),
                ("superseded_by", &source.lineage.superseded_by),
            ] {
                for (index, raw) in refs.iter().enumerate() {
                    declarations.push(GkxCandidateDeclaration {
                        source_record_key: record_key.clone(),
                        category: GkxCandidateCategory::Lineage,
                        field: field.to_owned(),
                        origin: GkxCandidateOrigin::Authored,
                        declaration_index: index as u64,
                        raw_reference: raw.clone(),
                        resolution_tiers: vec![
                            GkxResolutionTier {
                                basis: GkxResolutionBasis::UidExact,
                                candidate_record_keys: record_by_id
                                    .get(raw)
                                    .cloned()
                                    .into_iter()
                                    .collect(),
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
                    });
                }
            }
            let mut nested = chunk_lineage_source(source, ChunkingOptions::default()).unwrap();
            let key_by_public = nested
                .iter()
                .map(|chunk| {
                    (
                        chunk.chunk_id.clone(),
                        candidate_chunk_key(&record_key, &chunk.chunk_id).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            for chunk in &mut nested {
                chunk.valid_to = None;
                chunk.lineage_id = None;
                chunk.supersedes.clear();
                chunk.superseded_by.clear();
                candidate_chunks.push(GkxCandidateChunk {
                    candidate_chunk_key: key_by_public[&chunk.chunk_id].clone(),
                    record_key: record_key.clone(),
                    parent_candidate_chunk_key: chunk
                        .parent_chunk_id
                        .as_ref()
                        .and_then(|id| key_by_public.get(id).cloned()),
                    chunk: chunk.clone(),
                });
            }
        }
        candidate_sources.sort_by(|a, b| code_unit_compare(&a.record_key, &b.record_key));
        candidate_chunks
            .sort_by(|a, b| code_unit_compare(&a.candidate_chunk_key, &b.candidate_chunk_key));
        let key_by_public = candidate_chunks
            .iter()
            .map(|item| {
                (
                    item.chunk.chunk_id.clone(),
                    item.candidate_chunk_key.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let eligible_keys = eligible
            .iter()
            .map(|id| key_by_public[id].clone())
            .collect::<Vec<_>>();
        let candidate_vectors = vectors
            .into_iter()
            .map(|item| GkxCandidateVector {
                candidate_chunk_key: key_by_public[&item.chunk_id].clone(),
                vector: item.vector,
            })
            .collect();
        let (provider, model, dimensions) = identity.map_or((None, None, None), |(p, m, d)| {
            (Some(p.to_owned()), Some(m.to_owned()), Some(d))
        });
        GkxRetrievalGenerationInput {
            state_directory,
            engine_version: "2.1.2".to_owned(),
            vault_id: "fixture-vault".to_owned(),
            source_snapshot_digest: crate::digest::sha256(b"snapshot"),
            configuration_digest: crate::digest::sha256(b"config"),
            policy_digest: crate::digest::sha256(b"phase2-policy"),
            candidate_sources,
            candidate_declarations: declarations,
            candidate_chunks,
            embedding_eligible_candidate_chunk_keys: eligible_keys,
            vectors: candidate_vectors,
            embedding_provider_id: provider,
            embedding_model_id: model,
            embedding_dimensions: dimensions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::candidate_chunk_key;
    use crate::contract::GkxSensitivity;
    use crate::lineage_store::test_support::{chunks, generation, source};
    use crate::sqlite_store::StoredVector;
    use tempfile::tempdir;

    fn vector_generation(state: PathBuf) -> GkxRetrievalGenerationInput {
        let text = "# Same\nReusable content.\n";
        let pairs = vec![
            source(
                test_support::OLD,
                "old.md",
                text,
                "2026-07-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            source(
                test_support::NEW,
                "new.md",
                text,
                "2026-08-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
        ];
        let public = chunks(&pairs.iter().map(|pair| pair.0.clone()).collect::<Vec<_>>());
        let eligible = public
            .iter()
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<Vec<_>>();
        let vectors = public
            .iter()
            .map(|chunk| StoredVector {
                chunk_id: chunk.chunk_id.clone(),
                vector: vec![1.0, 0.0],
            })
            .collect();
        generation(
            state,
            pairs,
            eligible,
            vectors,
            Some(("provider", "model", 2)),
        )
    }

    fn mutable_copy(built: &BuiltGkxRetrievalGeneration, name: &str) -> PathBuf {
        let path = built.database_path().parent().unwrap().join(name);
        fs::copy(built.database_path(), &path).unwrap();
        path
    }

    #[test]
    fn candidate_store_retains_zero_chunk_sources_and_exact_physical_counts() {
        let root = tempdir().unwrap();
        let pair = source(
            test_support::OLD,
            "blank.md",
            "",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let input = generation(root.path().join("state"), vec![pair], vec![], vec![], None);
        let built = build_gkx_retrieval_generation(input).unwrap();
        assert_eq!(built.manifest().candidate_source_count, 1);
        assert_eq!(built.manifest().represented_candidate_source_count, 0);
        assert_eq!(built.manifest().candidate_chunk_count, 0);
        let reopened = GkxSqliteRetrievalStore::open(built.database_path()).unwrap();
        assert_eq!(reopened.list_candidate_sources().unwrap().len(), 1);
        assert!(reopened.list_candidate_chunks().unwrap().is_empty());
    }

    #[test]
    fn schema_three_writes_full_two_key_pointer_and_old_lite_pointer_is_read_only() {
        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            test_support::OLD,
            "old.md",
            "# Old\nVisible text.\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let input = generation(state.clone(), vec![pair], vec![], vec![], None);
        let built = build_gkx_retrieval_generation(input.clone()).unwrap();
        activate_gkx_retrieval_generation(&state, &built).unwrap();
        let pointer_path = state.join(ACTIVE_POINTER_NAME);
        let pointer_bytes = fs::read(&pointer_path).unwrap();
        let mut pointer: serde_json::Value = serde_json::from_slice(&pointer_bytes).unwrap();
        assert_eq!(
            pointer
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            ["database_file", "manifest"]
        );

        pointer.as_object_mut().unwrap().insert(
            "contract_version".to_owned(),
            RETRIEVAL_LINEAGE_CONTRACT.into(),
        );
        let historical = format!("{}\n", canonical_json(&pointer).unwrap()).into_bytes();
        fs::write(&pointer_path, &historical).unwrap();
        let opened = open_active_gkx_retrieval_generation(&state).unwrap();
        assert_eq!(opened.manifest, *built.manifest());
        drop(opened);
        let database_before = fs::read(built.database_path()).unwrap();
        assert!(build_gkx_retrieval_generation(input).is_err());
        assert_eq!(fs::read(pointer_path).unwrap(), historical);
        assert_eq!(fs::read(built.database_path()).unwrap(), database_before);
        assert!(!state.join("retrieval-writer.lock").exists());
    }

    #[cfg(unix)]
    #[test]
    fn schema_three_widened_immutable_generation_is_rejected_without_repair_or_activation() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempdir().unwrap();
        let state = root.path().join("state");
        let pair = source(
            test_support::OLD,
            "old.md",
            "# Old\nVisible text.\n",
            "2026-07-01T00:00:00.000Z",
            GkxSensitivity::Public,
        );
        let input = generation(state.clone(), vec![pair], vec![], vec![], None);
        let built = build_gkx_retrieval_generation(input.clone()).unwrap();
        let database_before = fs::read(built.database_path()).unwrap();
        fs::set_permissions(built.database_path(), fs::Permissions::from_mode(0o644)).unwrap();

        assert!(build_gkx_retrieval_generation(input).is_err());
        assert!(activate_gkx_retrieval_generation(&state, &built).is_err());
        assert_eq!(fs::read(built.database_path()).unwrap(), database_before);
        assert_eq!(
            fs::metadata(built.database_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
        assert!(!state.join(ACTIVE_POINTER_NAME).exists());
        assert!(!state
            .join(crate::writer_lock::LEGACY_WRITER_LOCK_FILE)
            .exists());
    }

    #[test]
    fn candidate_key_scoped_reads_do_not_materialize_other_records() {
        let root = tempdir().unwrap();
        let pairs = vec![
            test_support::source_with_lineage(
                test_support::OLD,
                "old.md",
                "# Old\nVisible text.\n",
                "2026-07-01T00:00:00.000Z",
                None,
                vec![test_support::NEW.to_owned()],
                vec![],
                GkxSensitivity::Public,
            ),
            source(
                test_support::NEW,
                "hidden.md",
                "# Hidden\nDenied text.\n",
                "2026-08-01T00:00:00.000Z",
                GkxSensitivity::Secret,
            ),
        ];
        let input = generation(root.path().join("state"), pairs, vec![], vec![], None);
        let visible_key = input
            .candidate_sources
            .iter()
            .find(|source| source.source_id == test_support::OLD)
            .unwrap()
            .record_key
            .clone();
        let hidden_key = input
            .candidate_sources
            .iter()
            .find(|source| source.source_id == test_support::NEW)
            .unwrap()
            .record_key
            .clone();
        let built = build_gkx_retrieval_generation(input).unwrap();
        let reopened = GkxSqliteRetrievalStore::open(built.database_path()).unwrap();
        let scoped_chunks = reopened
            .list_candidate_chunks_for_record_keys(std::slice::from_ref(&visible_key))
            .unwrap();
        assert!(!scoped_chunks.is_empty());
        assert!(scoped_chunks
            .iter()
            .all(|chunk| chunk.record_key == visible_key));
        assert!(scoped_chunks
            .iter()
            .all(|chunk| chunk.record_key != hidden_key));
        let scoped_declarations = reopened
            .list_candidate_declarations_for_record_keys(std::slice::from_ref(&visible_key))
            .unwrap();
        assert_eq!(scoped_declarations.len(), 1);
        assert!(scoped_declarations
            .iter()
            .all(|declaration| declaration.source_record_key == visible_key));
        assert!(reopened
            .list_candidate_declarations_for_record_keys(&[hidden_key])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn reopen_rejects_resealed_source_chunk_declaration_vector_and_fts_tampering() {
        let root = tempdir().unwrap();
        let built =
            build_gkx_retrieval_generation(vector_generation(root.path().join("state"))).unwrap();

        let quality_path = mutable_copy(&built, "quality.sqlite");
        {
            let database = Connection::open(&quality_path).unwrap();
            let source_json: String = database
                .query_row(
                    "SELECT candidate_json FROM candidate_sources LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let mut source: GkxCandidateSource = serde_json::from_str(&source_json).unwrap();
            source.source_metadata.quality = Some(2.0);
            source.candidate_digest = source.expected_digest().unwrap();
            database.execute("UPDATE candidate_sources SET candidate_json=?1,candidate_digest=?2 WHERE record_key=?3",params![canonical_json(&source).unwrap(),source.candidate_digest,source.record_key]).unwrap();
            let chunk_json: String = database
                .query_row(
                    "SELECT chunk_json FROM candidate_chunks WHERE record_key=?1 LIMIT 1",
                    [&source.record_key],
                    |row| row.get(0),
                )
                .unwrap();
            let mut chunk: RetrievalChunk = serde_json::from_str(&chunk_json).unwrap();
            chunk.metadata.quality = Some(2.0);
            let candidate = GkxCandidateChunk {
                candidate_chunk_key: candidate_chunk_key(&source.record_key, &chunk.chunk_id)
                    .unwrap(),
                record_key: source.record_key.clone(),
                parent_candidate_chunk_key: None,
                chunk,
            };
            database.execute("UPDATE candidate_chunks SET chunk_json=?1,chunk_digest=?2 WHERE candidate_chunk_key=?3",params![canonical_json(&candidate.chunk).unwrap(),canonical_digest(&candidate).unwrap(),candidate.candidate_chunk_key]).unwrap();
        }
        assert!(GkxSqliteRetrievalStore::open(&quality_path).is_err());

        let vector_path = mutable_copy(&built, "vector.sqlite");
        {
            let database = Connection::open(&vector_path).unwrap();
            database.execute("UPDATE candidate_chunk_vectors SET vector_json='[0,1]' WHERE candidate_chunk_key=(SELECT candidate_chunk_key FROM candidate_chunk_vectors LIMIT 1)",[]).unwrap();
        }
        assert!(GkxSqliteRetrievalStore::open(&vector_path).is_err());

        let declaration_path = mutable_copy(&built, "declaration.sqlite");
        {
            let database = Connection::open(&declaration_path).unwrap();
            let source_key: String = database
                .query_row(
                    "SELECT record_key FROM candidate_sources LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let target_key: String = database
                .query_row(
                    "SELECT record_key FROM candidate_sources ORDER BY record_key DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let declaration = GkxCandidateDeclaration {
                source_record_key: source_key.clone(),
                category: crate::candidate::GkxCandidateCategory::Relationship,
                field: "relationships.nonsense".to_owned(),
                origin: crate::candidate::GkxCandidateOrigin::Authored,
                declaration_index: 0,
                raw_reference: "target".to_owned(),
                resolution_tiers: vec![
                    crate::candidate::GkxResolutionTier {
                        basis: crate::candidate::GkxResolutionBasis::UidExact,
                        candidate_record_keys: vec![target_key],
                    },
                    crate::candidate::GkxResolutionTier {
                        basis: crate::candidate::GkxResolutionBasis::PathExact,
                        candidate_record_keys: vec![],
                    },
                    crate::candidate::GkxResolutionTier {
                        basis: crate::candidate::GkxResolutionBasis::PathWithoutExtensionExact,
                        candidate_record_keys: vec![],
                    },
                    crate::candidate::GkxResolutionTier {
                        basis: crate::candidate::GkxResolutionBasis::BasenameTitle,
                        candidate_record_keys: vec![],
                    },
                    crate::candidate::GkxResolutionTier {
                        basis: crate::candidate::GkxResolutionBasis::Alias,
                        candidate_record_keys: vec![],
                    },
                ],
            };
            let digest = canonical_digest(&declaration).unwrap();
            database
                .execute(
                    "INSERT INTO candidate_declarations VALUES (?1,?2,?3)",
                    params![digest, source_key, canonical_json(&declaration).unwrap()],
                )
                .unwrap();
        }
        assert!(GkxSqliteRetrievalStore::open(&declaration_path).is_err());

        let fts_path = mutable_copy(&built, "fts.sqlite");
        {
            let database = Connection::open(&fts_path).unwrap();
            let keys = database
                .prepare("SELECT chunk_id FROM chunk_fts ORDER BY chunk_id")
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            database
                .execute("DELETE FROM chunk_fts WHERE chunk_id=?1", [&keys[1]])
                .unwrap();
            database
                .execute(
                    "INSERT INTO chunk_fts SELECT * FROM chunk_fts WHERE chunk_id=?1",
                    [&keys[0]],
                )
                .unwrap();
        }
        assert!(GkxSqliteRetrievalStore::open(&fts_path).is_err());

        let ddl_path = mutable_copy(&built, "ddl.sqlite");
        {
            let database = Connection::open(&ddl_path).unwrap();
            database.execute_batch("DROP TABLE chunk_fts; CREATE VIRTUAL TABLE chunk_fts USING fts5(chunk_id UNINDEXED,title,heading_path,tags,topic,category,text,tokenize='porter');").unwrap();
        }
        assert!(GkxSqliteRetrievalStore::open(&ddl_path).is_err());
    }

    #[test]
    fn duplicate_content_vectors_accept_identical_payloads_and_reject_conflicts() {
        let root = tempdir().unwrap();
        let valid = vector_generation(root.path().join("valid"));
        build_gkx_retrieval_generation(valid.clone()).unwrap();
        let mut conflicting = valid;
        conflicting.state_directory = root.path().join("conflict");
        conflicting.vectors[1].vector = vec![0.0, 1.0];
        assert!(
            matches!(build_gkx_retrieval_generation(conflicting),Err(RetrievalError::ProjectionMismatch(message)) if message=="CONTENT_VECTOR_CACHE_CONFLICT")
        );
    }
}
