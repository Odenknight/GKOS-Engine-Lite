use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::chunker::{
    chunk_identity, chunk_source, token_count, validate_chunk_set, ChunkingOptions,
};
use crate::contract::{
    is_normalized_relative_path, is_sha256_digest, is_valid_authored_uid, RetrievalChunk,
    RetrievalChunkMetadata, RetrievalProjectionManifest, RetrievalSource, SqliteLexicalBackend,
    CHUNKER_VERSION, MAX_CHUNK_BYTES, PROJECTION_SCHEMA_VERSION, RETRIEVAL_CONTRACT,
    TOKENIZER_VERSION,
};
use crate::digest::{canonical_digest, canonical_json, sha256};
use crate::fusion::{code_unit_compare, cosine_similarity, RankedInput};
use crate::path_security::{contained_path, equivalent_paths};
use crate::{RetrievalError, RetrievalResult};

const MIGRATION: &str = r#"
CREATE TABLE projection_manifest (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  manifest_json TEXT NOT NULL,
  contract_version TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  projection_id TEXT NOT NULL UNIQUE,
  projection_digest TEXT NOT NULL UNIQUE,
  lexical_backend TEXT NOT NULL
);
CREATE TABLE sources (
  source_id TEXT PRIMARY KEY,
  source_path TEXT NOT NULL,
  source_digest TEXT NOT NULL
);
CREATE TABLE chunks (
  chunk_id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES sources(source_id) ON DELETE CASCADE,
  source_path TEXT NOT NULL,
  source_digest TEXT NOT NULL,
  heading_path_json TEXT NOT NULL,
  heading_depth INTEGER NOT NULL,
  ordinal_within_source INTEGER NOT NULL,
  structural_position TEXT NOT NULL,
  part_ordinal INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  content_digest TEXT NOT NULL,
  text TEXT NOT NULL,
  token_count INTEGER NOT NULL,
  parent_chunk_id TEXT REFERENCES chunks(chunk_id) DEFERRABLE INITIALLY DEFERRED,
  lineage_id TEXT,
  valid_from TEXT,
  valid_to TEXT,
  supersedes_json TEXT NOT NULL,
  superseded_by_json TEXT NOT NULL,
  metadata_json TEXT NOT NULL,
  UNIQUE(source_id, ordinal_within_source)
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
CREATE TABLE chunk_vectors (
  chunk_id TEXT PRIMARY KEY REFERENCES chunks(chunk_id) ON DELETE CASCADE,
  provider_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  dimensions INTEGER NOT NULL,
  vector_json TEXT NOT NULL
);
CREATE INDEX chunks_source_idx ON chunks(source_id);
CREATE INDEX chunks_path_idx ON chunks(source_path);
CREATE INDEX chunks_digest_idx ON chunks(source_digest);
CREATE INDEX chunks_parent_idx ON chunks(parent_chunk_id);
"#;

const ACTIVE_POINTER_NAME: &str = "active-retrieval.json";
const VECTOR_ELIGIBLE_SQL: &str =
    "SELECT v.chunk_id, c.source_id, v.provider_id, v.model_id, v.dimensions, v.vector_json
     FROM chunk_vectors AS v
     JOIN retrieval_eligible AS e ON e.chunk_id = v.chunk_id
     JOIN chunks AS c ON c.chunk_id = v.chunk_id
     ORDER BY v.chunk_id";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredVector {
    pub chunk_id: String,
    pub vector: Vec<f64>,
}

#[derive(Clone)]
pub struct RetrievalGenerationInput {
    pub state_directory: PathBuf,
    pub engine_version: String,
    pub vault_id: String,
    pub source_snapshot_digest: String,
    pub configuration_digest: String,
    pub policy_digest: String,
    pub sources: Vec<RetrievalSource>,
    pub chunking: ChunkingOptions,
    pub vectors: Vec<StoredVector>,
    pub embedding_provider_id: Option<String>,
    pub embedding_model_id: Option<String>,
    pub embedding_dimensions: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltRetrievalGeneration {
    pub database_path: PathBuf,
    pub manifest: RetrievalProjectionManifest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ActiveRetrievalPointer {
    contract_version: String,
    database_file: String,
    manifest: RetrievalProjectionManifest,
}

#[derive(Serialize)]
struct ProjectionDigestEnvelope<'a> {
    contract_version: &'a str,
    projection_schema_version: u32,
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
    source_count: u32,
    chunk_count: u32,
    chunks: &'a [RetrievalChunk],
    vectors: &'a [StoredVector],
}

pub fn build_retrieval_generation(
    input: RetrievalGenerationInput,
) -> RetrievalResult<BuiltRetrievalGeneration> {
    let state_directory = validate_state_directory(&input.state_directory)?;
    let (chunks, vectors, manifest) = prepare_generation(&input)?;
    fs::create_dir_all(&state_directory)?;
    let state_directory = validate_existing_state_directory(&state_directory)?;
    harden_directory_permissions(&state_directory)?;
    let suffix = manifest.projection_digest.trim_start_matches("sha256:");
    let final_path = state_directory.join(format!("retrieval-{suffix}.sqlite"));
    if path_entry_exists(&final_path)? {
        match SqliteRetrievalStore::open(&final_path) {
            Ok(store) if store.manifest == manifest => {
                drop(store);
                harden_file_permissions(&final_path)?;
                return Ok(BuiltRetrievalGeneration {
                    database_path: final_path,
                    manifest,
                });
            }
            Ok(_) => {
                quarantine_generation_files(&final_path)?;
            }
            Err(error) if is_quarantinable_store_error(&error) => {
                quarantine_generation_files(&final_path)?;
            }
            Err(error) => return Err(error),
        }
    } else {
        quarantine_orphan_sidecars(&final_path)?;
    }
    let temporary_path =
        state_directory.join(format!(".retrieval-{suffix}-{}.tmp", std::process::id()));
    if path_entry_exists(&temporary_path)? {
        quarantine_generation_files(&temporary_path)?;
    }
    let build_result = insert_generation(&temporary_path, &manifest, &chunks, &vectors);
    if let Err(error) = build_result {
        let _ = quarantine_generation_files(&temporary_path);
        return Err(error);
    }
    harden_file_permissions(&temporary_path)?;
    let verified = SqliteRetrievalStore::open(&temporary_path)?;
    if verified.manifest != manifest || verified.count_chunks()? != chunks.len() {
        drop(verified);
        let _ = quarantine_generation_files(&temporary_path);
        return Err(RetrievalError::ProjectionMismatch(
            "new retrieval generation did not verify".to_owned(),
        ));
    }
    drop(verified);
    fs::rename(&temporary_path, &final_path)?;
    harden_file_permissions(&final_path)?;
    sync_directory(&state_directory)?;
    let final_verified = match SqliteRetrievalStore::open(&final_path) {
        Ok(store) => store,
        Err(error) => {
            let _ = quarantine_generation_files(&final_path);
            return Err(error);
        }
    };
    if final_verified.manifest != manifest || final_verified.count_chunks()? != chunks.len() {
        drop(final_verified);
        quarantine_generation_files(&final_path)?;
        return Err(RetrievalError::ProjectionMismatch(
            "renamed retrieval generation did not verify at its final path".to_owned(),
        ));
    }
    drop(final_verified);
    Ok(BuiltRetrievalGeneration {
        database_path: final_path,
        manifest,
    })
}

/// Atomically selects a verified immutable generation as the service's active
/// generation. The pointer contains no source content or credentials.
pub fn activate_retrieval_generation(
    state_directory: &Path,
    generation: &BuiltRetrievalGeneration,
) -> RetrievalResult<()> {
    let state_directory = validate_existing_state_directory(state_directory)?;
    let database_path = fs::canonicalize(&generation.database_path)?;
    if !database_path
        .parent()
        .is_some_and(|parent| equivalent_paths(parent, &state_directory))
        || !contained_path(&database_path, &state_directory)
    {
        return Err(RetrievalError::InvalidConfig(
            "active retrieval generation must be directly contained by its state directory"
                .to_owned(),
        ));
    }
    reject_file_alias(&database_path, "retrieval generation")?;
    let verified = SqliteRetrievalStore::open(&database_path)?;
    if verified.manifest != generation.manifest {
        return Err(RetrievalError::ProjectionMismatch(
            "active generation manifest differs from its verified database".to_owned(),
        ));
    }
    drop(verified);
    let database_file = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            RetrievalError::InvalidConfig(
                "retrieval generation filename is not portable UTF-8".to_owned(),
            )
        })?
        .to_owned();
    let pointer = ActiveRetrievalPointer {
        contract_version: RETRIEVAL_CONTRACT.to_owned(),
        database_file,
        manifest: generation.manifest.clone(),
    };
    let bytes = format!("{}\n", canonical_json(&pointer)?).into_bytes();
    let temporary_path =
        state_directory.join(format!(".active-retrieval-{}.tmp", std::process::id()));
    if path_entry_exists(&temporary_path)? {
        quarantine_file(&temporary_path)?;
    }
    write_owner_file(&temporary_path, &bytes)?;
    let active_path = state_directory.join(ACTIVE_POINTER_NAME);
    if path_entry_exists(&active_path)? {
        reject_file_alias(&active_path, "active retrieval pointer")?;
    }
    atomic_replace(&temporary_path, &active_path)?;
    sync_directory(&state_directory)?;
    let reopened = open_active_retrieval_generation(&state_directory)?;
    if reopened.manifest != generation.manifest {
        return Err(RetrievalError::ProjectionMismatch(
            "active retrieval pointer did not round-trip".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn try_open_active_retrieval_generation(
    state_directory: &Path,
) -> RetrievalResult<Option<SqliteRetrievalStore>> {
    let state_directory = validate_state_directory(state_directory)?;
    let active_path = state_directory.join(ACTIVE_POINTER_NAME);
    if !path_entry_exists(&active_path)? {
        return Ok(None);
    }
    open_active_retrieval_generation(&state_directory).map(Some)
}

pub(crate) fn open_active_retrieval_generation(
    state_directory: &Path,
) -> RetrievalResult<SqliteRetrievalStore> {
    let state_directory = validate_existing_state_directory(state_directory)?;
    let active_path = state_directory.join(ACTIVE_POINTER_NAME);
    reject_file_alias(&active_path, "active retrieval pointer")?;
    if fs::metadata(&active_path)?.len() > 1_048_576 {
        return Err(RetrievalError::ProjectionMismatch(
            "active retrieval pointer exceeds one MiB".to_owned(),
        ));
    }
    let bytes = fs::read(&active_path)?;
    let pointer: ActiveRetrievalPointer = serde_json::from_slice(&bytes)?;
    if pointer.contract_version != RETRIEVAL_CONTRACT
        || bytes != format!("{}\n", canonical_json(&pointer)?).as_bytes()
    {
        return Err(RetrievalError::ProjectionMismatch(
            "active retrieval pointer is noncanonical or incompatible".to_owned(),
        ));
    }
    pointer.manifest.validate()?;
    let expected_file = format!(
        "retrieval-{}.sqlite",
        pointer
            .manifest
            .projection_digest
            .trim_start_matches("sha256:")
    );
    if pointer.database_file != expected_file
        || Path::new(&pointer.database_file)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(pointer.database_file.as_str())
    {
        return Err(RetrievalError::ProjectionMismatch(
            "active retrieval filename is not digest-bound".to_owned(),
        ));
    }
    let database_path = state_directory.join(&pointer.database_file);
    reject_file_alias(&database_path, "active retrieval generation")?;
    let store = SqliteRetrievalStore::open(&database_path)?;
    if store.manifest != pointer.manifest {
        return Err(RetrievalError::ProjectionMismatch(
            "active pointer and retrieval manifest disagree".to_owned(),
        ));
    }
    Ok(store)
}

fn validate_state_directory(path: &Path) -> RetrievalResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(RetrievalError::InvalidConfig(
            "state_directory must not be empty".to_owned(),
        ));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(RetrievalError::InvalidConfig(
            "state_directory must not contain dot or parent traversal components".to_owned(),
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute.parent().is_none() {
        return Err(RetrievalError::InvalidConfig(
            "state_directory cannot be a filesystem root".to_owned(),
        ));
    }
    let mut existing = absolute.as_path();
    while !path_entry_exists(existing)? {
        existing = existing.parent().ok_or_else(|| {
            RetrievalError::InvalidConfig("state_directory has no existing ancestor".to_owned())
        })?;
    }
    if !equivalent_paths(&fs::canonicalize(existing)?, existing) {
        return Err(RetrievalError::InvalidConfig(
            "state_directory traverses a filesystem alias".to_owned(),
        ));
    }
    Ok(absolute)
}

fn validate_existing_state_directory(path: &Path) -> RetrievalResult<PathBuf> {
    let path = validate_state_directory(path)?;
    reject_symlink_directory(&path)?;
    let canonical = fs::canonicalize(&path)?;
    if !equivalent_paths(&canonical, &path) {
        return Err(RetrievalError::InvalidConfig(
            "state_directory resolves through an alias".to_owned(),
        ));
    }
    Ok(canonical)
}

fn reject_symlink_directory(path: &Path) -> RetrievalResult<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(RetrievalError::InvalidConfig(
            "state_directory must be a real directory".to_owned(),
        ));
    }
    Ok(())
}

fn reject_file_alias(path: &Path, label: &str) -> RetrievalResult<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || file_link_count(path, &metadata)? > 1
    {
        return Err(RetrievalError::InvalidConfig(format!(
            "{label} must be a non-aliased regular file"
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        RetrievalError::InvalidConfig(format!("{label} has no containing directory"))
    })?;
    if !equivalent_paths(
        &fs::canonicalize(parent)?
            .join(path.file_name().ok_or_else(|| {
                RetrievalError::InvalidConfig(format!("{label} has no filename"))
            })?),
        path,
    ) {
        return Err(RetrievalError::InvalidConfig(format!(
            "{label} resolves through a filesystem alias"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn file_link_count(_path: &Path, metadata: &fs::Metadata) -> RetrievalResult<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink())
}

#[cfg(windows)]
fn file_link_count(path: &Path, _metadata: &fs::Metadata) -> RetrievalResult<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the File owns the valid handle and the output pointer is live.
    let succeeded = unsafe {
        GetFileInformationByHandle(file.as_raw_handle() as _, &mut information as *mut _)
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(u64::from(information.nNumberOfLinks))
}

#[cfg(not(any(unix, windows)))]
fn file_link_count(_path: &Path, _metadata: &fs::Metadata) -> RetrievalResult<u64> {
    Ok(1)
}

fn is_quarantinable_store_error(error: &RetrievalError) -> bool {
    matches!(
        error,
        RetrievalError::ProjectionMismatch(_)
            | RetrievalError::Serialization(_)
            | RetrievalError::Sqlite(_)
    )
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn path_entry_exists(path: &Path) -> RetrievalResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn quarantine_generation_files(path: &Path) -> RetrievalResult<()> {
    for candidate in [
        path.to_path_buf(),
        sidecar_path(path, "-wal"),
        sidecar_path(path, "-shm"),
    ] {
        if path_entry_exists(&candidate)? {
            quarantine_file(&candidate)?;
        }
    }
    Ok(())
}

fn quarantine_orphan_sidecars(path: &Path) -> RetrievalResult<()> {
    for candidate in [sidecar_path(path, "-wal"), sidecar_path(path, "-shm")] {
        if path_entry_exists(&candidate)? {
            quarantine_file(&candidate)?;
        }
    }
    Ok(())
}

fn reject_generation_sidecars(path: &Path) -> RetrievalResult<()> {
    if [sidecar_path(path, "-wal"), sidecar_path(path, "-shm")]
        .iter()
        .try_fold(false, |found, candidate| {
            Ok::<_, RetrievalError>(found || path_entry_exists(candidate)?)
        })?
    {
        return Err(RetrievalError::ProjectionMismatch(
            "immutable retrieval generation has a SQLite sidecar".to_owned(),
        ));
    }
    Ok(())
}

fn quarantine_file(path: &Path) -> RetrievalResult<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        RetrievalError::InvalidConfig("quarantine target has no parent".to_owned())
    })?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            RetrievalError::InvalidConfig("quarantine target filename is not UTF-8".to_owned())
        })?;
    for ordinal in 0_u32..1_000 {
        let destination = parent.join(format!(
            ".quarantine-{filename}-{}-{ordinal}",
            std::process::id()
        ));
        if !path_entry_exists(&destination)? {
            fs::rename(path, &destination)?;
            return Ok(destination);
        }
    }
    Err(RetrievalError::ProjectionMismatch(
        "quarantine namespace is exhausted".to_owned(),
    ))
}

fn write_owner_file(path: &Path, bytes: &[u8]) -> RetrievalResult<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn harden_directory_permissions(path: &Path) -> RetrievalResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_directory_permissions(_path: &Path) -> RetrievalResult<()> {
    Ok(())
}

#[cfg(unix)]
fn harden_file_permissions(path: &Path) -> RetrievalResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_file_permissions(_path: &Path) -> RetrievalResult<()> {
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> RetrievalResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are NUL-terminated and remain live during the call.
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> RetrievalResult<()> {
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> RetrievalResult<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> RetrievalResult<()> {
    Ok(())
}

fn prepare_generation(
    input: &RetrievalGenerationInput,
) -> RetrievalResult<(
    Vec<RetrievalChunk>,
    Vec<StoredVector>,
    RetrievalProjectionManifest,
)> {
    if input.engine_version.trim().is_empty() || input.vault_id.trim().is_empty() {
        return Err(RetrievalError::InvalidConfig(
            "engine_version and vault_id must not be empty".to_owned(),
        ));
    }
    let mut sources = input.sources.iter().collect::<Vec<_>>();
    sources.sort_by(|left, right| code_unit_compare(&left.source_id, &right.source_id));
    let mut source_ids = BTreeSet::new();
    let mut chunks = Vec::new();
    for source in sources {
        if source.vault_id != input.vault_id || !source_ids.insert(source.source_id.clone()) {
            return Err(RetrievalError::InvalidEnvelope(
                "source vault binding or stable identity is inconsistent".to_owned(),
            ));
        }
        let source_chunks = chunk_source(source, input.chunking)?;
        validate_chunk_set(source, &source_chunks)?;
        chunks.extend(source_chunks);
    }
    chunks.sort_by(|left, right| code_unit_compare(&left.chunk_id, &right.chunk_id));
    let mut vectors = input.vectors.clone();
    vectors.sort_by(|left, right| code_unit_compare(&left.chunk_id, &right.chunk_id));
    validate_vectors(input, &chunks, &vectors)?;
    let represented_source_ids = chunks
        .iter()
        .map(|chunk| chunk.source_id.as_str())
        .collect::<BTreeSet<_>>();
    let source_count = u32::try_from(represented_source_ids.len())
        .map_err(|_| RetrievalError::InvalidConfig("too many sources".to_owned()))?;
    let chunk_count = u32::try_from(chunks.len())
        .map_err(|_| RetrievalError::InvalidConfig("too many chunks".to_owned()))?;
    let envelope = ProjectionDigestEnvelope {
        contract_version: RETRIEVAL_CONTRACT,
        projection_schema_version: PROJECTION_SCHEMA_VERSION,
        engine_version: &input.engine_version,
        vault_id: &input.vault_id,
        source_snapshot_digest: &input.source_snapshot_digest,
        configuration_digest: &input.configuration_digest,
        policy_digest: &input.policy_digest,
        chunker_version: CHUNKER_VERSION,
        tokenizer_version: TOKENIZER_VERSION,
        lexical_backend: SqliteLexicalBackend::SqliteFts5,
        embedding_provider_id: &input.embedding_provider_id,
        embedding_model_id: &input.embedding_model_id,
        embedding_dimensions: input.embedding_dimensions,
        source_count,
        chunk_count,
        chunks: &chunks,
        vectors: &vectors,
    };
    let projection_digest = canonical_digest(&envelope)?;
    let projection_id = format!("retrieval:{}", &projection_digest[7..31]);
    let manifest = RetrievalProjectionManifest {
        contract_version: RETRIEVAL_CONTRACT.to_owned(),
        projection_schema_version: PROJECTION_SCHEMA_VERSION,
        projection_id,
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
        source_count,
        chunk_count,
        projection_digest,
    };
    manifest.validate()?;
    Ok((chunks, vectors, manifest))
}

fn validate_vectors(
    input: &RetrievalGenerationInput,
    chunks: &[RetrievalChunk],
    vectors: &[StoredVector],
) -> RetrievalResult<()> {
    let identity = (
        input.embedding_provider_id.as_ref(),
        input.embedding_model_id.as_ref(),
        input.embedding_dimensions,
    );
    let dimensions = match identity {
        (None, None, None) if vectors.is_empty() => return Ok(()),
        (Some(provider), Some(model), Some(dimensions))
            if !provider.is_empty() && !model.is_empty() && dimensions > 0 =>
        {
            dimensions
        }
        _ => {
            return Err(RetrievalError::ProjectionMismatch(
                "vector identity must be complete and vectors must not silently cross spaces"
                    .to_owned(),
            ));
        }
    };
    if vectors.len() != chunks.len() {
        return Err(RetrievalError::ProjectionMismatch(
            "a vector generation must be complete or absent".to_owned(),
        ));
    }
    let chunk_ids = chunks
        .iter()
        .map(|chunk| chunk.chunk_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for vector in vectors {
        if !chunk_ids.contains(vector.chunk_id.as_str()) || !seen.insert(&vector.chunk_id) {
            return Err(RetrievalError::ProjectionMismatch(
                "vector rows must bind one-to-one to chunks".to_owned(),
            ));
        }
        if vector.vector.len() != dimensions as usize
            || vector.vector.iter().any(|value| !value.is_finite())
        {
            return Err(RetrievalError::ProjectionMismatch(
                "vector dimensions or values are invalid".to_owned(),
            ));
        }
    }
    Ok(())
}

fn insert_generation(
    path: &Path,
    manifest: &RetrievalProjectionManifest,
    chunks: &[RetrievalChunk],
    vectors: &[StoredVector],
) -> RetrievalResult<()> {
    let mut database = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    database.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    ensure_fts5(&database)?;
    let transaction = database.transaction()?;
    transaction.execute_batch(MIGRATION)?;
    transaction.pragma_update(None, "user_version", PROJECTION_SCHEMA_VERSION)?;
    transaction.execute(
        "INSERT INTO projection_manifest VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            canonical_json(manifest)?,
            manifest.contract_version,
            manifest.projection_schema_version,
            manifest.projection_id,
            manifest.projection_digest,
            manifest.lexical_backend.as_str(),
        ],
    )?;
    let vector_by_chunk = vectors
        .iter()
        .map(|vector| (vector.chunk_id.as_str(), vector))
        .collect::<BTreeMap<_, _>>();
    for chunk in chunks {
        let start_byte = sqlite_integer(chunk.start_byte, "start_byte")?;
        let end_byte = sqlite_integer(chunk.end_byte, "end_byte")?;
        transaction.execute(
            "INSERT OR IGNORE INTO sources VALUES (?1, ?2, ?3)",
            params![chunk.source_id, chunk.source_path, chunk.source_digest],
        )?;
        transaction.execute(
            "INSERT INTO chunks VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
            params![
                chunk.chunk_id,
                chunk.source_id,
                chunk.source_path,
                chunk.source_digest,
                canonical_json(&chunk.heading_path)?,
                chunk.heading_depth,
                chunk.ordinal_within_source,
                chunk.structural_position,
                chunk.part_ordinal,
                start_byte,
                end_byte,
                chunk.start_line,
                chunk.end_line,
                chunk.content_digest,
                chunk.text,
                chunk.token_count,
                chunk.parent_chunk_id,
                chunk.lineage_id,
                chunk.valid_from,
                chunk.valid_to,
                canonical_json(&chunk.supersedes)?,
                canonical_json(&chunk.superseded_by)?,
                canonical_json(&chunk.metadata)?,
            ],
        )?;
        transaction.execute(
            "INSERT INTO chunk_fts VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                chunk.chunk_id,
                chunk.metadata.title.as_deref().unwrap_or(""),
                chunk.heading_path.join(" / "),
                chunk
                    .metadata
                    .tags
                    .as_ref()
                    .map_or_else(String::new, |tags| tags.join(" ")),
                chunk.metadata.topic.as_deref().unwrap_or(""),
                chunk.metadata.category.as_deref().unwrap_or(""),
                chunk.text,
            ],
        )?;
        if let Some(vector) = vector_by_chunk.get(chunk.chunk_id.as_str()) {
            transaction.execute(
                "INSERT INTO chunk_vectors VALUES (?1,?2,?3,?4,?5)",
                params![
                    vector.chunk_id,
                    manifest.embedding_provider_id,
                    manifest.embedding_model_id,
                    manifest.embedding_dimensions,
                    canonical_json(&vector.vector)?,
                ],
            )?;
        }
    }
    transaction.commit()?;
    database.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;")?;
    Ok(())
}

pub(crate) fn ensure_fts5(database: &Connection) -> RetrievalResult<()> {
    let enabled: i64 = database.query_row(
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
        [],
        |row| row.get(0),
    )?;
    if enabled != 1 {
        return Err(RetrievalError::ProjectionMismatch(
            "bundled SQLite was compiled without ENABLE_FTS5".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) struct SqliteRetrievalStore {
    database: Connection,
    pub(crate) manifest: RetrievalProjectionManifest,
}

impl SqliteRetrievalStore {
    pub(crate) fn open(path: &Path) -> RetrievalResult<Self> {
        reject_file_alias(path, "retrieval generation")?;
        reject_generation_sidecars(path)?;
        let database = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // The main database is opened read-only. Keep TEMP writable because
        // policy-eligible IDs are materialized only in a connection-local
        // table and never mutate the immutable projection.
        database.execute_batch("PRAGMA foreign_keys = ON; PRAGMA temp_store = MEMORY;")?;
        ensure_fts5(&database)?;
        let version: u32 = database.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != PROJECTION_SCHEMA_VERSION {
            return Err(RetrievalError::ProjectionMismatch(
                "retrieval SQLite schema version is incompatible".to_owned(),
            ));
        }
        let integrity: String =
            database.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(RetrievalError::ProjectionMismatch(
                "retrieval SQLite integrity check failed".to_owned(),
            ));
        }
        let foreign_key_failures: i64 =
            database.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if foreign_key_failures != 0 {
            return Err(RetrievalError::ProjectionMismatch(
                "retrieval SQLite foreign key check failed".to_owned(),
            ));
        }
        let (manifest_json, contract_version, schema_version, projection_id, projection_digest, lexical_backend): (
            String,
            String,
            u32,
            String,
            String,
            String,
        ) = database.query_row(
            "SELECT manifest_json, contract_version, schema_version, projection_id, projection_digest, lexical_backend FROM projection_manifest WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )?;
        let manifest: RetrievalProjectionManifest = serde_json::from_str(&manifest_json)?;
        manifest.validate()?;
        if manifest.lexical_backend != SqliteLexicalBackend::SqliteFts5 {
            return Err(RetrievalError::ProjectionMismatch(
                "Lite does not implement the Node-only sqlite_lexical_scan backend".to_owned(),
            ));
        }
        if contract_version != manifest.contract_version
            || schema_version != manifest.projection_schema_version
            || projection_id != manifest.projection_id
            || projection_digest != manifest.projection_digest
            || lexical_backend != manifest.lexical_backend.as_str()
            || manifest_json != canonical_json(&manifest)?
        {
            return Err(RetrievalError::ProjectionMismatch(
                "retrieval manifest columns or canonical bytes disagree".to_owned(),
            ));
        }
        let store = Self { database, manifest };
        store.verify_persisted_projection()?;
        Ok(store)
    }

    pub(crate) fn count_chunks(&self) -> RetrievalResult<usize> {
        let count: i64 = self
            .database
            .query_row("SELECT count(*) FROM chunks", [], |row| row.get(0))?;
        usize::try_from(count).map_err(|_| {
            RetrievalError::ProjectionMismatch("negative SQLite chunk count".to_owned())
        })
    }

    pub(crate) fn list_chunks(&self) -> RetrievalResult<Vec<RetrievalChunk>> {
        let mut statement = self.database.prepare("SELECT chunk_id, source_id, source_path, source_digest, heading_path_json, heading_depth, ordinal_within_source, structural_position, part_ordinal, start_byte, end_byte, start_line, end_line, content_digest, text, token_count, parent_chunk_id, lineage_id, valid_from, valid_to, supersedes_json, superseded_by_json, metadata_json FROM chunks ORDER BY chunk_id")?;
        let mut rows = statement.query([])?;
        let mut chunks = Vec::new();
        while let Some(row) = rows.next()? {
            let stored_start: i64 = row.get(9)?;
            let stored_end: i64 = row.get(10)?;
            let start_byte = u64::try_from(stored_start).map_err(|_| {
                RetrievalError::ProjectionMismatch("stored start_byte is negative".to_owned())
            })?;
            let end_byte = u64::try_from(stored_end).map_err(|_| {
                RetrievalError::ProjectionMismatch("stored end_byte is negative".to_owned())
            })?;
            chunks.push(RetrievalChunk {
                chunk_id: row.get(0)?,
                source_id: row.get(1)?,
                source_path: row.get(2)?,
                source_digest: row.get(3)?,
                heading_path: serde_json::from_str(&row.get::<_, String>(4)?)?,
                heading_depth: row.get(5)?,
                ordinal_within_source: row.get(6)?,
                structural_position: row.get(7)?,
                part_ordinal: row.get(8)?,
                start_byte,
                end_byte,
                start_line: row.get(11)?,
                end_line: row.get(12)?,
                content_digest: row.get(13)?,
                text: row.get(14)?,
                token_count: row.get(15)?,
                parent_chunk_id: row.get(16)?,
                lineage_id: row.get(17)?,
                valid_from: row.get(18)?,
                valid_to: row.get(19)?,
                supersedes: serde_json::from_str(&row.get::<_, String>(20)?)?,
                superseded_by: serde_json::from_str(&row.get::<_, String>(21)?)?,
                metadata: serde_json::from_str(&row.get::<_, String>(22)?)?,
            });
        }
        Ok(chunks)
    }

    /// Returns only vectors from this verified generation whose complete
    /// embedding-space identity matches the requested identity. Keys are
    /// content digests so unchanged bytes can be reused across chunk IDs.
    pub(crate) fn cached_vectors_by_content(
        &self,
        provider_id: &str,
        model_id: &str,
        dimensions: u32,
    ) -> RetrievalResult<BTreeMap<String, Vec<f64>>> {
        if self.manifest.embedding_provider_id.as_deref() != Some(provider_id)
            || self.manifest.embedding_model_id.as_deref() != Some(model_id)
            || self.manifest.embedding_dimensions != Some(dimensions)
        {
            return Ok(BTreeMap::new());
        }
        let mut statement = self.database.prepare(
            "SELECT c.content_digest, v.provider_id, v.model_id, v.dimensions, v.vector_json
             FROM chunks AS c JOIN chunk_vectors AS v ON v.chunk_id = c.chunk_id
             ORDER BY c.content_digest, c.chunk_id",
        )?;
        let mut rows = statement.query([])?;
        let mut output = BTreeMap::<String, Vec<f64>>::new();
        while let Some(row) = rows.next()? {
            let content_digest: String = row.get(0)?;
            let stored_provider: String = row.get(1)?;
            let stored_model: String = row.get(2)?;
            let stored_dimensions: u32 = row.get(3)?;
            let vector: Vec<f64> = serde_json::from_str(&row.get::<_, String>(4)?)?;
            if stored_provider != provider_id
                || stored_model != model_id
                || stored_dimensions != dimensions
                || vector.len() != dimensions as usize
                || vector.iter().any(|value| !value.is_finite())
            {
                return Err(RetrievalError::ProjectionMismatch(
                    "cached vector crossed an embedding space".to_owned(),
                ));
            }
            if let Some(existing) = output.insert(content_digest, vector.clone()) {
                if existing != vector {
                    return Err(RetrievalError::ProjectionMismatch(
                        "one content digest is bound to conflicting cached vectors".to_owned(),
                    ));
                }
            }
        }
        Ok(output)
    }

    /// Search only an already policy-eligible set. Callers must derive this set
    /// before scoring so hidden records cannot affect ranks or aggregates.
    pub(crate) fn lexical_search_eligible(
        &self,
        query: &str,
        eligible: &[String],
        limit: usize,
    ) -> RetrievalResult<Vec<RankedInput>> {
        if limit == 0 {
            return Err(RetrievalError::InvalidConfig(
                "lexical search limit must be positive".to_owned(),
            ));
        }
        let expression = fts_expression(query)?;
        if eligible.is_empty() {
            return Ok(Vec::new());
        }
        self.replace_eligible_ids(eligible.iter().map(String::as_str))?;
        let mut statement = self.database.prepare(
            "SELECT c.chunk_id, c.source_id, c.token_count, c.text, f.title, f.heading_path, f.tags, f.topic, f.category
             FROM chunk_fts AS f
             JOIN retrieval_eligible AS e ON e.chunk_id = f.chunk_id
             JOIN chunks AS c ON c.chunk_id = f.chunk_id
             WHERE chunk_fts MATCH ?1",
        )?;
        let mut rows = statement.query([expression])?;
        let mut output = Vec::new();
        while let Some(row) = rows.next()? {
            let token_count: u32 = row.get(2)?;
            let fields = [
                (row.get::<_, String>(4)?, 3.0),
                (row.get::<_, String>(5)?, 2.0),
                (row.get::<_, String>(6)?, 1.5),
                (row.get::<_, String>(7)?, 2.0),
                (row.get::<_, String>(8)?, 2.0),
                (row.get::<_, String>(3)?, 1.0),
            ];
            let weighted = weighted_lexical_score(query, &fields);
            output.push(RankedInput {
                chunk_id: row.get(0)?,
                source_id: row.get(1)?,
                score: weighted / f64::from(token_count.saturating_add(1)).sqrt(),
                vector: None,
            });
        }
        output.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| code_unit_compare(&left.chunk_id, &right.chunk_id))
        });
        output.truncate(limit);
        Ok(output)
    }

    pub(crate) fn vector_search(
        &self,
        query_vector: &[f64],
        eligible_chunk_ids: &BTreeSet<String>,
        limit: usize,
        provider_id: &str,
        model_id: &str,
    ) -> RetrievalResult<Vec<RankedInput>> {
        if limit == 0
            || self.manifest.embedding_provider_id.as_deref() != Some(provider_id)
            || self.manifest.embedding_model_id.as_deref() != Some(model_id)
            || self.manifest.embedding_dimensions != u32::try_from(query_vector.len()).ok()
            || query_vector.iter().any(|value| !value.is_finite())
        {
            return Err(RetrievalError::ProjectionMismatch(
                "query vector does not match the immutable vector space".to_owned(),
            ));
        }
        self.replace_eligible_ids(eligible_chunk_ids.iter().map(String::as_str))?;
        let mut statement = self.database.prepare(VECTOR_ELIGIBLE_SQL)?;
        let mut rows = statement.query([])?;
        let mut output = Vec::new();
        while let Some(row) = rows.next()? {
            let chunk_id: String = row.get(0)?;
            let source_id: String = row.get(1)?;
            let stored_provider: String = row.get(2)?;
            let stored_model: String = row.get(3)?;
            let dimensions: u32 = row.get(4)?;
            let vector: Vec<f64> = serde_json::from_str(&row.get::<_, String>(5)?)?;
            if stored_provider != provider_id
                || stored_model != model_id
                || dimensions as usize != vector.len()
            {
                return Err(RetrievalError::ProjectionMismatch(
                    "stored vector crossed an embedding space".to_owned(),
                ));
            }
            output.push(RankedInput {
                chunk_id,
                source_id,
                score: cosine_similarity(query_vector, &vector)?,
                vector: Some(vector),
            });
        }
        output.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| code_unit_compare(&left.chunk_id, &right.chunk_id))
        });
        output.truncate(limit);
        Ok(output)
    }

    fn replace_eligible_ids<'a>(
        &self,
        eligible: impl IntoIterator<Item = &'a str>,
    ) -> RetrievalResult<()> {
        self.database.execute_batch(
            "DROP TABLE IF EXISTS temp.retrieval_eligible; CREATE TEMP TABLE retrieval_eligible(chunk_id TEXT PRIMARY KEY);",
        )?;
        let mut insert = self
            .database
            .prepare("INSERT OR IGNORE INTO retrieval_eligible VALUES (?1)")?;
        for chunk_id in eligible {
            insert.execute([chunk_id])?;
        }
        Ok(())
    }

    fn verify_persisted_projection(&self) -> RetrievalResult<()> {
        let chunks = self.list_chunks()?;
        let source_bindings = validate_persisted_chunks(&chunks)?;
        if chunks.len() != self.manifest.chunk_count as usize {
            return Err(RetrievalError::ProjectionMismatch(
                "manifest chunk count differs from SQLite".to_owned(),
            ));
        }
        let mut source_statement = self.database.prepare(
            "SELECT source_id, source_path, source_digest FROM sources ORDER BY source_id",
        )?;
        let mut source_rows = source_statement.query([])?;
        let mut stored_source_count = 0_usize;
        while let Some(row) = source_rows.next()? {
            let source_id: String = row.get(0)?;
            let expected = source_bindings.get(&source_id).ok_or_else(|| {
                RetrievalError::ProjectionMismatch(
                    "source table contains an unbound identity".to_owned(),
                )
            })?;
            if row.get::<_, String>(1)? != expected.0 || row.get::<_, String>(2)? != expected.1 {
                return Err(RetrievalError::ProjectionMismatch(
                    "source table path or digest binding differs from its chunks".to_owned(),
                ));
            }
            stored_source_count += 1;
        }
        if stored_source_count != self.manifest.source_count as usize
            || source_bindings.len() != stored_source_count
        {
            return Err(RetrievalError::ProjectionMismatch(
                "manifest source count differs from SQLite".to_owned(),
            ));
        }
        self.verify_fts_projection(&chunks)?;
        let vectors = self.stored_vectors()?;
        let envelope = ProjectionDigestEnvelope {
            contract_version: &self.manifest.contract_version,
            projection_schema_version: self.manifest.projection_schema_version,
            engine_version: &self.manifest.engine_version,
            vault_id: &self.manifest.vault_id,
            source_snapshot_digest: &self.manifest.source_snapshot_digest,
            configuration_digest: &self.manifest.configuration_digest,
            policy_digest: &self.manifest.policy_digest,
            chunker_version: &self.manifest.chunker_version,
            tokenizer_version: &self.manifest.tokenizer_version,
            lexical_backend: self.manifest.lexical_backend,
            embedding_provider_id: &self.manifest.embedding_provider_id,
            embedding_model_id: &self.manifest.embedding_model_id,
            embedding_dimensions: self.manifest.embedding_dimensions,
            source_count: self.manifest.source_count,
            chunk_count: self.manifest.chunk_count,
            chunks: &chunks,
            vectors: &vectors,
        };
        if canonical_digest(&envelope)? != self.manifest.projection_digest {
            return Err(RetrievalError::ProjectionMismatch(
                "persisted projection digest mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn stored_vectors(&self) -> RetrievalResult<Vec<StoredVector>> {
        let mut statement = self
            .database
            .prepare("SELECT chunk_id, provider_id, model_id, dimensions, vector_json FROM chunk_vectors ORDER BY chunk_id")?;
        let mut rows = statement.query([])?;
        let mut vectors = Vec::new();
        while let Some(row) = rows.next()? {
            let provider: String = row.get(1)?;
            let model: String = row.get(2)?;
            let dimensions: u32 = row.get(3)?;
            let vector: Vec<f64> = serde_json::from_str(&row.get::<_, String>(4)?)?;
            if self.manifest.embedding_provider_id.as_deref() != Some(&provider)
                || self.manifest.embedding_model_id.as_deref() != Some(&model)
                || self.manifest.embedding_dimensions != Some(dimensions)
                || vector.len() != dimensions as usize
                || vector.iter().any(|value| !value.is_finite())
            {
                return Err(RetrievalError::ProjectionMismatch(
                    "stored vector identity is invalid".to_owned(),
                ));
            }
            vectors.push(StoredVector {
                chunk_id: row.get(0)?,
                vector,
            });
        }
        if self.manifest.embedding_provider_id.is_some() {
            if vectors.len() != self.manifest.chunk_count as usize {
                return Err(RetrievalError::ProjectionMismatch(
                    "vector generation is partial".to_owned(),
                ));
            }
        } else if !vectors.is_empty() {
            return Err(RetrievalError::ProjectionMismatch(
                "vectors exist without a manifest identity".to_owned(),
            ));
        }
        Ok(vectors)
    }

    fn verify_fts_projection(&self, chunks: &[RetrievalChunk]) -> RetrievalResult<()> {
        let chunks = chunks
            .iter()
            .map(|chunk| (chunk.chunk_id.as_str(), chunk))
            .collect::<BTreeMap<_, _>>();
        let mut statement = self.database.prepare(
            "SELECT chunk_id, title, heading_path, tags, topic, category, text FROM chunk_fts ORDER BY chunk_id",
        )?;
        let mut rows = statement.query([])?;
        let mut count = 0_usize;
        while let Some(row) = rows.next()? {
            let chunk_id: String = row.get(0)?;
            let chunk = chunks.get(chunk_id.as_str()).copied().ok_or_else(|| {
                RetrievalError::ProjectionMismatch("FTS row has no bound chunk".to_owned())
            })?;
            let expected_tags = chunk
                .metadata
                .tags
                .as_ref()
                .map_or_else(String::new, |tags| tags.join(" "));
            if row.get::<_, String>(1)? != chunk.metadata.title.as_deref().unwrap_or("")
                || row.get::<_, String>(2)? != chunk.heading_path.join(" / ")
                || row.get::<_, String>(3)? != expected_tags
                || row.get::<_, String>(4)? != chunk.metadata.topic.as_deref().unwrap_or("")
                || row.get::<_, String>(5)? != chunk.metadata.category.as_deref().unwrap_or("")
                || row.get::<_, String>(6)? != chunk.text
            {
                return Err(RetrievalError::ProjectionMismatch(
                    "FTS projection differs from its canonical chunk".to_owned(),
                ));
            }
            count += 1;
        }
        if count != chunks.len() {
            return Err(RetrievalError::ProjectionMismatch(
                "FTS projection is incomplete".to_owned(),
            ));
        }
        Ok(())
    }
}

fn sqlite_integer(value: u64, field: &str) -> RetrievalResult<i64> {
    i64::try_from(value).map_err(|_| {
        RetrievalError::InvalidEnvelope(format!("{field} exceeds SQLite's signed integer range"))
    })
}

#[derive(Serialize)]
struct SourceInvariant<'a> {
    source_path: &'a str,
    source_digest: &'a str,
    lineage_id: &'a Option<String>,
    valid_from: &'a Option<String>,
    valid_to: &'a Option<String>,
    supersedes: &'a [String],
    superseded_by: &'a [String],
    metadata: &'a RetrievalChunkMetadata,
}

fn validate_persisted_chunks(
    chunks: &[RetrievalChunk],
) -> RetrievalResult<BTreeMap<String, (String, String)>> {
    let mut ids = BTreeSet::new();
    let mut source_invariants = BTreeMap::<String, String>::new();
    let mut source_bindings = BTreeMap::<String, (String, String)>::new();
    let mut ordinals_by_source = BTreeMap::<&str, BTreeSet<u32>>::new();
    let mut parts_by_position = BTreeMap::<(&str, &str), BTreeSet<u32>>::new();
    let mut first_by_position = BTreeMap::<(&str, &str), &RetrievalChunk>::new();
    for chunk in chunks {
        if !is_valid_authored_uid(&chunk.source_id)
            || !is_normalized_relative_path(&chunk.source_path)
            || !is_sha256_digest(&chunk.source_digest)
            || !is_sha256_digest(&chunk.content_digest)
            || !ids.insert(&chunk.chunk_id)
            || chunk.heading_depth > 6
            || chunk.ordinal_within_source == 0
            || chunk.part_ordinal == 0
            || chunk.start_byte >= chunk.end_byte
            || chunk.start_line == 0
            || chunk.end_line < chunk.start_line
            || chunk.structural_position.is_empty()
            || chunk.end_byte.saturating_sub(chunk.start_byte) != chunk.text.len() as u64
            || chunk.text.len() > MAX_CHUNK_BYTES
            || chunk.content_digest != sha256(chunk.text.as_bytes())
            || chunk.token_count as usize != token_count(&chunk.text)
            || chunk.chunk_id
                != chunk_identity(
                    &chunk.source_id,
                    &chunk.structural_position,
                    chunk.part_ordinal,
                    &chunk.content_digest,
                )?
        {
            return Err(RetrievalError::ProjectionMismatch(
                "persisted chunk identity or content binding is invalid".to_owned(),
            ));
        }
        canonical_digest(&chunk.metadata)?;
        let invariant = canonical_json(&SourceInvariant {
            source_path: &chunk.source_path,
            source_digest: &chunk.source_digest,
            lineage_id: &chunk.lineage_id,
            valid_from: &chunk.valid_from,
            valid_to: &chunk.valid_to,
            supersedes: &chunk.supersedes,
            superseded_by: &chunk.superseded_by,
            metadata: &chunk.metadata,
        })?;
        if source_invariants
            .insert(chunk.source_id.clone(), invariant.clone())
            .is_some_and(|prior| prior != invariant)
        {
            return Err(RetrievalError::ProjectionMismatch(
                "chunks for one source disagree on metadata, lineage, or temporal binding"
                    .to_owned(),
            ));
        }
        source_bindings.insert(
            chunk.source_id.clone(),
            (chunk.source_path.clone(), chunk.source_digest.clone()),
        );
        ordinals_by_source
            .entry(&chunk.source_id)
            .or_default()
            .insert(chunk.ordinal_within_source);
        parts_by_position
            .entry((&chunk.source_id, &chunk.structural_position))
            .or_default()
            .insert(chunk.part_ordinal);
        if chunk.part_ordinal == 1
            && first_by_position
                .insert((&chunk.source_id, &chunk.structural_position), chunk)
                .is_some()
        {
            return Err(RetrievalError::ProjectionMismatch(
                "persisted structural position has more than one first chunk".to_owned(),
            ));
        }
    }
    for ordinals in ordinals_by_source.values() {
        if ordinals
            .iter()
            .copied()
            .ne(1..=u32::try_from(ordinals.len()).map_err(|_| {
                RetrievalError::ProjectionMismatch(
                    "persisted source has too many chunks".to_owned(),
                )
            })?)
        {
            return Err(RetrievalError::ProjectionMismatch(
                "persisted source ordinals are not contiguous and one-based".to_owned(),
            ));
        }
    }
    for parts in parts_by_position.values() {
        if parts
            .iter()
            .copied()
            .ne(1..=u32::try_from(parts.len()).map_err(|_| {
                RetrievalError::ProjectionMismatch(
                    "persisted structural position has too many chunks".to_owned(),
                )
            })?)
        {
            return Err(RetrievalError::ProjectionMismatch(
                "persisted part ordinals are not contiguous and one-based".to_owned(),
            ));
        }
    }
    for chunk in chunks {
        let parent_position = chunk
            .structural_position
            .rsplit_once('.')
            .map(|(parent, _)| parent);
        match (parent_position, chunk.parent_chunk_id.as_deref()) {
            (None, None) => {}
            (Some(parent_position), Some(parent_id)) => {
                let expected = first_by_position.get(&(chunk.source_id.as_str(), parent_position));
                if !expected.is_some_and(|parent| {
                    parent.chunk_id == parent_id
                        && parent.part_ordinal == 1
                        && parent.source_id == chunk.source_id
                }) {
                    return Err(RetrievalError::ProjectionMismatch(
                        "persisted parent must be the nearest structural ancestor's first chunk"
                            .to_owned(),
                    ));
                }
            }
            _ => {
                return Err(RetrievalError::ProjectionMismatch(
                    "persisted parent does not match structural ancestry".to_owned(),
                ));
            }
        }
    }
    Ok(source_bindings)
}

fn fts_expression(query: &str) -> RetrievalResult<String> {
    Ok(lexical_query_clauses(query)?
        .into_iter()
        .map(|clause| format!("\"{}\"", clause.value))
        .collect::<Vec<_>>()
        .join(" AND "))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LexicalQueryClause {
    pub(crate) value: String,
    pub(crate) tokens: Vec<String>,
    pub(crate) quoted: bool,
}

pub(crate) fn lexical_query_clauses(query: &str) -> RetrievalResult<Vec<LexicalQueryClause>> {
    if query
        .chars()
        .any(|character| matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}'))
    {
        return Err(RetrievalError::InvalidConfig(
            "RETRIEVAL_QUERY_LEXICAL_INVALID:control".to_owned(),
        ));
    }
    let mut clauses = Vec::new();
    let mut offset = 0_usize;
    while offset < query.len() {
        while offset < query.len() {
            let character = query[offset..]
                .chars()
                .next()
                .expect("offset is inside the query");
            if !is_ecmascript_whitespace(character) {
                break;
            }
            offset += character.len_utf8();
        }
        if offset >= query.len() {
            break;
        }
        let quoted = query[offset..].starts_with('"');
        let value = if quoted {
            let content_start = offset + 1;
            let relative_close = query[content_start..].find('"').ok_or_else(|| {
                RetrievalError::InvalidConfig(
                    "RETRIEVAL_QUERY_LEXICAL_INVALID:unmatched_quote".to_owned(),
                )
            })?;
            let close = content_start + relative_close;
            let value = query[content_start..close].to_owned();
            offset = close + 1;
            if offset < query.len()
                && !is_ecmascript_whitespace(
                    query[offset..]
                        .chars()
                        .next()
                        .expect("offset is inside the query"),
                )
            {
                return Err(RetrievalError::InvalidConfig(
                    "RETRIEVAL_QUERY_LEXICAL_INVALID:quote_boundary".to_owned(),
                ));
            }
            value
        } else {
            let start = offset;
            while offset < query.len() {
                let character = query[offset..]
                    .chars()
                    .next()
                    .expect("offset is inside the query");
                if is_ecmascript_whitespace(character) {
                    break;
                }
                offset += character.len_utf8();
            }
            let value = query[start..offset].to_owned();
            if value.contains('"') {
                return Err(RetrievalError::InvalidConfig(
                    "RETRIEVAL_QUERY_LEXICAL_INVALID:quote_boundary".to_owned(),
                ));
            }
            value
        };
        let tokens = query_terms(&value);
        if value.is_empty() || tokens.is_empty() {
            return Err(RetrievalError::InvalidConfig(
                "RETRIEVAL_QUERY_LEXICAL_INVALID:empty_clause".to_owned(),
            ));
        }
        clauses.push(LexicalQueryClause {
            value,
            tokens,
            quoted,
        });
    }
    if clauses.is_empty() {
        return Err(RetrievalError::InvalidConfig(
            "RETRIEVAL_QUERY_LEXICAL_INVALID:empty_query".to_owned(),
        ));
    }
    Ok(clauses)
}

pub(crate) fn trim_ecmascript_whitespace(value: &str) -> &str {
    value.trim_matches(is_ecmascript_whitespace)
}

fn is_ecmascript_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in normalize_term(query).chars() {
        if is_lexical_token_character(character) {
            current.push(character);
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

pub fn normalized_lexical_terms(query: &str) -> Vec<String> {
    query_terms(query)
}

fn weighted_lexical_score(query: &str, fields: &[(String, f64)]) -> f64 {
    query_terms(query)
        .iter()
        .map(|term| {
            fields
                .iter()
                .map(|(field, boost)| occurrences(field, term) as f64 * boost)
                .sum::<f64>()
        })
        .sum::<f64>()
}

#[allow(clippy::too_many_arguments)]
pub fn lexical_field_score(
    query: &str,
    title: &str,
    heading_path: &str,
    tags: &str,
    topic: &str,
    category: &str,
    text: &str,
    token_count: u32,
) -> f64 {
    weighted_lexical_score(
        query,
        &[
            (title.to_owned(), 3.0),
            (heading_path.to_owned(), 2.0),
            (tags.to_owned(), 1.5),
            (topic.to_owned(), 2.0),
            (category.to_owned(), 2.0),
            (text.to_owned(), 1.0),
        ],
    ) / f64::from(token_count.saturating_add(1)).sqrt()
}

fn normalize_term(value: &str) -> String {
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

fn is_lexical_token_character(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(
            character as u32,
            0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
        )
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    let normalized = normalize_term(haystack);
    let mut count = 0;
    let mut offset = 0;
    while let Some(found) = normalized[offset..].find(needle) {
        count += 1;
        offset += found + needle.len().max(1);
    }
    count
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, DiscoverabilityDecision,
        RetrievalChunkMetadata,
    };

    use super::*;

    fn source(id: &str, path: &str, text: &str, tag: &str) -> RetrievalSource {
        RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: id.to_owned(),
            source_path: path.to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata {
                title: Some("Agent policy".to_owned()),
                tags: Some(vec![tag.to_owned()]),
                ..RetrievalChunkMetadata::default()
            },
        }
    }

    #[derive(serde::Deserialize)]
    struct LexicalConformanceRoot {
        lexical_backends: LexicalConformanceBackends,
    }

    #[derive(serde::Deserialize)]
    struct LexicalConformanceBackends {
        differential_rows: Vec<LexicalConformanceRow>,
        differential_queries: Vec<LexicalConformanceQuery>,
        rejected_queries: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct LexicalConformanceRow {
        chunk_id: String,
        title: String,
        heading_path: String,
        tags: String,
        topic: String,
        category: String,
        text: String,
        token_count: u32,
    }

    #[derive(serde::Deserialize)]
    struct LexicalConformanceQuery {
        query: String,
        expected: Vec<LexicalConformanceExpected>,
    }

    #[derive(serde::Deserialize)]
    struct LexicalConformanceExpected {
        chunk_id: String,
        score: f64,
    }

    fn generation(state_directory: PathBuf) -> RetrievalGenerationInput {
        RetrievalGenerationInput {
            state_directory,
            engine_version: "lite-phase1".to_owned(),
            vault_id: "vault-a".to_owned(),
            source_snapshot_digest: sha256(b"snapshot"),
            configuration_digest: sha256(b"configuration"),
            policy_digest: sha256(b"policy"),
            sources: vec![
                source(
                    "019b2d14-4230-7db7-87d4-7d81cfaeca01",
                    "policy/agent.md",
                    "# Writing\nAgents must cite canonical policy.",
                    "policy",
                ),
                source(
                    "019b2d14-4230-7db7-87d4-7d81cfaeca02",
                    "notes/other.md",
                    "# Other\nUnrelated material.",
                    "other",
                ),
            ],
            chunking: ChunkingOptions::default(),
            vectors: Vec::new(),
            embedding_provider_id: None,
            embedding_model_id: None,
            embedding_dimensions: None,
        }
    }

    fn hierarchical_generation(
        state_directory: PathBuf,
        text: &str,
        chunking: ChunkingOptions,
    ) -> (RetrievalGenerationInput, Vec<RetrievalChunk>) {
        let record = source(
            "019b2d14-4230-7db7-87d4-7d81cfaeca03",
            "notes/hierarchy.md",
            text,
            "hierarchy",
        );
        let chunks = chunk_source(&record, chunking).unwrap();
        let mut input = generation(state_directory);
        input.sources = vec![record];
        input.chunking = chunking;
        (input, chunks)
    }

    fn tamper_parent(database_path: &Path, chunk_id: &str, parent_id: Option<&str>) {
        let database = Connection::open(database_path).unwrap();
        database
            .execute(
                "UPDATE chunks SET parent_chunk_id = ?1 WHERE chunk_id = ?2",
                params![parent_id, chunk_id],
            )
            .unwrap();
    }

    fn assert_persisted_parent_rejected(database_path: &Path) {
        assert!(matches!(
            SqliteRetrievalStore::open(database_path),
            Err(RetrievalError::ProjectionMismatch(message))
                if message.contains("persisted parent")
        ));
    }

    #[test]
    fn bundled_sqlite_proves_fts5_at_runtime_and_searches_without_vectors() {
        let directory = tempdir().unwrap();
        let built = build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        ensure_fts5(&store.database).unwrap();
        let temp_store: i64 = store
            .database
            .pragma_query_value(None, "temp_store", |row| row.get(0))
            .unwrap();
        assert_eq!(temp_store, 2);
        assert_eq!(store.count_chunks().unwrap(), 2);
        let eligible = store
            .list_chunks()
            .unwrap()
            .into_iter()
            .filter(|chunk| {
                chunk
                    .metadata
                    .tags
                    .as_ref()
                    .is_some_and(|tags| tags.iter().any(|tag| tag == "policy"))
            })
            .map(|chunk| chunk.chunk_id)
            .collect::<Vec<_>>();
        let results = store
            .lexical_search_eligible("canonical policy", &eligible, 5)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_id, "019b2d14-4230-7db7-87d4-7d81cfaeca01");
        assert!(store.manifest.embedding_provider_id.is_none());
    }

    #[test]
    fn vector_sql_materializes_only_coordinator_eligible_rows() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        let chunks = input
            .sources
            .iter()
            .flat_map(|source| chunk_source(source, input.chunking).unwrap())
            .collect::<Vec<_>>();
        let eligible_source = "019b2d14-4230-7db7-87d4-7d81cfaeca02";
        input.vectors = chunks
            .iter()
            .map(|chunk| StoredVector {
                chunk_id: chunk.chunk_id.clone(),
                vector: if chunk.source_id == eligible_source {
                    vec![0.0, 1.0]
                } else {
                    vec![1.0, 0.0]
                },
            })
            .collect();
        input.embedding_provider_id = Some("provider-a".to_owned());
        input.embedding_model_id = Some("model-a".to_owned());
        input.embedding_dimensions = Some(2);
        let eligible_chunk_id = chunks
            .iter()
            .find(|chunk| chunk.source_id == eligible_source)
            .unwrap()
            .chunk_id
            .clone();
        let built = build_retrieval_generation(input).unwrap();
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        let results = store
            .vector_search(
                &[1.0, 0.0],
                &BTreeSet::from([eligible_chunk_id.clone()]),
                5,
                "provider-a",
                "model-a",
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, eligible_chunk_id);
        assert_eq!(results[0].source_id, eligible_source);
        assert_eq!(results[0].score, 0.0);
        assert!(VECTOR_ELIGIBLE_SQL.contains("JOIN retrieval_eligible"));
    }

    #[test]
    fn projection_digest_matches_the_full_reference_envelope() {
        let directory = tempdir().unwrap();
        let input = RetrievalGenerationInput {
            state_directory: directory.path().to_path_buf(),
            engine_version: "2.1.2".to_owned(),
            vault_id: "vault-a".to_owned(),
            source_snapshot_digest: sha256(b"snapshot"),
            configuration_digest: sha256(b"configuration"),
            policy_digest: sha256(b"policy"),
            sources: vec![source(
                "019b2d14-4230-7db7-87d4-7d81cfaeca01",
                "policy/agent.md",
                "# Writing\nAgents must cite canonical policy.",
                "policy",
            )],
            chunking: ChunkingOptions::default(),
            vectors: Vec::new(),
            embedding_provider_id: None,
            embedding_model_id: None,
            embedding_dimensions: None,
        };
        let built = build_retrieval_generation(input).unwrap();
        assert_eq!(
            built.manifest.projection_digest,
            "sha256:ea11fa3ca95ec89446ebfd867f5bf77e3597b50755609065a7de52de8a57e159"
        );
    }

    #[test]
    fn source_count_includes_only_sources_represented_by_chunks() {
        let directory = tempdir().unwrap();
        let mut mixed = generation(directory.path().to_path_buf());
        mixed.sources[1] = source(
            "019b2d14-4230-7db7-87d4-7d81cfaeca02",
            "notes/frontmatter-only.md",
            "---\ntitle: No body\n---\n",
            "blank",
        );
        let built = build_retrieval_generation(mixed).unwrap();
        assert_eq!(built.manifest.source_count, 1);
        assert_eq!(built.manifest.chunk_count, 1);
        let store = SqliteRetrievalStore::open(&built.database_path).unwrap();
        assert_eq!(store.count_chunks().unwrap(), 1);

        let all_blank_directory = tempdir().unwrap();
        let mut all_blank = generation(all_blank_directory.path().to_path_buf());
        all_blank.sources = vec![
            source(
                "019b2d14-4230-7db7-87d4-7d81cfaeca01",
                "notes/blank.md",
                "   ",
                "blank",
            ),
            source(
                "019b2d14-4230-7db7-87d4-7d81cfaeca02",
                "notes/frontmatter-only.md",
                "---\ntitle: No body\n---\n",
                "blank",
            ),
        ];
        let empty = build_retrieval_generation(all_blank).unwrap();
        assert_eq!(empty.manifest.source_count, 0);
        assert_eq!(empty.manifest.chunk_count, 0);
        let empty_store = SqliteRetrievalStore::open(&empty.database_path).unwrap();
        assert_eq!(empty_store.count_chunks().unwrap(), 0);
    }

    #[test]
    fn persisted_parent_binding_is_exact_and_survives_skipped_heading_depths() {
        let directory = tempdir().unwrap();

        let sibling_text = "# One\nA\n## Child\nB\n# Two\nC\n";
        let (sibling_input, sibling_chunks) = hierarchical_generation(
            directory.path().join("sibling"),
            sibling_text,
            ChunkingOptions::default(),
        );
        let sibling_child = sibling_chunks
            .iter()
            .find(|chunk| {
                chunk
                    .heading_path
                    .last()
                    .is_some_and(|name| name == "Child")
            })
            .unwrap();
        let wrong_sibling = sibling_chunks
            .iter()
            .find(|chunk| chunk.heading_path.last().is_some_and(|name| name == "Two"))
            .unwrap();
        let sibling_built = build_retrieval_generation(sibling_input).unwrap();
        tamper_parent(
            &sibling_built.database_path,
            &sibling_child.chunk_id,
            Some(&wrong_sibling.chunk_id),
        );
        assert_persisted_parent_rejected(&sibling_built.database_path);

        let (missing_input, missing_chunks) = hierarchical_generation(
            directory.path().join("missing"),
            sibling_text,
            ChunkingOptions::default(),
        );
        let missing_child = missing_chunks
            .iter()
            .find(|chunk| {
                chunk
                    .heading_path
                    .last()
                    .is_some_and(|name| name == "Child")
            })
            .unwrap();
        let missing_built = build_retrieval_generation(missing_input).unwrap();
        tamper_parent(&missing_built.database_path, &missing_child.chunk_id, None);
        assert_persisted_parent_rejected(&missing_built.database_path);

        let extra_text = "Preamble\n# One\nA\n";
        let (extra_input, extra_chunks) = hierarchical_generation(
            directory.path().join("extra"),
            extra_text,
            ChunkingOptions::default(),
        );
        let root = extra_chunks
            .iter()
            .find(|chunk| chunk.structural_position == "root")
            .unwrap();
        let top_level = extra_chunks
            .iter()
            .find(|chunk| chunk.heading_path.last().is_some_and(|name| name == "One"))
            .unwrap();
        let extra_built = build_retrieval_generation(extra_input).unwrap();
        tamper_parent(
            &extra_built.database_path,
            &top_level.chunk_id,
            Some(&root.chunk_id),
        );
        assert_persisted_parent_rejected(&extra_built.database_path);

        let long_parent = format!(
            "# One\n{}\n## Child\nB\n",
            (0..40)
                .map(|index| format!("t{index}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let chunking = ChunkingOptions {
            max_tokens: 16,
            overlap_tokens: 0,
        };
        let (non_first_input, non_first_chunks) =
            hierarchical_generation(directory.path().join("non-first"), &long_parent, chunking);
        let non_first_child = non_first_chunks
            .iter()
            .find(|chunk| {
                chunk
                    .heading_path
                    .last()
                    .is_some_and(|name| name == "Child")
            })
            .unwrap();
        let second_parent_part = non_first_chunks
            .iter()
            .find(|chunk| chunk.heading_path == ["One"] && chunk.part_ordinal == 2)
            .unwrap();
        let non_first_built = build_retrieval_generation(non_first_input).unwrap();
        tamper_parent(
            &non_first_built.database_path,
            &non_first_child.chunk_id,
            Some(&second_parent_part.chunk_id),
        );
        assert_persisted_parent_rejected(&non_first_built.database_path);

        let (skipped_input, _) = hierarchical_generation(
            directory.path().join("skipped-depth"),
            "# One\nA\n### Child\nB\n",
            ChunkingOptions::default(),
        );
        let skipped_built = build_retrieval_generation(skipped_input).unwrap();
        SqliteRetrievalStore::open(&skipped_built.database_path).unwrap();
    }

    #[test]
    fn fts_query_parser_matches_full_for_phrases_punctuation_and_unclosed_quotes() {
        assert_eq!(
            fts_expression(r#""two words" plain"#).unwrap(),
            r#""two words" AND "plain""#
        );
        for invalid in [
            r#""unclosed"#,
            "!!!",
            r#"alpha"beta"#,
            "alpha\0beta",
            "\talpha",
        ] {
            assert!(
                fts_expression(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("RETRIEVAL_QUERY_LEXICAL_INVALID"),
                "{invalid:?}"
            );
        }
        assert_eq!(
            lexical_query_clauses("alpha\u{0085}beta")
                .unwrap()
                .into_iter()
                .map(|clause| clause.value)
                .collect::<Vec<_>>(),
            ["alpha\u{0085}beta"]
        );
        assert_eq!(
            lexical_query_clauses("alpha\u{feff}beta")
                .unwrap()
                .into_iter()
                .map(|clause| clause.value)
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        assert_eq!(trim_ecmascript_whitespace("\u{feff}alpha\u{feff}"), "alpha");
        assert_eq!(
            trim_ecmascript_whitespace("\u{0085}alpha\u{0085}"),
            "\u{0085}alpha\u{0085}"
        );
    }

    #[test]
    fn bundled_fts5_candidate_selection_matches_full_differential_fixture() {
        let fixture: LexicalConformanceRoot = serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
            "conformance-fixture.json"
        )))
        .unwrap();
        let database = Connection::open_in_memory().unwrap();
        ensure_fts5(&database).unwrap();
        database.execute_batch(MIGRATION).unwrap();
        for row in &fixture.lexical_backends.differential_rows {
            database
                .execute(
                    "INSERT INTO chunk_fts(chunk_id,title,heading_path,tags,topic,category,text) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![row.chunk_id, row.title, row.heading_path, row.tags, row.topic, row.category, row.text],
                )
                .unwrap();
        }
        for query in &fixture.lexical_backends.differential_queries {
            let expression = fts_expression(&query.query).unwrap();
            let mut statement = database
                .prepare("SELECT chunk_id FROM chunk_fts WHERE chunk_fts MATCH ?1")
                .unwrap();
            let candidate_ids = statement
                .query_map([expression], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let mut actual = candidate_ids
                .into_iter()
                .map(|chunk_id| {
                    let row = fixture
                        .lexical_backends
                        .differential_rows
                        .iter()
                        .find(|row| row.chunk_id == chunk_id)
                        .unwrap();
                    let score = lexical_field_score(
                        &query.query,
                        &row.title,
                        &row.heading_path,
                        &row.tags,
                        &row.topic,
                        &row.category,
                        &row.text,
                        row.token_count,
                    );
                    (chunk_id, score)
                })
                .collect::<Vec<_>>();
            actual.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| code_unit_compare(&left.0, &right.0))
            });
            assert_eq!(
                actual,
                query
                    .expected
                    .iter()
                    .map(|item| (item.chunk_id.clone(), item.score))
                    .collect::<Vec<_>>(),
                "{}",
                query.query
            );
        }
        for query in fixture.lexical_backends.rejected_queries {
            assert!(fts_expression(&query).is_err(), "{query:?}");
        }
    }

    #[test]
    fn identical_inputs_reuse_identical_immutable_generation() {
        let directory = tempdir().unwrap();
        let first = build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        let second =
            build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        assert_eq!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn reused_generation_and_state_directory_are_rehardened_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let first = build_retrieval_generation(input.clone()).unwrap();
        fs::set_permissions(&first.database_path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
        build_retrieval_generation(input).unwrap();
        assert_eq!(
            fs::metadata(&first.database_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn active_pointer_atomically_round_trips_only_the_verified_generation() {
        let directory = tempdir().unwrap();
        let first = build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        activate_retrieval_generation(directory.path(), &first).unwrap();
        let active = open_active_retrieval_generation(directory.path()).unwrap();
        assert_eq!(active.manifest, first.manifest);
        drop(active);

        let mut changed = generation(directory.path().to_path_buf());
        changed.source_snapshot_digest = sha256(b"new-snapshot");
        let second = build_retrieval_generation(changed).unwrap();
        activate_retrieval_generation(directory.path(), &second).unwrap();
        assert_eq!(
            open_active_retrieval_generation(directory.path())
                .unwrap()
                .manifest,
            second.manifest
        );
        assert!(first.database_path.exists());
    }

    #[test]
    fn corrupt_generation_and_sqlite_sidecars_are_quarantined_before_rebuild() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let first = build_retrieval_generation(input.clone()).unwrap();
        fs::write(&first.database_path, b"not a SQLite database").unwrap();
        fs::write(sidecar_path(&first.database_path, "-wal"), b"stale wal").unwrap();
        fs::write(sidecar_path(&first.database_path, "-shm"), b"stale shm").unwrap();

        let rebuilt = build_retrieval_generation(input).unwrap();
        assert_eq!(rebuilt.manifest, first.manifest);
        SqliteRetrievalStore::open(&rebuilt.database_path).unwrap();
        let quarantined = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".quarantine-")
            })
            .count();
        assert_eq!(quarantined, 3);
    }

    #[test]
    fn orphan_sidecars_without_main_database_are_quarantined_before_final_rename() {
        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let first = build_retrieval_generation(input.clone()).unwrap();
        fs::remove_file(&first.database_path).unwrap();
        fs::write(sidecar_path(&first.database_path, "-wal"), b"orphan wal").unwrap();
        fs::write(sidecar_path(&first.database_path, "-shm"), b"orphan shm").unwrap();

        let rebuilt = build_retrieval_generation(input).unwrap();
        assert_eq!(rebuilt.manifest, first.manifest);
        SqliteRetrievalStore::open(&rebuilt.database_path).unwrap();
        let quarantine_names = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".quarantine-"))
            .collect::<Vec<_>>();
        assert_eq!(quarantine_names.len(), 2);
        assert!(quarantine_names.iter().any(|name| name.contains("-wal")));
        assert!(quarantine_names.iter().any(|name| name.contains("-shm")));
    }

    #[test]
    fn direct_open_rejects_any_sidecar_for_an_immutable_generation() {
        let directory = tempdir().unwrap();
        let built = build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        fs::write(
            sidecar_path(&built.database_path, "-wal"),
            b"unexpected wal",
        )
        .unwrap();
        assert!(matches!(
            SqliteRetrievalStore::open(&built.database_path),
            Err(RetrievalError::ProjectionMismatch(message))
                if message.contains("SQLite sidecar")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn broken_orphan_sidecar_symlink_is_detected_and_quarantined() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let input = generation(directory.path().to_path_buf());
        let first = build_retrieval_generation(input.clone()).unwrap();
        fs::remove_file(&first.database_path).unwrap();
        symlink(
            directory.path().join("missing-wal-target"),
            sidecar_path(&first.database_path, "-wal"),
        )
        .unwrap();
        let rebuilt = build_retrieval_generation(input).unwrap();
        SqliteRetrievalStore::open(&rebuilt.database_path).unwrap();
        assert!(fs::read_dir(directory.path()).unwrap().any(|entry| {
            entry
                .ok()
                .is_some_and(|entry| entry.file_name().to_string_lossy().contains("-wal-"))
        }));
    }

    #[test]
    fn active_pointer_size_is_bounded_before_json_allocation() {
        let directory = tempdir().unwrap();
        let built = build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        activate_retrieval_generation(directory.path(), &built).unwrap();
        fs::write(
            directory.path().join(ACTIVE_POINTER_NAME),
            vec![b' '; 1_048_577],
        )
        .unwrap();
        assert!(matches!(
            open_active_retrieval_generation(directory.path()),
            Err(RetrievalError::ProjectionMismatch(message))
                if message.contains("exceeds one MiB")
        ));
    }

    #[test]
    fn hard_linked_active_pointer_is_rejected_as_an_alias() {
        let directory = tempdir().unwrap();
        let built = build_retrieval_generation(generation(directory.path().to_path_buf())).unwrap();
        activate_retrieval_generation(directory.path(), &built).unwrap();
        fs::hard_link(
            directory.path().join(ACTIVE_POINTER_NAME),
            directory.path().join("pointer-alias.json"),
        )
        .unwrap();
        assert!(matches!(
            open_active_retrieval_generation(directory.path()),
            Err(RetrievalError::InvalidConfig(message))
                if message.contains("non-aliased")
        ));
    }

    #[test]
    fn partial_or_unidentified_vectors_fail_before_publication() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        input.vectors.push(StoredVector {
            chunk_id: "not-a-chunk".to_owned(),
            vector: vec![1.0, 0.0],
        });
        assert!(build_retrieval_generation(input).is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn duplicate_source_identity_with_mixed_policy_cannot_publish() {
        let directory = tempdir().unwrap();
        let mut input = generation(directory.path().to_path_buf());
        let mut duplicate = input.sources[0].clone();
        duplicate.discoverability = DiscoverabilityDecision::Deny;
        input.sources.push(duplicate);
        assert!(build_retrieval_generation(input).is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
