//! Private cross-runtime serialization for legacy retrieval writers.
//!
//! The byte format and controlled filenames mirror Full's package-private
//! `retrieval-writer.lock` protocol. This module does not implement the
//! Phase-3 ingest authority; it only prevents the frozen Phase-1/2 legacy
//! writers from racing or downgrading that authority.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{is_sha256_digest, RETRIEVAL_CONTRACT};
use crate::digest::{canonical_digest, canonical_json, sha256};
use crate::ingest::validate_retrieval_manifest;
use crate::path_security::{contained_path, equivalent_paths};
use crate::sqlite_store::{
    atomic_replace, harden_directory_permissions, harden_file_permissions, path_entry_exists,
    sync_directory, validate_existing_state_directory, validate_state_directory, write_owner_file,
};
use crate::{RetrievalError, RetrievalResult};

pub(crate) const LEGACY_WRITER_LOCK_FILE: &str = "retrieval-writer.lock";
pub(crate) const LEGACY_WRITER_RECOVERY_FILE: &str = "retrieval-writer.recovery";
const LOCK_CONTRACT: &str = "gkos-retrieval-writer-lock/1.0.0-draft.1";
const MAX_LOCK_BYTES: u64 = 1_048_576;
const MAX_STATE_ENTRIES: usize = 100_000;
const PHASE3_TOMBSTONE_CONTRACT: &str = "gkos-ingest-legacy-pointer-tombstone/1.0.0-draft.1";
const AUTHORITY_EVIDENCE_NAMES: &[&str] = &[
    "ingest-authority.lock",
    "ingest-authority.recovery",
    "ingest-activation-root.json",
    "ingest-authority.json",
    "ingest-attempt-status.json",
    "active-ingest.json",
    "active-retrieval.json",
];

static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyWriterLock {
    contract_version: String,
    lock_id: String,
    process_id: u32,
    prior_pointer_digest: Option<String>,
    target_pointer_digest: Option<String>,
    lock_digest: String,
}

#[derive(Serialize)]
struct LockDigestMaterial<'a> {
    contract_version: &'a str,
    lock_id: &'a str,
    process_id: u32,
    prior_pointer_digest: &'a Option<String>,
    target_pointer_digest: &'a Option<String>,
}

#[derive(Debug)]
pub(crate) struct LegacyRetrievalWriterCapability {
    state_directory: PathBuf,
    lock: LegacyWriterLock,
    file_digest: String,
    remove_empty_directory_on_release: bool,
    held: bool,
}

impl LegacyRetrievalWriterCapability {
    pub(crate) fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    pub(crate) fn is_held(&self) -> bool {
        self.held
    }
}

impl Drop for LegacyRetrievalWriterCapability {
    fn drop(&mut self) {
        if self.held {
            // Cancellation and unwind must not strand a lock owned by this
            // exact capability. Ambiguous state deliberately retains it for
            // the explicit stale-recovery path.
            let _ = release_owned_writer_if_unambiguous(self);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity {
    length: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    platform: PlatformFileIdentity,
}

#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct PlatformFileIdentity {
    device: u64,
    inode: u64,
    links: u64,
    mode: u32,
    mtime_seconds: i64,
    mtime_nanoseconds: i64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
}

#[cfg(windows)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct PlatformFileIdentity {
    attributes: u32,
    creation_time: u64,
    last_write_time: u64,
    volume_serial_number: u32,
    number_of_links: u32,
    file_index: u64,
}

#[cfg(not(any(unix, windows)))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct PlatformFileIdentity;

fn invalid(message: impl Into<String>) -> RetrievalError {
    RetrievalError::InvalidConfig(message.into())
}

fn canonical_bytes<T: Serialize>(value: &T) -> RetrievalResult<Vec<u8>> {
    Ok(format!("{}\n", canonical_json(value)?).into_bytes())
}

fn lock_material(lock: &LegacyWriterLock) -> LockDigestMaterial<'_> {
    LockDigestMaterial {
        contract_version: &lock.contract_version,
        lock_id: &lock.lock_id,
        process_id: lock.process_id,
        prior_pointer_digest: &lock.prior_pointer_digest,
        target_pointer_digest: &lock.target_pointer_digest,
    }
}

fn seal_lock(lock: LegacyWriterLock) -> RetrievalResult<LegacyWriterLock> {
    if lock.contract_version != LOCK_CONTRACT
        || lock.process_id == 0
        || !is_sha256_digest(&lock.lock_id)
        || lock
            .prior_pointer_digest
            .as_deref()
            .is_some_and(|value| !is_sha256_digest(value))
        || lock
            .target_pointer_digest
            .as_deref()
            .is_some_and(|value| !is_sha256_digest(value))
        || !is_sha256_digest(&lock.lock_digest)
        || canonical_digest(&lock_material(&lock))? != lock.lock_digest
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_INVALID"));
    }
    Ok(lock)
}

fn fresh_digest(label: &str) -> String {
    let ordinal = NONCE.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    sha256(format!(
        "{label}\0{}\0{nanos}\0{ordinal}",
        std::process::id()
    ))
}

fn validate_state_path_namespace(path: &Path) -> RetrievalResult<()> {
    let value = path
        .as_os_str()
        .to_str()
        .ok_or_else(|| invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"))?;
    if value.is_empty()
        || value.encode_utf16().count() > 4_096
        || value
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
    {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
    }
    let lexical = value.replace('\\', "/");
    let extended_local = lexical
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("//?/"))
        && lexical.get(4..7).is_some_and(|prefix| {
            prefix.as_bytes()[0].is_ascii_alphabetic() && &prefix[1..] == ":/"
        });
    #[cfg(not(windows))]
    if extended_local {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
    }
    if !extended_local && (lexical.starts_with("//") || lexical.starts_with("/??/")) {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
    }
    #[cfg(windows)]
    let lexical = if extended_local {
        lexical[4..].to_owned()
    } else {
        lexical
    };
    let drive = lexical.len() >= 2
        && lexical.as_bytes()[0].is_ascii_alphabetic()
        && lexical.as_bytes()[1] == b':';
    if drive && !lexical.as_bytes().get(2).is_some_and(|byte| *byte == b'/') {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
    }
    let tail = if drive {
        &lexical[2..]
    } else {
        lexical.as_str()
    };
    if tail.contains(':') {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
    }
    for component in tail.split('/').filter(|component| !component.is_empty()) {
        if component.ends_with([' ', '.']) {
            return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
        }
        let stem = component
            .split('.')
            .next()
            .unwrap_or(component)
            .to_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        ) || matches_reserved_numbered_device(&stem, "COM")
            || matches_reserved_numbered_device(&stem, "LPT");
        if reserved {
            return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
        }
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_INVALID"));
    }
    Ok(())
}

fn matches_reserved_numbered_device(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata, _file: Option<&File>) -> RetrievalResult<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileIdentity {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        platform: PlatformFileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
            mode: metadata.mode(),
            mtime_seconds: metadata.mtime(),
            mtime_nanoseconds: metadata.mtime_nsec(),
            ctime_seconds: metadata.ctime(),
            ctime_nanoseconds: metadata.ctime_nsec(),
        },
    })
}

#[cfg(windows)]
fn file_identity(metadata: &fs::Metadata, file: Option<&File>) -> RetrievalResult<FileIdentity> {
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = file.ok_or_else(|| invalid("RETRIEVAL_STATE_WRITER_ALIAS_REJECTED"))?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the File owns a valid handle and the output remains live.
    if unsafe {
        GetFileInformationByHandle(
            file.as_raw_handle() as _,
            &mut information as *mut BY_HANDLE_FILE_INFORMATION,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(FileIdentity {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        platform: PlatformFileIdentity {
            attributes: metadata.file_attributes(),
            creation_time: metadata.creation_time(),
            last_write_time: metadata.last_write_time(),
            volume_serial_number: information.dwVolumeSerialNumber,
            number_of_links: information.nNumberOfLinks,
            file_index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        },
    })
}

#[cfg(not(any(unix, windows)))]
fn file_identity(metadata: &fs::Metadata, _file: Option<&File>) -> RetrievalResult<FileIdentity> {
    Ok(FileIdentity {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        platform: PlatformFileIdentity,
    })
}

fn require_owner_file(
    metadata: &fs::Metadata,
    _identity: &FileIdentity,
    allowed_links: u64,
) -> RetrievalResult<()> {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("RETRIEVAL_STATE_WRITER_ALIAS_REJECTED"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != allowed_links {
            return Err(invalid(if allowed_links == 1 && metadata.nlink() > 1 {
                "RETRIEVAL_STATE_HARDLINK_REJECTED"
            } else {
                "RETRIEVAL_STATE_WRITER_ALIAS_REJECTED"
            }));
        }
        if metadata.mode() & 0o777 != 0o600 {
            return Err(invalid("RETRIEVAL_STATE_WRITER_PERMISSION_REJECTED"));
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if _identity.platform.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(invalid("RETRIEVAL_STATE_WRITER_ALIAS_REJECTED"));
        }
        if _identity.platform.number_of_links != allowed_links as u32 {
            return Err(invalid(
                if allowed_links == 1 && _identity.platform.number_of_links > 1 {
                    "RETRIEVAL_STATE_HARDLINK_REJECTED"
                } else {
                    "RETRIEVAL_STATE_WRITER_ALIAS_REJECTED"
                },
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn path_identity_snapshot(_path: &Path, metadata: &fs::Metadata) -> RetrievalResult<FileIdentity> {
    file_identity(metadata, None)
}

#[cfg(windows)]
fn path_identity_snapshot(path: &Path, metadata: &fs::Metadata) -> RetrievalResult<FileIdentity> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

    let probe = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let probed = probe.metadata()?;
    if metadata.len() != probed.len()
        || metadata.file_attributes() != probed.file_attributes()
        || metadata.creation_time() != probed.creation_time()
        || metadata.last_write_time() != probed.last_write_time()
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
    }
    file_identity(&probed, Some(&probe))
}

#[cfg(not(any(unix, windows)))]
fn path_identity_snapshot(_path: &Path, metadata: &fs::Metadata) -> RetrievalResult<FileIdentity> {
    file_identity(metadata, None)
}

fn assert_directory_capability(directory: &Path) -> RetrievalResult<()> {
    let reopened = validate_existing_state_directory(directory)?;
    if !equivalent_paths(&reopened, directory) {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_CHANGED"));
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn assert_legacy_writer_directory_permissions(directory: &Path) -> RetrievalResult<()> {
    use std::os::unix::fs::MetadataExt;
    if fs::symlink_metadata(directory)?.mode() & 0o777 != 0o700
        || fs::metadata(directory)?.mode() & 0o777 != 0o700
    {
        return Err(invalid("RETRIEVAL_STATE_DIRECTORY_PERMISSION_REJECTED"));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn assert_legacy_writer_directory_permissions(_directory: &Path) -> RetrievalResult<()> {
    Ok(())
}

fn read_sealed_bytes(
    path: &Path,
    directory: &Path,
    allowed_links: u64,
) -> RetrievalResult<Vec<u8>> {
    read_sealed_bytes_with_hook(path, directory, allowed_links, None)
}

fn read_sealed_bytes_with_hook(
    path: &Path,
    directory: &Path,
    allowed_links: u64,
    mut after_path_snapshot: Option<&mut dyn FnMut()>,
) -> RetrievalResult<Vec<u8>> {
    assert_directory_capability(directory)?;
    if path
        .parent()
        .is_none_or(|parent| !equivalent_paths(parent, directory))
        || !contained_path(path, directory)
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_PATH_ESCAPE"));
    }
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(invalid("RETRIEVAL_STATE_WRITER_ALIAS_REJECTED"));
    }
    let before_identity = path_identity_snapshot(path, &before)?;
    require_owner_file(&before, &before_identity, allowed_links)?;
    if let Some(hook) = after_path_snapshot.as_mut() {
        hook();
    }
    if before.len() == 0 || before.len() > MAX_LOCK_BYTES {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_SIZE_INVALID"));
    }
    let mut file = File::open(path)?;
    let opened = file.metadata()?;
    let opened_identity = file_identity(&opened, Some(&file))?;
    require_owner_file(&opened, &opened_identity, allowed_links)?;
    if opened_identity != before_identity {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize + 1);
    Read::by_ref(&mut file)
        .take(MAX_LOCK_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != before.len() {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
    }
    let after = file.metadata()?;
    let path_after = fs::symlink_metadata(path)?;
    let after_identity = file_identity(&after, Some(&file))?;
    let path_after_identity = path_identity_snapshot(path, &path_after)?;
    require_owner_file(&after, &after_identity, allowed_links)?;
    require_owner_file(&path_after, &path_after_identity, allowed_links)?;
    if after_identity != before_identity || path_after_identity != before_identity {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
    }
    let canonical = fs::canonicalize(path)?;
    if canonical
        .parent()
        .is_none_or(|parent| !equivalent_paths(parent, directory))
        || canonical
            .file_name()
            .zip(path.file_name())
            .is_none_or(|(actual, requested)| actual != requested)
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_PATH_ESCAPE"));
    }
    Ok(bytes)
}

fn read_state_entries(directory: &Path) -> RetrievalResult<Vec<String>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        if entries.len() >= MAX_STATE_ENTRIES {
            return Err(invalid("RETRIEVAL_STATE_DIRECTORY_ENTRY_LIMIT_EXCEEDED"));
        }
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| invalid("RETRIEVAL_STATE_AUTHORITY_NAME_INVALID"))?;
        entries.push(name);
    }
    Ok(entries)
}

fn assert_canonical_controlled_names(directory: &Path) -> RetrievalResult<Vec<String>> {
    let entries = read_state_entries(directory)?;
    for entry in &entries {
        for expected in AUTHORITY_EVIDENCE_NAMES
            .iter()
            .copied()
            .chain([LEGACY_WRITER_LOCK_FILE, LEGACY_WRITER_RECOVERY_FILE])
        {
            if entry.eq_ignore_ascii_case(expected) && entry != expected {
                return Err(invalid("RETRIEVAL_STATE_AUTHORITY_NAME_INVALID"));
            }
        }
    }
    Ok(entries)
}

fn assert_no_writer_temporaries(directory: &Path) -> RetrievalResult<()> {
    for name in read_state_entries(directory)? {
        if classify_writer_temporary(&name).is_some()
            || is_controlled_writer_artifact_spelling(&name)
        {
            return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_REQUIRED"));
        }
    }
    Ok(())
}

fn validate_legacy_pointer_bytes(bytes: &[u8], allow_historical_lite: bool) -> RetrievalResult<()> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| invalid("RETRIEVAL_STATE_POINTER_JSON_INVALID"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("RETRIEVAL_STATE_POINTER_INVALID"))?;
    if object.get("contract_version").and_then(Value::as_str) == Some(PHASE3_TOMBSTONE_CONTRACT) {
        return Err(invalid("RETRIEVAL_PHASE3_AUTHORITY_ACTIVE"));
    }
    let historical = object.len() == 3
        && object.contains_key("contract_version")
        && object.contains_key("database_file")
        && object.contains_key("manifest");
    let full = object.len() == 2
        && object.contains_key("database_file")
        && object.contains_key("manifest");
    if !full && !(allow_historical_lite && historical) {
        return Err(invalid("RETRIEVAL_STATE_POINTER_INVALID"));
    }
    let manifest = object
        .get("manifest")
        .ok_or_else(|| invalid("RETRIEVAL_STATE_POINTER_INVALID"))?;
    let projection_digest = validate_retrieval_manifest(manifest, false)
        .map_err(|_| invalid("RETRIEVAL_STATE_POINTER_INVALID"))?;
    let manifest_contract = manifest
        .as_object()
        .and_then(|value| value.get("contract_version"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("RETRIEVAL_STATE_POINTER_INVALID"))?;
    if historical
        && object.get("contract_version").and_then(Value::as_str) != Some(manifest_contract)
    {
        return Err(invalid("RETRIEVAL_STATE_POINTER_INVALID"));
    }
    // The historical three-key form was a Lite-only schema-2 implementation
    // detail. Full's schema-3 pointer has always been the exact two-key form.
    if historical && manifest_contract != RETRIEVAL_CONTRACT {
        return Err(invalid("RETRIEVAL_STATE_POINTER_INVALID"));
    }
    let expected = format!(
        "retrieval-{}.sqlite",
        projection_digest.trim_start_matches("sha256:")
    );
    if object.get("database_file").and_then(Value::as_str) != Some(expected.as_str())
        || bytes != canonical_bytes(&value)?.as_slice()
    {
        return Err(invalid("RETRIEVAL_STATE_POINTER_INVALID"));
    }
    Ok(())
}

fn pointer_digest(directory: &Path) -> RetrievalResult<Option<String>> {
    let path = directory.join("active-retrieval.json");
    if !path_entry_exists(&path)? {
        return Ok(None);
    }
    let bytes = read_sealed_bytes(&path, directory, 1)?;
    validate_legacy_pointer_bytes(&bytes, true)?;
    Ok(Some(sha256(bytes)))
}

pub(crate) fn assert_no_phase3_authority(directory: &Path) -> RetrievalResult<()> {
    assert_directory_capability(directory)?;
    let entries = assert_canonical_controlled_names(directory)?;
    for evidence in AUTHORITY_EVIDENCE_NAMES {
        if *evidence != "active-retrieval.json" && entries.iter().any(|entry| entry == evidence) {
            return Err(invalid("RETRIEVAL_PHASE3_AUTHORITY_ACTIVE"));
        }
    }
    if entries.iter().any(|entry| entry == "active-retrieval.json") {
        let path = directory.join("active-retrieval.json");
        let bytes = read_sealed_bytes(&path, directory, 1)?;
        validate_legacy_pointer_bytes(&bytes, true)?;
    }
    Ok(())
}

fn read_lock_path(
    path: &Path,
    directory: &Path,
    allowed_links: u64,
) -> RetrievalResult<(LegacyWriterLock, String)> {
    let bytes = read_sealed_bytes(path, directory, allowed_links)?;
    let lock: LegacyWriterLock = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("RETRIEVAL_STATE_WRITER_LOCK_JSON_INVALID"))?;
    let lock = seal_lock(lock)?;
    if bytes != canonical_bytes(&lock)? {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_NONCANONICAL"));
    }
    Ok((lock, sha256(bytes)))
}

fn read_lock(directory: &Path) -> RetrievalResult<(LegacyWriterLock, String)> {
    read_lock_path(&directory.join(LEGACY_WRITER_LOCK_FILE), directory, 1)
}

fn remove_created_directory_if_empty(directory: &Path, created: bool) {
    if created
        && fs::read_dir(directory)
            .ok()
            .is_some_and(|mut entries| entries.next().is_none())
    {
        let _ = fs::remove_dir(directory);
    }
}

pub(crate) fn acquire_legacy_retrieval_writer(
    state_directory: &Path,
) -> RetrievalResult<LegacyRetrievalWriterCapability> {
    validate_state_path_namespace(state_directory)?;
    let requested = validate_state_directory(state_directory)?;
    let created = !path_entry_exists(&requested)?;
    fs::create_dir_all(&requested)?;
    let directory = validate_existing_state_directory(&requested)?;
    if created {
        harden_directory_permissions(&directory)?;
    }
    assert_legacy_writer_directory_permissions(&directory)?;
    if let Err(error) = assert_no_phase3_authority(&directory) {
        remove_created_directory_if_empty(&directory, created);
        return Err(error);
    }
    if let Err(error) = assert_no_writer_temporaries(&directory) {
        remove_created_directory_if_empty(&directory, created);
        return Err(error);
    }
    if path_entry_exists(&directory.join(LEGACY_WRITER_RECOVERY_FILE))? {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_ACTIVE"));
    }
    let prior_pointer_digest = pointer_digest(&directory)?;
    let material = LegacyWriterLock {
        contract_version: LOCK_CONTRACT.to_owned(),
        lock_id: fresh_digest("retrieval-writer-lock"),
        process_id: std::process::id(),
        prior_pointer_digest,
        target_pointer_digest: None,
        lock_digest: String::new(),
    };
    let lock = LegacyWriterLock {
        lock_digest: canonical_digest(&lock_material(&material))?,
        ..material
    };
    let lock = seal_lock(lock)?;
    let bytes = canonical_bytes(&lock)?;
    let lock_path = directory.join(LEGACY_WRITER_LOCK_FILE);
    if let Err(error) = write_owner_file(&lock_path, &bytes) {
        remove_created_directory_if_empty(&directory, created);
        if matches!(&error, RetrievalError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists)
        {
            return Err(invalid("RETRIEVAL_STATE_WRITER_LOCKED"));
        }
        let _ = remove_uncommitted_writer_temporary(&lock_path, &directory);
        return Err(error);
    }
    if let Err(error) =
        harden_file_permissions(&lock_path).and_then(|()| sync_directory(&directory))
    {
        let _ = remove_uncommitted_writer_temporary(&lock_path, &directory);
        remove_created_directory_if_empty(&directory, created);
        return Err(error);
    }
    let file_digest = sha256(&bytes);
    let postcheck = (|| {
        assert_no_phase3_authority(&directory)?;
        assert_no_writer_temporaries(&directory)?;
        let (reopened, reopened_digest) = read_lock(&directory)?;
        if reopened != lock || reopened_digest != file_digest {
            return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
        }
        Ok(())
    })();
    if let Err(error) = postcheck {
        if read_lock(&directory)
            .ok()
            .is_some_and(|(_, digest)| digest == file_digest)
        {
            let _ = fs::remove_file(&lock_path);
            let _ = sync_directory(&directory);
        }
        remove_created_directory_if_empty(&directory, created);
        return Err(error);
    }
    let capability = LegacyRetrievalWriterCapability {
        state_directory: directory,
        lock,
        file_digest,
        remove_empty_directory_on_release: created,
        held: true,
    };
    recover_lite_database_temporaries(&capability)?;
    Ok(capability)
}

pub(crate) fn assert_legacy_writer_capability(
    capability: &LegacyRetrievalWriterCapability,
    state_directory: &Path,
) -> RetrievalResult<()> {
    if !capability.held {
        return Err(invalid("RETRIEVAL_STATE_WRITER_CAPABILITY_INVALID"));
    }
    let requested = validate_state_directory(state_directory)?;
    let requested = if path_entry_exists(&requested)? {
        validate_existing_state_directory(&requested)?
    } else {
        requested
    };
    if !equivalent_paths(&requested, &capability.state_directory) {
        return Err(invalid("RETRIEVAL_STATE_WRITER_COORDINATE_MISMATCH"));
    }
    let (lock, digest) = read_lock(&capability.state_directory)?;
    if lock != capability.lock || digest != capability.file_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
    }
    Ok(())
}

fn replace_lock(
    capability: &mut LegacyRetrievalWriterCapability,
    lock: LegacyWriterLock,
) -> RetrievalResult<()> {
    let suffix = fresh_digest("retrieval-writer-temp");
    let suffix = &suffix["sha256:".len().."sha256:".len() + 16];
    let temporary = capability.state_directory.join(format!(
        "{LEGACY_WRITER_LOCK_FILE}.{}.{suffix}.tmp",
        std::process::id()
    ));
    let bytes = canonical_bytes(&lock)?;
    if let Err(error) = write_owner_file(&temporary, &bytes) {
        let _ = remove_uncommitted_writer_temporary(&temporary, &capability.state_directory);
        return Err(error);
    }
    if let Err(error) = harden_file_permissions(&temporary) {
        let _ = remove_uncommitted_writer_temporary(&temporary, &capability.state_directory);
        return Err(error);
    }
    atomic_replace(
        &temporary,
        &capability.state_directory.join(LEGACY_WRITER_LOCK_FILE),
    )?;
    sync_directory(&capability.state_directory)?;
    let (reopened, file_digest) = read_lock(&capability.state_directory)?;
    if reopened != lock || file_digest != sha256(&bytes) {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_CHANGED"));
    }
    capability.lock = reopened;
    capability.file_digest = file_digest;
    Ok(())
}

pub(crate) fn bind_legacy_writer_target(
    capability: &mut LegacyRetrievalWriterCapability,
    target_pointer_bytes: &[u8],
) -> RetrievalResult<()> {
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    validate_legacy_pointer_bytes(target_pointer_bytes, false)?;
    if capability.lock.target_pointer_digest.is_some() {
        return Err(invalid("RETRIEVAL_STATE_WRITER_TARGET_ALREADY_BOUND"));
    }
    if pointer_digest(capability.state_directory())? != capability.lock.prior_pointer_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_PRIOR_POINTER_CHANGED"));
    }
    let material = LegacyWriterLock {
        target_pointer_digest: Some(sha256(target_pointer_bytes)),
        lock_digest: String::new(),
        ..capability.lock.clone()
    };
    let lock = LegacyWriterLock {
        lock_digest: canonical_digest(&lock_material(&material))?,
        ..material
    };
    replace_lock(capability, seal_lock(lock)?)
}

pub(crate) fn assert_legacy_writer_commit(
    capability: &LegacyRetrievalWriterCapability,
    target_pointer_bytes: &[u8],
) -> RetrievalResult<()> {
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    if capability.lock.target_pointer_digest.as_deref()
        != Some(sha256(target_pointer_bytes).as_str())
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_TARGET_MISMATCH"));
    }
    if pointer_digest(capability.state_directory())? != capability.lock.prior_pointer_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_PRIOR_POINTER_CHANGED"));
    }
    assert_legacy_writer_capability(capability, capability.state_directory())
}

pub(crate) fn verify_legacy_writer_target_published(
    capability: &LegacyRetrievalWriterCapability,
    target_pointer_bytes: &[u8],
) -> RetrievalResult<()> {
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    let target = sha256(target_pointer_bytes);
    if capability.lock.target_pointer_digest.as_deref() != Some(target.as_str())
        || pointer_digest(capability.state_directory())?.as_deref() != Some(target.as_str())
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_TARGET_PUBLICATION_INVALID"));
    }
    assert_no_phase3_authority(capability.state_directory())?;
    assert_legacy_writer_capability(capability, capability.state_directory())
}

pub(crate) fn release_legacy_retrieval_writer(
    capability: &mut LegacyRetrievalWriterCapability,
) -> RetrievalResult<()> {
    release_owned_writer_if_unambiguous(capability)
}

fn release_owned_writer_if_unambiguous(
    capability: &mut LegacyRetrievalWriterCapability,
) -> RetrievalResult<()> {
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    let current = pointer_digest(capability.state_directory())?;
    if current != capability.lock.prior_pointer_digest
        && current != capability.lock.target_pointer_digest
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RELEASE_STATE_INVALID"));
    }
    recover_writer_temporaries(capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    if pointer_digest(capability.state_directory())? != current {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RELEASE_STATE_INVALID"));
    }
    fs::remove_file(capability.state_directory.join(LEGACY_WRITER_LOCK_FILE))?;
    sync_directory(&capability.state_directory)?;
    capability.held = false;
    if capability.remove_empty_directory_on_release
        && fs::read_dir(&capability.state_directory)?.next().is_none()
    {
        fs::remove_dir(&capability.state_directory)?;
    }
    Ok(())
}

pub(crate) fn finish_with_writer<T>(
    result: RetrievalResult<T>,
    capability: &mut LegacyRetrievalWriterCapability,
) -> RetrievalResult<T> {
    let release = if capability.is_held() {
        release_legacy_retrieval_writer(capability)
    } else {
        Ok(())
    };
    match (result, release) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

pub(crate) fn recover_stale_legacy_retrieval_writer(
    state_directory: &Path,
    expected_lock_digest: &str,
    confirm_process_incarnation_stale: bool,
    confirm_recovery_claim_stale: bool,
) -> RetrievalResult<()> {
    if !is_sha256_digest(expected_lock_digest) || !confirm_process_incarnation_stale {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_NOT_AUTHORIZED"));
    }
    validate_state_path_namespace(state_directory)?;
    let directory = validate_existing_state_directory(state_directory)?;
    let lock_path = directory.join(LEGACY_WRITER_LOCK_FILE);
    let claim_path = directory.join(LEGACY_WRITER_RECOVERY_FILE);
    let claim_exists = path_entry_exists(&claim_path)?;
    let lock_exists = path_entry_exists(&lock_path)?;
    if claim_exists && !confirm_recovery_claim_stale {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_ALREADY_CLAIMED"));
    }
    if claim_exists && !lock_exists {
        let _ = read_lock_path(&claim_path, &directory, 1)?;
        fs::hard_link(&claim_path, &lock_path)?;
        sync_directory(&directory)?;
    }
    let (lock, file_digest) = if claim_exists {
        let claim = read_lock_path(&claim_path, &directory, 2)?;
        let live = read_lock_path(&lock_path, &directory, 2)?;
        if claim != live {
            return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_CLAIM_CHANGED"));
        }
        claim
    } else {
        read_lock(&directory)?
    };
    if lock.lock_digest != expected_lock_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_LOCK_DIGEST_MISMATCH"));
    }
    if claim_exists {
        fs::remove_file(&claim_path)?;
        sync_directory(&directory)?;
        let (live, live_digest) = read_lock(&directory)?;
        if live != lock || live_digest != file_digest {
            return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_CLAIM_CHANGED"));
        }
    }
    fs::hard_link(&lock_path, &claim_path)?;
    sync_directory(&directory)?;
    let (claimed, claimed_digest) = read_lock_path(&claim_path, &directory, 2)?;
    if claimed != lock || claimed_digest != file_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_CLAIM_CHANGED"));
    }
    recover_writer_temporaries(&directory)?;
    assert_no_phase3_authority(&directory)?;
    let current = pointer_digest(&directory)?;
    if current != lock.prior_pointer_digest && current != lock.target_pointer_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_STATE_INVALID"));
    }
    fs::remove_file(&lock_path)?;
    sync_directory(&directory)?;
    let (final_claim, final_digest) = read_lock_path(&claim_path, &directory, 1)?;
    if final_claim != lock || final_digest != file_digest {
        return Err(invalid("RETRIEVAL_STATE_WRITER_RECOVERY_CLAIM_CHANGED"));
    }
    fs::remove_file(&claim_path)?;
    sync_directory(&directory)
}

fn recover_writer_temporaries(directory: &Path) -> RetrievalResult<()> {
    for name in read_state_entries(directory)? {
        let kind = classify_writer_temporary(&name);
        if kind.is_none() {
            if is_controlled_writer_artifact_spelling(&name) {
                return Err(invalid("RETRIEVAL_STATE_WRITER_ARTIFACT_NAME_INVALID"));
            }
            continue;
        }
        let path = directory.join(&name);
        match kind.unwrap() {
            WriterTemporaryKind::Lock => {
                read_lock_path(&path, directory, 1)?;
            }
            WriterTemporaryKind::Pointer => {
                let bytes = read_sealed_bytes(&path, directory, 1)?;
                validate_legacy_pointer_bytes(&bytes, false)
                    .map_err(|_| invalid("RETRIEVAL_STATE_WRITER_POINTER_TEMP_INVALID"))?;
            }
        }
        fs::remove_file(path)?;
    }
    sync_directory(directory)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriterTemporaryKind {
    Lock,
    Pointer,
}

fn is_ascii_decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn classify_writer_temporary(name: &str) -> Option<WriterTemporaryKind> {
    if let Some(middle) = name
        .strip_prefix("retrieval-writer.lock.")
        .and_then(|value| value.strip_suffix(".tmp"))
    {
        let mut fields = middle.split('.');
        if fields.next().is_some_and(is_ascii_decimal)
            && fields.next().is_some_and(|value| is_lower_hex(value, 16))
            && fields.next().is_none()
        {
            return Some(WriterTemporaryKind::Lock);
        }
    }
    if let Some(middle) = name
        .strip_prefix("active-retrieval.json.")
        .and_then(|value| value.strip_suffix(".tmp"))
    {
        if is_ascii_decimal(middle) {
            return Some(WriterTemporaryKind::Pointer);
        }
    }
    None
}

fn is_controlled_writer_artifact_spelling(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let accepted_fixed = matches!(
        name,
        LEGACY_WRITER_LOCK_FILE | LEGACY_WRITER_RECOVERY_FILE | "active-retrieval.json"
    );
    if accepted_fixed {
        return false;
    }
    lower.starts_with("retrieval-writer.lock")
        || lower.starts_with("retrieval-writer.recovery")
        || lower.starts_with("active-retrieval.json")
}

fn is_lite_database_temporary(name: &str) -> bool {
    let core = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .unwrap_or(name);
    core.strip_prefix("retrieval-")
        .and_then(|value| value.strip_suffix(".tmp"))
        .and_then(|value| value.split_once(".sqlite."))
        .is_some_and(|(digest, process)| is_lower_hex(digest, 64) && is_ascii_decimal(process))
}

fn is_lite_database_temporary_spelling(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("retrieval-")
        && lower.contains(".sqlite.")
        && (lower.ends_with(".tmp") || lower.ends_with(".tmp-wal") || lower.ends_with(".tmp-shm"))
}

fn recover_lite_database_temporaries(
    capability: &LegacyRetrievalWriterCapability,
) -> RetrievalResult<()> {
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    let mut temporaries = Vec::new();
    for name in read_state_entries(capability.state_directory())? {
        if is_lite_database_temporary(&name) {
            temporaries.push(name);
        } else if is_lite_database_temporary_spelling(&name) {
            return Err(invalid("RETRIEVAL_STATE_WRITER_DATABASE_TEMP_NAME_INVALID"));
        }
    }
    temporaries.sort();
    for name in &temporaries {
        validate_plain_owner_temporary(
            &capability.state_directory().join(name),
            capability.state_directory(),
        )?;
    }
    assert_legacy_writer_capability(capability, capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    for name in temporaries {
        fs::remove_file(capability.state_directory().join(name))?;
    }
    sync_directory(capability.state_directory())?;
    assert_no_phase3_authority(capability.state_directory())?;
    assert_legacy_writer_capability(capability, capability.state_directory())
}

fn validate_plain_owner_temporary(path: &Path, directory: &Path) -> RetrievalResult<()> {
    assert_directory_capability(directory)?;
    if path
        .parent()
        .is_none_or(|parent| !equivalent_paths(parent, directory))
        || !contained_path(path, directory)
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_PATH_ESCAPE"));
    }
    let before = fs::symlink_metadata(path)?;
    let before_identity = path_identity_snapshot(path, &before)?;
    require_owner_file(&before, &before_identity, 1)
        .map_err(|_| invalid("RETRIEVAL_STATE_WRITER_TEMP_ALIAS_REJECTED"))?;
    let file = File::open(path)?;
    let opened = file.metadata()?;
    let opened_identity = file_identity(&opened, Some(&file))?;
    require_owner_file(&opened, &opened_identity, 1)
        .map_err(|_| invalid("RETRIEVAL_STATE_WRITER_TEMP_ALIAS_REJECTED"))?;
    let path_after = fs::symlink_metadata(path)?;
    let path_after_identity = path_identity_snapshot(path, &path_after)?;
    if opened_identity != before_identity || path_after_identity != before_identity {
        return Err(invalid("RETRIEVAL_STATE_WRITER_TEMP_CHANGED"));
    }
    let canonical = fs::canonicalize(path)?;
    if canonical
        .parent()
        .is_none_or(|parent| !equivalent_paths(parent, directory))
        || canonical
            .file_name()
            .zip(path.file_name())
            .is_none_or(|(actual, requested)| actual != requested)
    {
        return Err(invalid("RETRIEVAL_STATE_WRITER_PATH_ESCAPE"));
    }
    Ok(())
}

pub(crate) fn remove_uncommitted_writer_temporary(
    path: &Path,
    directory: &Path,
) -> RetrievalResult<()> {
    if !path_entry_exists(path)? {
        return Ok(());
    }
    validate_plain_owner_temporary(path, directory)?;
    fs::remove_file(path)?;
    sync_directory(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn schema_two_manifest() -> Value {
        serde_json::json!({
            "chunk_count": 0,
            "chunker_version": "gkos-heading-chunker/1",
            "configuration_digest": digest('b'),
            "contract_version": "gkos-retrieval/1.0.0-draft.1",
            "embedding_dimensions": null,
            "embedding_model_id": null,
            "embedding_provider_id": null,
            "engine_version": "2.1.2",
            "lexical_backend": "sqlite_lexical_scan",
            "policy_digest": digest('c'),
            "projection_digest": digest('a'),
            "projection_id": format!("retrieval:{}", "a".repeat(24)),
            "projection_schema_version": 2,
            "source_count": 0,
            "source_snapshot_digest": digest('d'),
            "tokenizer_version": "gkos-ascii-whitespace/1",
            "vault_id": "vault",
        })
    }

    fn schema_three_manifest() -> Value {
        serde_json::json!({
            "candidate_chunk_count": 0,
            "candidate_declaration_count": 0,
            "candidate_source_count": 0,
            "chunker_version": "gkos-heading-chunker/1",
            "configuration_digest": digest('b'),
            "contract_version": "gkos-retrieval/1.0.0-draft.2",
            "embedding_dimensions": null,
            "embedding_eligible_candidate_chunk_count": 0,
            "embedding_model_id": null,
            "embedding_provider_id": null,
            "engine_version": "2.1.2",
            "gkx_projection_profile": "gkx-2.3-validating-projection",
            "gkx_standard_commit": "a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6",
            "lexical_backend": "sqlite_lexical_scan",
            "policy_digest": digest('c'),
            "projection_digest": digest('e'),
            "projection_id": format!("retrieval:{}", "e".repeat(24)),
            "projection_schema_version": 3,
            "provenance_contract_version": "gkos-retrieval-provenance/1.0.0-draft.1",
            "represented_candidate_source_count": 0,
            "source_snapshot_digest": digest('d'),
            "tokenizer_version": "gkos-ascii-whitespace/1",
            "vault_id": "vault",
        })
    }

    fn pointer_bytes(manifest: Value) -> Vec<u8> {
        let projection = manifest["projection_digest"].as_str().unwrap();
        canonical_bytes(&serde_json::json!({
            "database_file": format!("retrieval-{}.sqlite", projection.trim_start_matches("sha256:")),
            "manifest": manifest,
        }))
        .unwrap()
    }

    fn owner_write(path: &Path, bytes: &[u8]) {
        write_owner_file(path, bytes).unwrap();
        harden_file_permissions(path).unwrap();
    }

    #[test]
    fn lock_bytes_use_fulls_exact_six_field_canonical_shape() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        let mut capability = acquire_legacy_retrieval_writer(&state).unwrap();
        let bytes = fs::read(state.join(LEGACY_WRITER_LOCK_FILE)).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "contract_version",
                "lock_digest",
                "lock_id",
                "prior_pointer_digest",
                "process_id",
                "target_pointer_digest",
            ]
        );
        assert_eq!(bytes, canonical_bytes(&value).unwrap());
        release_legacy_retrieval_writer(&mut capability).unwrap();
    }

    #[test]
    fn full_schema_two_and_schema_three_pointers_are_guard_inputs() {
        let schema_two = pointer_bytes(schema_two_manifest());
        let schema_three = pointer_bytes(schema_three_manifest());
        validate_legacy_pointer_bytes(&schema_two, false).unwrap();
        validate_legacy_pointer_bytes(&schema_three, false).unwrap();

        let manifest = schema_two_manifest();
        let historical = canonical_bytes(&serde_json::json!({
            "contract_version": manifest["contract_version"],
            "database_file": format!("retrieval-{}.sqlite", digest('a').trim_start_matches("sha256:")),
            "manifest": manifest,
        }))
        .unwrap();
        validate_legacy_pointer_bytes(&historical, true).unwrap();
        assert!(validate_legacy_pointer_bytes(&historical, false).is_err());

        let schema_three = schema_three_manifest();
        let historical_three = canonical_bytes(&serde_json::json!({
            "contract_version": schema_three["contract_version"],
            "database_file": format!("retrieval-{}.sqlite", digest('e').trim_start_matches("sha256:")),
            "manifest": schema_three,
        }))
        .unwrap();
        assert!(validate_legacy_pointer_bytes(&historical_three, true).is_err());
    }

    #[test]
    fn exact_writer_temporary_names_are_classified() {
        assert_eq!(
            classify_writer_temporary("retrieval-writer.lock.42.0123456789abcdef.tmp"),
            Some(WriterTemporaryKind::Lock)
        );
        assert_eq!(
            classify_writer_temporary("active-retrieval.json.42.tmp"),
            Some(WriterTemporaryKind::Pointer)
        );
        let database = format!("retrieval-{}.sqlite.42.tmp", "a".repeat(64));
        assert_eq!(classify_writer_temporary(&database), None);
        assert_eq!(classify_writer_temporary(&format!("{database}-wal")), None);
        assert!(is_lite_database_temporary(&database));
        assert!(is_lite_database_temporary(&format!("{database}-wal")));
        for invalid in [
            "Retrieval-writer.lock.42.0123456789abcdef.tmp",
            "retrieval-writer.lock.x.0123456789abcdef.tmp",
            "active-retrieval.json.x.tmp",
            "active-retrieval.json.42.bad.tmp",
            "active-retrieval.json.42.0123456789abcdef.tmp",
        ] {
            assert!(classify_writer_temporary(invalid).is_none());
            assert!(is_controlled_writer_artifact_spelling(invalid));
        }
    }

    #[test]
    fn stale_recovery_removes_only_exact_sealed_writer_temporaries() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let capability = acquire_legacy_retrieval_writer(&state).unwrap();
        let expected = capability.lock.lock_digest.clone();
        std::mem::forget(capability);
        let lock_bytes = fs::read(state.join(LEGACY_WRITER_LOCK_FILE)).unwrap();
        owner_write(
            &state.join(format!(
                "retrieval-writer.lock.{}.0123456789abcdef.tmp",
                std::process::id()
            )),
            &lock_bytes,
        );
        owner_write(
            &state.join(format!("active-retrieval.json.{}.tmp", std::process::id())),
            &pointer_bytes(schema_two_manifest()),
        );
        let database = state.join(format!(
            "retrieval-{}.sqlite.{}.tmp",
            "f".repeat(64),
            std::process::id()
        ));
        owner_write(&database, b"partial database");
        owner_write(&PathBuf::from(format!("{}-wal", database.display())), b"");

        recover_stale_legacy_retrieval_writer(&state, &expected, true, false).unwrap();
        assert!(!path_entry_exists(&state.join(LEGACY_WRITER_LOCK_FILE)).unwrap());
        assert!(!path_entry_exists(&state.join(LEGACY_WRITER_RECOVERY_FILE)).unwrap());
        assert!(path_entry_exists(&database).unwrap());
        assert!(path_entry_exists(&PathBuf::from(format!("{}-wal", database.display()))).unwrap());

        let mut next = acquire_legacy_retrieval_writer(&state).unwrap();
        assert!(!path_entry_exists(&database).unwrap());
        assert!(!path_entry_exists(&PathBuf::from(format!("{}-wal", database.display()))).unwrap());
        release_legacy_retrieval_writer(&mut next).unwrap();
        assert_eq!(read_state_entries(&state).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn recovery_claim_resumes_and_malformed_or_aliased_temp_retains_guard() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let capability = acquire_legacy_retrieval_writer(&state).unwrap();
        let expected = capability.lock.lock_digest.clone();
        let malformed = state.join("Active-Retrieval.json.42.tmp");
        owner_write(&malformed, &pointer_bytes(schema_two_manifest()));
        assert!(recover_stale_legacy_retrieval_writer(&state, &expected, true, false).is_err());
        assert!(path_entry_exists(&state.join(LEGACY_WRITER_LOCK_FILE)).unwrap());
        assert!(path_entry_exists(&state.join(LEGACY_WRITER_RECOVERY_FILE)).unwrap());
        fs::remove_file(&malformed).unwrap();
        recover_stale_legacy_retrieval_writer(&state, &expected, true, true).unwrap();

        let other = root.path().join("other");
        let state = root.path().join("state-two");
        fs::create_dir(&state).unwrap();
        harden_directory_permissions(&state).unwrap();
        let database = state.join(format!("retrieval-{}.sqlite.42.tmp", "f".repeat(64)));
        owner_write(&database, b"partial");
        fs::hard_link(&database, &other).unwrap();
        assert!(acquire_legacy_retrieval_writer(&state).is_err());
        assert!(path_entry_exists(&database).unwrap());
        assert!(!path_entry_exists(&state.join(LEGACY_WRITER_LOCK_FILE)).unwrap());
        fs::remove_file(&other).unwrap();
        let mut capability = acquire_legacy_retrieval_writer(&state).unwrap();
        assert!(!path_entry_exists(&database).unwrap());
        release_legacy_retrieval_writer(&mut capability).unwrap();
    }

    #[test]
    fn private_database_temporary_cleanup_rejects_case_and_widened_mode() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        fs::create_dir(&state).unwrap();
        harden_directory_permissions(&state).unwrap();
        let canonical = format!("retrieval-{}.sqlite.42.tmp", "e".repeat(64));
        let mixed_case = state.join(format!("Retrieval-{}.sqlite.42.tmp", "e".repeat(64)));
        owner_write(&mixed_case, b"partial");
        assert!(acquire_legacy_retrieval_writer(&state).is_err());
        assert!(path_entry_exists(&mixed_case).unwrap());
        assert!(!path_entry_exists(&state.join(LEGACY_WRITER_LOCK_FILE)).unwrap());
        fs::remove_file(&mixed_case).unwrap();

        let canonical = state.join(canonical);
        owner_write(&canonical, b"partial");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&canonical, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(acquire_legacy_retrieval_writer(&state).is_err());
            assert!(path_entry_exists(&canonical).unwrap());
            assert_eq!(
                fs::metadata(&canonical).unwrap().permissions().mode() & 0o777,
                0o644
            );
            assert!(!path_entry_exists(&state.join(LEGACY_WRITER_LOCK_FILE)).unwrap());
            fs::set_permissions(&canonical, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut capability = acquire_legacy_retrieval_writer(&state).unwrap();
        assert!(!path_entry_exists(&canonical).unwrap());
        release_legacy_retrieval_writer(&mut capability).unwrap();
    }

    #[test]
    fn sealed_reads_reject_path_replacement_after_the_authoritative_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        fs::create_dir(&state).unwrap();
        harden_directory_permissions(&state).unwrap();
        for name in [
            LEGACY_WRITER_LOCK_FILE,
            "active-retrieval.json",
            LEGACY_WRITER_RECOVERY_FILE,
        ] {
            let path = state.join(name);
            let replacement = state.join(format!("replacement-{name}"));
            owner_write(&path, b"{}\n");
            owner_write(&replacement, b"[]\n");
            let mut replace = || atomic_replace(&replacement, &path).unwrap();
            let error =
                read_sealed_bytes_with_hook(&path, &state, 1, Some(&mut replace)).unwrap_err();
            assert!(error.to_string().contains("LOCK_CHANGED"));
            fs::remove_file(path).unwrap();
        }
    }
}
