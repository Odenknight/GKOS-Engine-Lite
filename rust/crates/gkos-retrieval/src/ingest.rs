//! Verifier-only consumer for the Full-owned Phase-3 ingest envelopes.
//!
//! This module deliberately has no YAML or TOML parser and no GKX identity,
//! relationship, profile-selection, or Decision-A resolver. It accepts only
//! already-normalized Full-produced JSON envelopes, verifies their frozen
//! coordinates, derivations, ordering, and digests, and returns opaque sealed
//! values suitable for persistence.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
#[cfg(test)]
use std::{fs, path::Path};

use serde_json::{Map, Value};

use crate::contract::{
    is_valid_retrieval_source_path, CHUNKER_VERSION, GKX_PROJECTION_PROFILE, GKX_STANDARD_COMMIT,
    LINEAGE_PROJECTION_SCHEMA_VERSION, PROJECTION_SCHEMA_VERSION, RETRIEVAL_CONTRACT,
    RETRIEVAL_LINEAGE_CONTRACT, RETRIEVAL_PROVENANCE_CONTRACT, TOKENIZER_VERSION,
};
use crate::digest::{canonical_digest, canonical_json, sha256};
use crate::fusion::code_unit_compare;
use crate::sqlite_store::trim_ecmascript_whitespace;
use crate::{RetrievalError, RetrievalResult};

const PROFILE_FIXTURE: &str = include_str!(
    "../../../contracts/gkos-ingest-validation-1.0.0-draft.1/conformance-fixture.json"
);
const FINDING_SCHEMA: &str =
    include_str!("../../../contracts/gkos-ingest-validation-1.0.0-draft.1/finding.schema.json");

const NORMALIZED_PROFILE_VERSION: &str = "gkos-frontmatter-profile-effective/1.0.0-draft.1";
const PROFILE_COORDINATE_VERSION: &str = "gkos-frontmatter-profile-coordinate/1.0.0-draft.1";
const FINDING_VERSION: &str = "gkos-ingest-finding/1.0.0-draft.1";
const REJECTION_VERSION: &str = "gkos-ingest-rejection/1.0.0-draft.1";
const OBSERVATION_VERSION: &str = "gkos-ingest-source-observation/1.0.0-draft.1";
const VALIDATION_VERSION: &str = "gkos-ingest-validation/1.0.0-draft.1";
const JOURNAL_VERSION: &str = "gkos-ingest-rejection-journal/1.0.0-draft.1";
const BUILTIN_SELECTOR: &str = "gkos:frontmatter-profile/current";
const BUILTIN_PROFILE_DIGEST: &str =
    "sha256:9ab3b07da4cdfb584c2766762a32dc71653dffd87537ad0a4c9190e3a69015c5";
const STANDARD_COMMIT: &str = "a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6";
const STANDARD_SCHEMA_DIGEST: &str =
    "sha256:eb25f75b4864a9130b2e27bdba2627e561fc79ab3a2537397bf5df024aab5ca3";
const STANDARD_DEFS_DIGEST: &str =
    "sha256:5b58edd93a53f6b821bfc03e2e4e3955da394bfaccb72e3ff194b45115c44c98";
const STANDARD_DIAGNOSTICS_DIGEST: &str =
    "sha256:178e4801d1274da60fe029d3f326b9adbdbf0e2a2033469928022fa107427403";
const ENGINE_PROFILE: &str = "gkx-2.3-validating-projection";
const ENGINE_POLICY_ID: &str = "policy:gkx23-default-v1";
const ENGINE_POLICY_HASH: &str =
    "sha256:2c2d8ec1e6481cbd4476bcc544c4fd19be03d8f21e317e44d889ea46e940ec8b";
const FULL_ENGINE_VERSION: &str = "2.1.2";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Frozen Full-owned envelope classes accepted by the verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestEnvelopeKind {
    NormalizedProfile,
    ValidationResult,
    Rejection,
    RejectionJournal,
    OwnerGeneration,
    ActivePointer,
    AttemptStatus,
    AuthorityLock,
    AuthorityWitness,
    ActivationRoot,
    LegacyTombstone,
    Migration,
    IndexResult,
}

/// Opaque, canonical, semantically verified Full-produced ingest envelope.
///
/// There is intentionally no public constructor and no mutable value access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIngestEnvelope {
    kind: IngestEnvelopeKind,
    value: Value,
    canonical: String,
}

/// Opaque cross-bound owner bundle. All three members have already passed
/// their individual seals and are additionally bound here to the same journal,
/// owner generation, and public-safe inner projection coordinate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIngestOwnerBundle {
    owner: VerifiedIngestEnvelope,
    journal: VerifiedIngestEnvelope,
    pointer: VerifiedIngestEnvelope,
}

impl VerifiedIngestEnvelope {
    pub fn kind(&self) -> IngestEnvelopeKind {
        self.kind
    }

    /// Canonical JSON with the one terminal LF required for persisted bytes.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = self.canonical.as_bytes().to_vec();
        bytes.push(b'\n');
        bytes
    }

    pub(crate) fn value(&self) -> &Value {
        &self.value
    }
}

/// Verify one Full-produced normalized profile without parsing its TOML source.
pub fn verify_normalized_ingest_profile(bytes: &[u8]) -> RetrievalResult<VerifiedIngestEnvelope> {
    let value = parse_json(bytes, 2_097_152)?;
    validate_normalized_profile(&value)?;
    seal(IngestEnvelopeKind::NormalizedProfile, value)
}

/// Verify one complete Full-produced validation result, including nested
/// profile, finding, rejection, observation, count, and predicate bindings.
pub fn verify_ingest_validation_result(bytes: &[u8]) -> RetrievalResult<VerifiedIngestEnvelope> {
    let value = parse_json(bytes, 536_870_912)?;
    validate_validation_result(&value)?;
    seal(IngestEnvelopeKind::ValidationResult, value)
}

/// Verify one standalone Full-produced safe rejection against an already
/// verified normalized-profile capability. The rejection carries its own
/// portable profile coordinate; no TOML or authored source is consulted.
pub fn verify_ingest_rejection(
    bytes: &[u8],
    normalized_profile: &VerifiedIngestEnvelope,
) -> RetrievalResult<VerifiedIngestEnvelope> {
    if normalized_profile.kind != IngestEnvelopeKind::NormalizedProfile {
        return Err(invalid(
            "standalone rejection requires a verified normalized profile",
        ));
    }
    let value = parse_json(bytes, 536_870_912)?;
    let object = as_object(&value, "rejection")?;
    let profile = field(object, "profile")?;
    validate_rejection(&value, normalized_profile.value(), profile)?;
    seal(IngestEnvelopeKind::Rejection, value)
}

/// Verify one Full-produced rejection journal and its normalized profile,
/// multiplicity, ordering, and digest bindings.
pub fn verify_ingest_rejection_journal(bytes: &[u8]) -> RetrievalResult<VerifiedIngestEnvelope> {
    let value = parse_json(bytes, 536_870_912)?;
    validate_rejection_journal(&value)?;
    seal(IngestEnvelopeKind::RejectionJournal, value)
}

/// Verify a Full-produced owner generation manifest. This checks the complete
/// validation/profile/journal coordinate and the opaque schema-3 inner
/// projection manifest, but never interprets candidate receipts or GKX.
pub fn verify_ingest_owner_generation(bytes: &[u8]) -> RetrievalResult<VerifiedIngestEnvelope> {
    let value = parse_json(bytes, 536_870_912)?;
    validate_owner_generation(&value)?;
    seal(IngestEnvelopeKind::OwnerGeneration, value)
}

/// Verify one of the Full-owned authority/state envelopes by its exact frozen
/// contract version. Index-result envelopes use the dedicated context-aware
/// verifier below.
pub fn verify_ingest_state_envelope(bytes: &[u8]) -> RetrievalResult<VerifiedIngestEnvelope> {
    let value = parse_json(bytes, 1_048_576)?;
    let root = as_object(&value, "ingest state envelope")?;
    let version = string(field(root, "contract_version")?, "contract_version")?;
    let kind = match version {
        "gkos-ingest-active-pointer/1.0.0-draft.1" => {
            validate_active_pointer(&value)?;
            IngestEnvelopeKind::ActivePointer
        }
        "gkos-ingest-attempt-status/1.0.0-draft.1" => {
            validate_attempt_status(&value)?;
            IngestEnvelopeKind::AttemptStatus
        }
        "gkos-ingest-authority-lock/1.0.0-draft.1" => {
            validate_authority_lock(&value)?;
            IngestEnvelopeKind::AuthorityLock
        }
        "gkos-ingest-authority-witness/1.0.0-draft.1" => {
            validate_witness(&value)?;
            IngestEnvelopeKind::AuthorityWitness
        }
        "gkos-ingest-activation-root/1.0.0-draft.1" => {
            validate_activation_root(&value)?;
            IngestEnvelopeKind::ActivationRoot
        }
        "gkos-ingest-legacy-pointer-tombstone/1.0.0-draft.1" => {
            validate_tombstone(&value)?;
            IngestEnvelopeKind::LegacyTombstone
        }
        "gkos-ingest-migration/1.0.0-draft.1" => {
            validate_migration(&value)?;
            IngestEnvelopeKind::Migration
        }
        _ => return Err(invalid("unknown Phase-3 ingest state contract version")),
    };
    seal(kind, value)
}

/// Verify the path-free public index result. A blocked-strict result must be
/// supplied with its already-verified attempt status so that active and digest
/// coordinates cannot be forged independently.
pub fn verify_ingest_index_result(
    bytes: &[u8],
    blocked_status: Option<&VerifiedIngestEnvelope>,
) -> RetrievalResult<VerifiedIngestEnvelope> {
    let value = parse_json(bytes, 1_048_576)?;
    let status = blocked_status
        .filter(|status| status.kind == IngestEnvelopeKind::AttemptStatus)
        .map(VerifiedIngestEnvelope::value);
    validate_index_result(&value, status)?;
    seal(IngestEnvelopeKind::IndexResult, value)
}

/// Cross-bind separately verified Full-produced owner, journal, and active
/// pointer envelopes. The returned capability cannot be constructed from raw
/// fields and contains no parser/profile/identity authority.
pub fn verify_ingest_owner_bundle(
    owner_bytes: &[u8],
    journal_bytes: &[u8],
    pointer_bytes: &[u8],
) -> RetrievalResult<VerifiedIngestOwnerBundle> {
    let owner = verify_ingest_owner_generation(owner_bytes)?;
    let journal = verify_ingest_rejection_journal(journal_bytes)?;
    let pointer = verify_ingest_state_envelope(pointer_bytes)?;
    if pointer.kind != IngestEnvelopeKind::ActivePointer {
        return Err(invalid("owner bundle pointer kind is invalid"));
    }
    validate_owner_bundle_values(owner.value(), journal.value(), pointer.value())?;
    Ok(VerifiedIngestOwnerBundle {
        owner,
        journal,
        pointer,
    })
}

fn seal(kind: IngestEnvelopeKind, value: Value) -> RetrievalResult<VerifiedIngestEnvelope> {
    let canonical = canonical_json(&value)?;
    Ok(VerifiedIngestEnvelope {
        kind,
        value,
        canonical,
    })
}

fn invalid(message: impl Into<String>) -> RetrievalError {
    RetrievalError::InvalidEnvelope(message.into())
}

fn parse_json(bytes: &[u8], maximum: usize) -> RetrievalResult<Value> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(invalid("Phase-3 ingest JSON byte length is invalid"));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    reject_negative_zero(&value)?;
    let canonical = canonical_json(&value)?;
    if bytes != canonical.as_bytes()
        && !(bytes.len() == canonical.len() + 1
            && bytes.last() == Some(&b'\n')
            && &bytes[..canonical.len()] == canonical.as_bytes())
    {
        return Err(invalid(
            "Phase-3 ingest JSON bytes are not canonical with at most one terminal LF",
        ));
    }
    Ok(value)
}

fn reject_negative_zero(value: &Value) -> RetrievalResult<()> {
    match value {
        Value::Number(number)
            if number.as_f64().is_some_and(|number| number == 0.0)
                && number.to_string().starts_with('-') =>
        {
            Err(invalid("Phase-3 canonical JSON rejects negative zero"))
        }
        Value::Array(values) => {
            for value in values {
                reject_negative_zero(value)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            for value in values.values() {
                reject_negative_zero(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn as_object<'a>(value: &'a Value, label: &str) -> RetrievalResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{label} must be an object")))
}

fn array<'a>(value: &'a Value, label: &str) -> RetrievalResult<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("{label} must be an array")))
}

fn string<'a>(value: &'a Value, label: &str) -> RetrievalResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| invalid(format!("{label} must be a string")))
}

fn boolean(value: &Value, label: &str) -> RetrievalResult<bool> {
    value
        .as_bool()
        .ok_or_else(|| invalid(format!("{label} must be a boolean")))
}

fn unsigned(value: &Value, maximum: u64, label: &str) -> RetrievalResult<u64> {
    value
        .as_u64()
        .filter(|number| *number <= maximum)
        .ok_or_else(|| {
            invalid(format!(
                "{label} must be a bounded nonnegative safe integer"
            ))
        })
}

fn exact_keys(object: &Map<String, Value>, expected: &[&str], label: &str) -> RetrievalResult<()> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(invalid(format!(
            "{label} fields do not match the frozen contract"
        )));
    }
    Ok(())
}

fn field<'a>(object: &'a Map<String, Value>, key: &str) -> RetrievalResult<&'a Value> {
    object
        .get(key)
        .ok_or_else(|| invalid(format!("missing required Phase-3 field {key}")))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 71
        && value.strip_prefix("sha256:").is_some_and(|hex| {
            hex.bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

fn is_profile_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(byte))
}

fn is_extension_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("x-") else {
        return false;
    };
    let bytes = rest.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(byte))
}

fn code_units(value: &str) -> usize {
    value.encode_utf16().count()
}

fn compare_strings(left: &str, right: &str) -> Ordering {
    code_unit_compare(left, right)
}

fn is_portable_source_path(value: &str) -> bool {
    code_units(value) <= 4_096
        && !value.chars().any(|character| character == '\u{007f}')
        && is_valid_retrieval_source_path(value)
}

fn is_safe_finding_field(value: &str) -> bool {
    if value.is_empty() || code_units(value) > 256 || !value.is_ascii() {
        return false;
    }
    let valid_head = |head: &str| {
        let bytes = head.as_bytes();
        bytes.first().is_some_and(u8::is_ascii_alphabetic)
            && bytes
                .iter()
                .skip(1)
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
    };
    let Some((head, suffix)) = value.split_once('[') else {
        return valid_head(value);
    };
    if !valid_head(head) || suffix.contains('[') {
        return false;
    }
    let Some((index, tail)) = suffix.split_once(']') else {
        return false;
    };
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    tail.is_empty() || tail.strip_prefix('.').is_some_and(valid_head)
}

fn sorted_unique_strings(values: &[Value], label: &str) -> RetrievalResult<Vec<String>> {
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        output.push(string(value, label)?.to_owned());
    }
    if output
        .windows(2)
        .any(|window| compare_strings(&window[0], &window[1]) != Ordering::Less)
    {
        return Err(invalid(format!("{label} must be UTF-16 sorted and unique")));
    }
    Ok(output)
}

fn baseline_profile() -> &'static Value {
    static BASELINE: OnceLock<Value> = OnceLock::new();
    BASELINE.get_or_init(|| {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).expect("frozen ingest fixture");
        fixture["executable"]["expected_result"]["normalized_profile"].clone()
    })
}

fn finding_codes() -> &'static BTreeSet<String> {
    static CODES: OnceLock<BTreeSet<String>> = OnceLock::new();
    CODES.get_or_init(|| {
        let schema: Value = serde_json::from_str(FINDING_SCHEMA).expect("frozen finding schema");
        schema["properties"]["code"]["enum"]
            .as_array()
            .expect("finding code enum")
            .iter()
            .map(|value| value.as_str().expect("finding code").to_owned())
            .collect()
    })
}

fn rank(values: &[&str], value: &str) -> Option<usize> {
    values.iter().position(|candidate| *candidate == value)
}

fn validate_normalized_profile(value: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "profile_id",
        "standard_commit",
        "standard_frontmatter_schema_sha256",
        "standard_common_defs_sha256",
        "standard_diagnostics_sha256",
        "engine_projection_profile",
        "engine_policy_id",
        "engine_policy_hash",
        "required_fields",
        "unknown_fields",
        "minimum_sensitivity",
        "identity_rules",
        "relationship_rules",
        "severity",
        "fields",
    ];
    let profile_object = as_object(value, "normalized profile")?;
    exact_keys(profile_object, KEYS, "normalized profile")?;
    let baseline = as_object(baseline_profile(), "built-in normalized profile")?;
    for (key, expected) in [
        ("contract_version", NORMALIZED_PROFILE_VERSION),
        ("standard_commit", STANDARD_COMMIT),
        ("standard_frontmatter_schema_sha256", STANDARD_SCHEMA_DIGEST),
        ("standard_common_defs_sha256", STANDARD_DEFS_DIGEST),
        ("standard_diagnostics_sha256", STANDARD_DIAGNOSTICS_DIGEST),
        ("engine_projection_profile", ENGINE_PROFILE),
        ("engine_policy_id", ENGINE_POLICY_ID),
        ("engine_policy_hash", ENGINE_POLICY_HASH),
    ] {
        if string(field(profile_object, key)?, key)? != expected {
            return Err(invalid(format!(
                "normalized profile {key} authority mismatch"
            )));
        }
    }
    if !is_profile_id(string(field(profile_object, "profile_id")?, "profile_id")?) {
        return Err(invalid("normalized profile_id is invalid"));
    }
    if field(profile_object, "identity_rules")? != field(baseline, "identity_rules")?
        || field(profile_object, "relationship_rules")? != field(baseline, "relationship_rules")?
    {
        return Err(invalid(
            "normalized profile changes canonical GKX authority",
        ));
    }
    let unknown_order = ["allow", "warn", "reject"];
    let sensitivity_order = [
        "public",
        "internal",
        "restricted",
        "confidential",
        "regulated",
        "phi",
        "secret",
    ];
    let unknown = string(field(profile_object, "unknown_fields")?, "unknown_fields")?;
    let minimum = string(
        field(profile_object, "minimum_sensitivity")?,
        "minimum_sensitivity",
    )?;
    if rank(&unknown_order, unknown).is_none()
        || rank(&unknown_order, unknown)
            < rank(
                &unknown_order,
                field(baseline, "unknown_fields")?.as_str().unwrap(),
            )
        || rank(&sensitivity_order, minimum).is_none()
        || rank(&sensitivity_order, minimum)
            < rank(
                &sensitivity_order,
                field(baseline, "minimum_sensitivity")?.as_str().unwrap(),
            )
    {
        return Err(invalid("normalized profile weakens a policy floor"));
    }
    validate_normalized_severity(profile_object, baseline)?;
    validate_normalized_fields(profile_object, baseline, minimum, &sensitivity_order)?;
    Ok(())
}

fn validate_normalized_severity(
    profile_object: &Map<String, Value>,
    baseline: &Map<String, Value>,
) -> RetrievalResult<()> {
    const KEYS: &[&str] = &["code", "severity"];
    const ORDER: &[&str] = &["info", "warning", "error", "critical"];
    const RAISEABLE: &[&str] = &[
        "GKX-AUTHORITY-ROLE-001",
        "GKX-AUTHORITY-ROLE-002",
        "GKX-EPISTEMIC-002",
        "GKX-EPISTEMIC-004",
        "GKX-EVIDENCE-002",
        "GKX-EVIDENCE-003",
        "GKX-IDENTITY-001",
        "GKX-IDENTITY-002",
        "GKX-PROVENANCE-001",
        "GKX-PROVENANCE-002",
        "GKX-SCHEMA-002",
        "GKX-SCHEMA-003",
        "GKX-SCHEMA-004",
        "GKX-SENSITIVITY-005",
        "GKX-TEMPORAL-001",
    ];
    let baseline_values = array(field(baseline, "severity")?, "baseline severity")?;
    let values = array(field(profile_object, "severity")?, "normalized severity")?;
    if values.len() != baseline_values.len() {
        return Err(invalid("normalized severity set is incomplete"));
    }
    for (index, value) in values.iter().enumerate() {
        let item = as_object(value, "normalized severity item")?;
        exact_keys(item, KEYS, "normalized severity item")?;
        let baseline_item = as_object(&baseline_values[index], "baseline severity item")?;
        let code = string(field(item, "code")?, "severity code")?;
        let baseline_code = string(field(baseline_item, "code")?, "baseline severity code")?;
        let severity = string(field(item, "severity")?, "severity")?;
        let floor = string(field(baseline_item, "severity")?, "baseline severity")?;
        if code != baseline_code
            || rank(ORDER, severity).is_none()
            || rank(ORDER, severity) < rank(ORDER, floor)
            || (!RAISEABLE.contains(&code) && severity != floor)
        {
            return Err(invalid("GKX_INGEST_NORMALIZED_PROFILE_SEVERITY_INVALID"));
        }
    }
    Ok(())
}

fn optional_bound(value: &Value, maximum: u64, label: &str) -> RetrievalResult<Option<u64>> {
    if value.is_null() {
        Ok(None)
    } else {
        unsigned(value, maximum, label).map(Some)
    }
}

fn validate_normalized_fields(
    profile_object: &Map<String, Value>,
    baseline: &Map<String, Value>,
    minimum_sensitivity: &str,
    sensitivity_order: &[&str],
) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "field",
        "type",
        "required",
        "min_length",
        "max_length",
        "integer_minimum",
        "integer_maximum",
        "array_max_items",
        "array_item_max_length",
        "enum",
        "extension",
    ];
    let baseline_values = array(field(baseline, "fields")?, "baseline fields")?;
    let mut baseline_by_name = BTreeMap::new();
    for value in baseline_values {
        let item = as_object(value, "baseline field")?;
        baseline_by_name.insert(string(field(item, "field")?, "baseline field name")?, item);
    }
    let values = array(field(profile_object, "fields")?, "normalized fields")?;
    if values.len() < baseline_values.len() || values.len() > 256 {
        return Err(invalid("normalized field set size is invalid"));
    }
    let mut names = Vec::with_capacity(values.len());
    let mut required_from_rows = Vec::new();
    let mut sensitivity_domain: Option<Vec<String>> = None;
    for value in values {
        let item = as_object(value, "normalized field")?;
        exact_keys(item, KEYS, "normalized field")?;
        let name = string(field(item, "field")?, "field name")?;
        let kind = string(field(item, "type")?, "field type")?;
        let required = boolean(field(item, "required")?, "field required")?;
        let extension = boolean(field(item, "extension")?, "field extension")?;
        let canonical = baseline_by_name.get(name).copied();
        if canonical.is_none() && !is_extension_id(name) {
            return Err(invalid("normalized extension field id is invalid"));
        }
        if extension != canonical.is_none()
            || !["string", "boolean", "integer", "array<string>"].contains(&kind)
        {
            return Err(invalid("normalized field authority/type is invalid"));
        }
        if let Some(canonical) = canonical {
            if string(field(canonical, "type")?, "canonical field type")? != kind
                || (boolean(field(canonical, "required")?, "canonical required")? && !required)
            {
                return Err(invalid("normalized field widens canonical requirements"));
            }
        }
        if name == "sensitivity" && required {
            return Err(invalid(
                "GKX_INGEST_NORMALIZED_PROFILE_SENSITIVITY_REQUIRED_INVALID",
            ));
        }
        let min_length = optional_bound(field(item, "min_length")?, 262_144, "min_length")?;
        let max_length = optional_bound(field(item, "max_length")?, 262_144, "max_length")?;
        let integer_minimum = optional_bound(
            field(item, "integer_minimum")?,
            2_147_483_647,
            "integer_minimum",
        )?;
        let integer_maximum = optional_bound(
            field(item, "integer_maximum")?,
            2_147_483_647,
            "integer_maximum",
        )?;
        let array_max_items =
            optional_bound(field(item, "array_max_items")?, 262_144, "array_max_items")?;
        let array_item_max_length = optional_bound(
            field(item, "array_item_max_length")?,
            262_144,
            "array_item_max_length",
        )?;
        if min_length
            .zip(max_length)
            .is_some_and(|(min, max)| min > max)
        {
            return Err(invalid("normalized string bounds are reversed"));
        }
        match kind {
            "string" => {
                if max_length.is_none()
                    || integer_minimum.is_some()
                    || integer_maximum.is_some()
                    || array_max_items.is_some()
                    || array_item_max_length.is_some()
                {
                    return Err(invalid("normalized string domain is invalid"));
                }
            }
            "boolean" => {
                if min_length.is_some()
                    || max_length.is_some()
                    || integer_minimum.is_some()
                    || integer_maximum.is_some()
                    || array_max_items.is_some()
                    || array_item_max_length.is_some()
                    || !field(item, "enum")?.is_null()
                {
                    return Err(invalid("normalized boolean domain is invalid"));
                }
            }
            "integer" => {
                if !extension
                    || integer_minimum != Some(0)
                    || integer_maximum != Some(2_147_483_647)
                    || min_length.is_some()
                    || max_length.is_some()
                    || array_max_items.is_some()
                    || array_item_max_length.is_some()
                    || !field(item, "enum")?.is_null()
                {
                    return Err(invalid("normalized integer domain is invalid"));
                }
            }
            "array<string>" => {
                let expected = if extension { 256 } else { 262_144 };
                let expected_length = if extension { 1_024 } else { 262_144 };
                if array_max_items != Some(expected)
                    || array_item_max_length != Some(expected_length)
                    || min_length.is_some()
                    || max_length.is_some()
                    || integer_minimum.is_some()
                    || integer_maximum.is_some()
                    || !field(item, "enum")?.is_null()
                {
                    return Err(invalid("normalized array domain is invalid"));
                }
            }
            _ => unreachable!(),
        }
        let enumeration = if field(item, "enum")?.is_null() {
            None
        } else {
            let entries = array(field(item, "enum")?, "field enum")?;
            if entries.is_empty() || entries.len() > 256 {
                return Err(invalid("normalized enum is empty or too large"));
            }
            let values = sorted_unique_strings(entries, "field enum")?;
            if values.iter().any(|value| {
                value.is_empty()
                    || code_units(value) > 512
                    || value
                        .chars()
                        .any(|character| character <= '\u{1f}' || character == '\u{7f}')
            }) {
                return Err(invalid("normalized enum item is invalid"));
            }
            Some(values)
        };
        if kind != "string" && enumeration.is_some() {
            return Err(invalid("only string fields may have enums"));
        }
        if required && kind == "string" && max_length == Some(0) {
            return Err(invalid("required string field domain is empty"));
        }
        if let Some(enumeration) = &enumeration {
            let within_bounds = |value: &str| {
                let length = code_units(value) as u64;
                min_length.is_none_or(|minimum| length >= minimum)
                    && max_length.is_none_or(|maximum| length <= maximum)
            };
            if !enumeration.iter().any(|value| within_bounds(value))
                || (required
                    && !enumeration.iter().any(|value| {
                        within_bounds(value) && !trim_ecmascript_whitespace(value).is_empty()
                    }))
            {
                return Err(invalid("normalized field enum domain is empty"));
            }
        }
        if let Some(canonical) = canonical {
            let canonical_min =
                optional_bound(field(canonical, "min_length")?, 262_144, "canonical min")?;
            let canonical_max =
                optional_bound(field(canonical, "max_length")?, 262_144, "canonical max")?;
            if canonical_min.is_some_and(|floor| min_length.is_none_or(|value| value < floor))
                || canonical_max
                    .is_some_and(|ceiling| max_length.is_none_or(|value| value > ceiling))
            {
                return Err(invalid("normalized field widens canonical bounds"));
            }
            let canonical_enum = field(canonical, "enum")?;
            if canonical_enum.is_array() {
                let allowed: BTreeSet<_> = canonical_enum
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                let Some(enumeration) = &enumeration else {
                    return Err(invalid("normalized field removes canonical enum"));
                };
                if enumeration
                    .iter()
                    .any(|value| !allowed.contains(value.as_str()))
                {
                    return Err(invalid("normalized field enum widens canonical domain"));
                }
            }
        }
        if name == "sensitivity" {
            sensitivity_domain = enumeration.clone();
        }
        if required {
            required_from_rows.push(name.to_owned());
        }
        names.push(name.to_owned());
    }
    if names
        .windows(2)
        .any(|window| compare_strings(&window[0], &window[1]) != Ordering::Less)
        || baseline_by_name
            .keys()
            .any(|name| !names.iter().any(|item| item == name))
    {
        return Err(invalid(
            "normalized fields are not complete UTF-16 sorted unique rows",
        ));
    }
    let required = sorted_unique_strings(
        array(field(profile_object, "required_fields")?, "required_fields")?,
        "required_fields",
    )?;
    if required != required_from_rows || required.iter().any(|field| field == "sensitivity") {
        return Err(invalid(
            "required_fields does not bind normalized field rows",
        ));
    }
    if let Some(domain) = sensitivity_domain {
        let floor = rank(sensitivity_order, minimum_sensitivity).unwrap();
        if !domain
            .iter()
            .any(|value| rank(sensitivity_order, value).is_some_and(|candidate| candidate >= floor))
        {
            return Err(invalid(
                "GKX_INGEST_NORMALIZED_PROFILE_SENSITIVITY_DOMAIN_EMPTY",
            ));
        }
    }
    Ok(())
}

fn validate_profile_coordinate(value: &Value, normalized: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "selector_id",
        "profile_id",
        "standard_commit",
        "standard_frontmatter_schema_sha256",
        "standard_common_defs_sha256",
        "standard_diagnostics_sha256",
        "engine_projection_profile",
        "engine_policy_id",
        "engine_policy_hash",
        "overlay_sha256",
        "effective_profile_digest",
    ];
    validate_normalized_profile(normalized)?;
    let coordinate = as_object(value, "profile coordinate")?;
    exact_keys(coordinate, KEYS, "profile coordinate")?;
    let normalized_object = as_object(normalized, "normalized profile")?;
    let selector = string(field(coordinate, "selector_id")?, "selector_id")?;
    let profile_id = string(field(coordinate, "profile_id")?, "profile_id")?;
    if string(field(coordinate, "contract_version")?, "contract_version")?
        != PROFILE_COORDINATE_VERSION
        || ![BUILTIN_SELECTOR, "operator-overlay"].contains(&selector)
        || profile_id != string(field(normalized_object, "profile_id")?, "profile_id")?
        || string(field(coordinate, "standard_commit")?, "standard_commit")? != STANDARD_COMMIT
        || string(
            field(coordinate, "standard_frontmatter_schema_sha256")?,
            "schema digest",
        )? != STANDARD_SCHEMA_DIGEST
        || string(
            field(coordinate, "standard_common_defs_sha256")?,
            "defs digest",
        )? != STANDARD_DEFS_DIGEST
        || string(
            field(coordinate, "standard_diagnostics_sha256")?,
            "diagnostics digest",
        )? != STANDARD_DIAGNOSTICS_DIGEST
        || string(
            field(coordinate, "engine_projection_profile")?,
            "engine profile",
        )? != ENGINE_PROFILE
        || string(field(coordinate, "engine_policy_id")?, "policy id")? != ENGINE_POLICY_ID
        || string(field(coordinate, "engine_policy_hash")?, "policy hash")? != ENGINE_POLICY_HASH
        || string(
            field(coordinate, "effective_profile_digest")?,
            "effective profile digest",
        )? != canonical_digest(normalized)?
    {
        return Err(invalid(
            "profile coordinate does not bind normalized authority",
        ));
    }
    match selector {
        BUILTIN_SELECTOR => {
            if profile_id != "gkos-current"
                || !field(coordinate, "overlay_sha256")?.is_null()
                || string(field(coordinate, "effective_profile_digest")?, "digest")?
                    != BUILTIN_PROFILE_DIGEST
            {
                return Err(invalid("built-in profile coordinate is invalid"));
            }
        }
        "operator-overlay" => {
            if !field(coordinate, "overlay_sha256")?
                .as_str()
                .is_some_and(is_sha256)
            {
                return Err(invalid("operator overlay coordinate is invalid"));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn digest_without(value: &Value, key: &str) -> RetrievalResult<String> {
    let mut material = as_object(value, "digest envelope")?.clone();
    material.remove(key);
    canonical_digest(&Value::Object(material))
}

fn validate_finding(value: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "finding_id",
        "code",
        "severity",
        "classification",
        "scope",
        "coordinate_basis",
        "source_path",
        "source_observation_ordinal",
        "line",
        "field",
        "deterministic",
    ];
    let object = as_object(value, "finding")?;
    exact_keys(object, KEYS, "finding")?;
    let code = string(field(object, "code")?, "finding code")?;
    let severity = string(field(object, "severity")?, "finding severity")?;
    let classification = string(field(object, "classification")?, "finding classification")?;
    let scope = string(field(object, "scope")?, "finding scope")?;
    let basis = string(field(object, "coordinate_basis")?, "coordinate basis")?;
    if string(field(object, "contract_version")?, "finding contract")? != FINDING_VERSION
        || !finding_codes().contains(code)
        || !["info", "warning", "error", "critical"].contains(&severity)
        || !["intrinsic", "cross_record_report_only"].contains(&classification)
        || !["file", "frontmatter", "field", "corpus"].contains(&scope)
        || ![
            "file_observation",
            "document_line",
            "frontmatter_field",
            "missing_field",
            "corpus",
        ]
        .contains(&basis)
        || !boolean(field(object, "deterministic")?, "deterministic")?
    {
        return Err(invalid(
            "finding value is outside the finite contract vocabulary",
        ));
    }
    let source_path = field(object, "source_path")?;
    let ordinal = field(object, "source_observation_ordinal")?;
    if source_path.is_null() {
        if !ordinal.is_null() {
            return Err(invalid(
                "corpus finding cannot carry an observation ordinal",
            ));
        }
    } else {
        let path = string(source_path, "finding source_path")?;
        if !is_portable_source_path(path) {
            return Err(invalid("finding source_path is nonportable"));
        }
        unsigned(ordinal, 999_999, "finding ordinal")?;
    }
    let line = field(object, "line")?;
    if !line.is_null() {
        let number = unsigned(line, 2_147_483_647, "finding line")?;
        if number == 0 {
            return Err(invalid("finding line must be positive"));
        }
    }
    let safe_field = field(object, "field")?;
    if let Some(value) = safe_field.as_str() {
        if !is_safe_finding_field(value) {
            return Err(invalid("finding field is not a bounded safe coordinate"));
        }
    } else if !safe_field.is_null() {
        return Err(invalid("finding field must be string or null"));
    }
    validate_finding_shape(object, code, severity, classification, scope, basis)?;
    let finding_id = string(field(object, "finding_id")?, "finding_id")?;
    if !is_sha256(finding_id) || digest_without(value, "finding_id")? != finding_id {
        return Err(invalid("finding_id does not bind safe finding fields"));
    }
    Ok(())
}

fn validate_finding_shape(
    finding: &Map<String, Value>,
    code: &str,
    severity: &str,
    classification: &str,
    scope: &str,
    basis: &str,
) -> RetrievalResult<()> {
    let path = field(finding, "source_path")?;
    let line = field(finding, "line")?;
    let safe_field = field(finding, "field")?;
    let exact_field = match code {
        "CANONICAL_SOURCE_UID_UNAVAILABLE"
        | "GKX-IDENTITY-001"
        | "GKX-IDENTITY-002"
        | "GKX_INGEST_UID_REQUIRED" => Some("uid"),
        "CANONICAL_VALIDITY_BINDING_MISMATCH" | "CANONICAL_VALIDITY_TIMESTAMP_NONPORTABLE" => {
            Some("created_at")
        }
        "GKX-EPISTEMIC-002" | "GKX-EPISTEMIC-004" => Some("epistemic.state"),
        "GKX-PROVENANCE-001" => Some("provenance.source_refs"),
        "GKX-PROVENANCE-002" => Some("provenance.content_hash"),
        "GKX-SCHEMA-002" | "GKX-SCHEMA-003" | "GKX_INGEST_PROFILE_VERSION_REQUIRED" => {
            Some("gkx_version")
        }
        "GKX-SENSITIVITY-001" | "GKX-SENSITIVITY-005" | "GKX_PROFILE_SENSITIVITY_BELOW_MINIMUM" => {
            Some("sensitivity.level")
        }
        _ => None,
    };
    if exact_field.is_some() && safe_field.as_str() != exact_field {
        return Err(invalid(
            "finding field does not match its frozen code binding",
        ));
    }
    if code == "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT" {
        if classification != "cross_record_report_only"
            || severity != "error"
            || scope != "corpus"
            || basis != "corpus"
            || !path.is_null()
            || !line.is_null()
            || !safe_field.is_null()
        {
            return Err(invalid("Decision-A conflict finding shape is invalid"));
        }
        return Ok(());
    }
    if classification != "intrinsic" || scope == "corpus" || basis == "corpus" || path.is_null() {
        return Err(invalid(
            "only the generic Decision-A conflict may be report-only",
        ));
    }
    const SCAN: &[&str] = &[
        "SOURCE_FILESYSTEM_ALIAS_REJECTED",
        "SOURCE_READ_FAILED",
        "SOURCE_SIZE_LIMIT_EXCEEDED",
        "SOURCE_SNAPSHOT_CHANGED_DURING_SCAN",
        "SOURCE_UTF8_INVALID",
    ];
    if SCAN.contains(&code) {
        return require_finding_coordinate(
            severity == "error" && scope == "file" && basis == "file_observation",
            line,
            safe_field,
            false,
            false,
            "scan finding",
        );
    }
    if code.starts_with("GKX_YAML_")
        || [
            "GKX_FRONTMATTER_LINE_LIMIT",
            "GKX_FRONTMATTER_SIZE_LIMIT",
            "GKX_FRONTMATTER_UNTERMINATED",
        ]
        .contains(&code)
    {
        return require_finding_coordinate(
            severity == "error" && scope == "frontmatter" && basis == "document_line",
            line,
            safe_field,
            true,
            false,
            "parser finding",
        );
    }
    if code == "GKX_FRONTMATTER_REQUIRED" {
        return require_finding_coordinate(
            severity == "error" && scope == "frontmatter" && basis == "missing_field",
            line,
            safe_field,
            false,
            false,
            "frontmatter-required finding",
        );
    }
    if code == "GKX_PROFILE_FIELD_REQUIRED" {
        return require_finding_coordinate(
            severity == "error" && scope == "field" && basis == "missing_field",
            line,
            safe_field,
            false,
            true,
            "profile-required finding",
        );
    }
    if [
        "GKX_INGEST_PROFILE_VERSION_REQUIRED",
        "GKX_INGEST_UID_REQUIRED",
        "CANONICAL_SOURCE_UID_UNAVAILABLE",
    ]
    .contains(&code)
    {
        let coordinate_ok = scope == "field"
            && matches!(basis, "missing_field" | "frontmatter_field")
            && (basis == "missing_field") == line.is_null();
        return require_finding_coordinate(
            severity == "error" && coordinate_ok,
            line,
            safe_field,
            basis == "frontmatter_field",
            true,
            "required identity finding",
        );
    }
    if [
        "AUTHORED_RELATIONSHIP_REFERENCE_INVALID",
        "CANONICAL_VALIDITY_BINDING_MISMATCH",
        "CANONICAL_VALIDITY_TIMESTAMP_NONPORTABLE",
    ]
    .contains(&code)
    {
        if code == "AUTHORED_RELATIONSHIP_REFERENCE_INVALID"
            && !safe_field
                .as_str()
                .is_some_and(valid_relationship_finding_field)
        {
            return Err(invalid("authored relationship finding field is invalid"));
        }
        return require_finding_coordinate(
            severity == "error" && scope == "field" && basis == "frontmatter_field",
            line,
            safe_field,
            true,
            true,
            "authored relationship/validity finding",
        );
    }
    if code == "AUTHORED_LINK_REFERENCE_INVALID" {
        return require_finding_coordinate(
            severity == "error" && scope == "file" && basis == "document_line",
            line,
            safe_field,
            true,
            false,
            "authored link finding",
        );
    }
    if [
        "CANONICAL_PROJECTION_INVALID",
        "CANONICAL_VALIDITY_REFERENCE_UNAVAILABLE",
    ]
    .contains(&code)
    {
        return require_finding_coordinate(
            severity == "error" && scope == "file" && basis == "file_observation",
            line,
            safe_field,
            false,
            false,
            "canonical projection finding",
        );
    }
    if [
        "GKX_PROFILE_ENUM_INVALID",
        "GKX_PROFILE_LENGTH_INVALID",
        "GKX_PROFILE_SENSITIVITY_BELOW_MINIMUM",
        "GKX_PROFILE_TYPE_INVALID",
    ]
    .contains(&code)
    {
        return require_finding_coordinate(
            severity == "error" && scope == "field" && basis == "frontmatter_field",
            line,
            safe_field,
            true,
            true,
            "profile field finding",
        );
    }
    if code == "GKX_PROFILE_UNKNOWN_FIELD" {
        return require_finding_coordinate(
            matches!(severity, "warning" | "error")
                && scope == "frontmatter"
                && basis == "frontmatter_field",
            line,
            safe_field,
            true,
            false,
            "unknown field finding",
        );
    }
    if code == "GKX_INGEST_CANONICAL_DIAGNOSTIC_UNMAPPED" {
        return require_finding_coordinate(
            severity == "error" && scope == "frontmatter" && basis == "file_observation",
            line,
            safe_field,
            false,
            false,
            "unmapped diagnostic finding",
        );
    }
    if code.starts_with("GKX-") {
        require_finding_coordinate(
            scope == "field"
                && matches!(basis, "frontmatter_field" | "missing_field")
                && (basis == "frontmatter_field") == !line.is_null(),
            line,
            safe_field,
            basis == "frontmatter_field",
            true,
            "canonical diagnostic finding",
        )?;
        let field = safe_field.as_str().unwrap();
        let valid = match code {
            "GKX-AUTHORITY-ROLE-001" => [
                "gkx_assignment.authority.may_approve",
                "gkx_assignment.authority.may_authorize_use",
                "gkx_assignment.authority.may_modify_originals",
                "gkx_assignment.authority.may_lower_sensitivity",
                "gkx_assignment.authority.may_promote_epistemic_state",
                "gkx_assignment.authority.may_change_authoritative_lineage",
            ]
            .contains(&field),
            "GKX-AUTHORITY-ROLE-002" => field == "gkx_assignment.output.write_mode",
            "GKX-EVIDENCE-002" => valid_evidence_field(field, "strength"),
            "GKX-EVIDENCE-003" => valid_evidence_field(field, "relevance"),
            "GKX-TEMPORAL-001" => matches!(field, "created_at" | "updated_at"),
            "GKX-SCHEMA-004" => [
                "gkx_version",
                "uid",
                "title",
                "type",
                "created_at",
                "authorship",
                "epistemic",
                "provenance",
                "relationships",
                "evidence",
                "lineage",
                "review",
                "assessment",
                "authorization",
                "labels",
                "sensitivity",
                "epistemic.state",
            ]
            .contains(&field),
            _ => true,
        };
        if !valid {
            return Err(invalid("canonical diagnostic finding field is invalid"));
        }
        return Ok(());
    }
    Err(invalid("finding code has no frozen coordinate rule"))
}

fn require_finding_coordinate(
    predicate: bool,
    line: &Value,
    safe_field: &Value,
    line_required: bool,
    field_required: bool,
    label: &str,
) -> RetrievalResult<()> {
    if !predicate || line_required != !line.is_null() || field_required != !safe_field.is_null() {
        return Err(invalid(format!("{label} coordinate is invalid")));
    }
    Ok(())
}

fn valid_relationship_finding_field(value: &str) -> bool {
    const FIELDS: &[&str] = &[
        "relationships",
        "supersedes",
        "superseded_by",
        "relationships.approved_by",
        "relationships.blocks",
        "relationships.cites",
        "relationships.contradicts",
        "relationships.depends_on",
        "relationships.derived_from",
        "relationships.derives_from",
        "relationships.documents",
        "relationships.extends",
        "relationships.fails_to_replicate",
        "relationships.generalizes",
        "relationships.governed_by",
        "relationships.has_part",
        "relationships.implements",
        "relationships.interprets",
        "relationships.narrows",
        "relationships.part_of",
        "relationships.quotes",
        "relationships.refines",
        "relationships.related_to",
        "relationships.replicates",
        "relationships.reviewed_by",
        "relationships.supports",
        "relationships.tests",
    ];
    let base = value
        .strip_suffix(']')
        .and_then(|prefix| prefix.rsplit_once('['))
        .filter(|(_, index)| {
            !index.is_empty()
                && index.bytes().all(|byte| byte.is_ascii_digit())
                && (*index == "0" || !index.starts_with('0'))
        })
        .map_or(value, |(base, _)| base);
    FIELDS.contains(&base)
}

fn valid_evidence_field(value: &str, member: &str) -> bool {
    ["evidence.supports[", "evidence.contradicts["]
        .iter()
        .any(|prefix| {
            value.strip_prefix(prefix).is_some_and(|suffix| {
                suffix
                    .strip_suffix(&format!("].{member}"))
                    .is_some_and(|index| {
                        !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
                    })
            })
        })
}

fn finding_floor(code: &str) -> Option<&'static str> {
    Some(match code {
        "GKX-AUTHORITY-ROLE-001" => "critical",
        "GKX-EPISTEMIC-004"
        | "GKX-IDENTITY-001"
        | "GKX-PROVENANCE-001"
        | "GKX-SCHEMA-003"
        | "GKX-SENSITIVITY-001"
        | "GKX-TEMPORAL-001"
        | "GKX_PROFILE_UNKNOWN_FIELD" => "warning",
        "GKX-SCHEMA-002" => "info",
        "AUTHORED_LINK_REFERENCE_INVALID"
        | "AUTHORED_RELATIONSHIP_REFERENCE_INVALID"
        | "CANONICAL_PROJECTION_INVALID"
        | "CANONICAL_SOURCE_UID_UNAVAILABLE"
        | "CANONICAL_VALIDITY_BINDING_MISMATCH"
        | "CANONICAL_VALIDITY_REFERENCE_UNAVAILABLE"
        | "CANONICAL_VALIDITY_TIMESTAMP_NONPORTABLE"
        | "GKX-AUTHORITY-ROLE-002"
        | "GKX-EPISTEMIC-002"
        | "GKX-EVIDENCE-002"
        | "GKX-EVIDENCE-003"
        | "GKX-IDENTITY-002"
        | "GKX-PROVENANCE-002"
        | "GKX-SCHEMA-004"
        | "GKX-SENSITIVITY-005"
        | "GKX_FRONTMATTER_LINE_LIMIT"
        | "GKX_FRONTMATTER_REQUIRED"
        | "GKX_FRONTMATTER_SIZE_LIMIT"
        | "GKX_FRONTMATTER_UNTERMINATED"
        | "GKX_INGEST_CANONICAL_DIAGNOSTIC_UNMAPPED"
        | "GKX_INGEST_PROFILE_VERSION_REQUIRED"
        | "GKX_INGEST_UID_REQUIRED"
        | "GKX_PROFILE_ENUM_INVALID"
        | "GKX_PROFILE_FIELD_REQUIRED"
        | "GKX_PROFILE_LENGTH_INVALID"
        | "GKX_PROFILE_SENSITIVITY_BELOW_MINIMUM"
        | "GKX_PROFILE_TYPE_INVALID"
        | "GKX_YAML_DUPLICATE_KEY"
        | "GKX_YAML_FEATURE_UNSUPPORTED"
        | "GKX_YAML_FLOW_INVALID"
        | "GKX_YAML_INDENT_TAB"
        | "GKX_YAML_KEY_UNSAFE"
        | "GKX_YAML_LIST_MAPPING_CONTINUATION"
        | "GKX_YAML_LIST_SCALAR_CONTINUATION"
        | "GKX_YAML_MAPPING_UNSUPPORTED"
        | "GKX_YAML_NESTING_LIMIT"
        | "GKX_YAML_NUMBER_NONFINITE"
        | "GKX_YAML_NUMBER_UNSUPPORTED"
        | "GKX_YAML_QUOTE_INVALID"
        | "GKX_YAML_TOP_LEVEL_INDENT"
        | "GKX_YAML_UNPARSED_CONTENT"
        | "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT"
        | "SOURCE_FILESYSTEM_ALIAS_REJECTED"
        | "SOURCE_READ_FAILED"
        | "SOURCE_SIZE_LIMIT_EXCEEDED"
        | "SOURCE_SNAPSHOT_CHANGED_DURING_SCAN"
        | "SOURCE_UTF8_INVALID" => "error",
        _ => return None,
    })
}

fn validate_finding_against_profile(value: &Value, normalized: &Value) -> RetrievalResult<()> {
    let finding = as_object(value, "finding")?;
    let profile = as_object(normalized, "normalized profile")?;
    let code = string(field(finding, "code")?, "finding code")?;
    let severity = string(field(finding, "severity")?, "finding severity")?;
    let expected = array(field(profile, "severity")?, "normalized severities")?
        .iter()
        .find_map(|entry| {
            let entry = entry.as_object()?;
            (entry.get("code")?.as_str()? == code)
                .then(|| entry.get("severity")?.as_str())
                .flatten()
        })
        .or_else(|| {
            if code == "GKX_PROFILE_UNKNOWN_FIELD" {
                match field(profile, "unknown_fields").ok()?.as_str()? {
                    "warn" => Some("warning"),
                    "reject" => Some("error"),
                    _ => None,
                }
            } else {
                finding_floor(code)
            }
        })
        .ok_or_else(|| invalid("finding is impossible under normalized profile"))?;
    if severity != expected {
        return Err(invalid(
            "finding severity does not equal normalized profile",
        ));
    }
    let safe_field = field(finding, "field")?.as_str();
    let fields = array(field(profile, "fields")?, "normalized fields")?;
    let rule = safe_field.and_then(|safe_field| {
        fields.iter().find(|entry| {
            entry["field"].as_str() == Some(safe_field)
                || (safe_field == "sensitivity.level"
                    && entry["field"].as_str() == Some("sensitivity"))
        })
    });
    match code {
        "GKX_PROFILE_FIELD_REQUIRED" => {
            let required = array(field(profile, "required_fields")?, "required_fields")?;
            if safe_field.is_none() || !required.iter().any(|field| field.as_str() == safe_field) {
                return Err(invalid("required-field finding has no normalized rule"));
            }
        }
        "GKX_PROFILE_ENUM_INVALID" if !rule.is_some_and(|rule| rule["enum"].is_array()) => {
            return Err(invalid("enum finding has no normalized enum rule"));
        }
        "GKX_PROFILE_LENGTH_INVALID"
            if !rule.is_some_and(|rule| {
                rule["type"] == "string"
                    && (rule["required"] == true
                        || !rule["min_length"].is_null()
                        || !rule["max_length"].is_null())
            }) =>
        {
            return Err(invalid("length finding has no normalized length rule"));
        }
        "GKX_PROFILE_SENSITIVITY_BELOW_MINIMUM" if safe_field != Some("sensitivity.level") => {
            return Err(invalid("sensitivity-floor finding coordinate is invalid"));
        }
        "GKX_PROFILE_SENSITIVITY_BELOW_MINIMUM"
            if field(profile, "minimum_sensitivity")?.as_str() == Some("public") =>
        {
            return Err(invalid(
                "sensitivity-floor finding is impossible at the public floor",
            ));
        }
        "GKX_PROFILE_TYPE_INVALID" if rule.is_none() => {
            return Err(invalid("type finding has no normalized field rule"));
        }
        _ => {}
    }
    Ok(())
}

fn finding_sort_key(value: &Value) -> RetrievalResult<(String, i64, u64, String, String, String)> {
    let object = as_object(value, "finding")?;
    Ok((
        field(object, "source_path")?
            .as_str()
            .unwrap_or("")
            .to_owned(),
        field(object, "source_observation_ordinal")?
            .as_i64()
            .unwrap_or(-1),
        field(object, "line")?.as_u64().unwrap_or(u64::MAX),
        string(field(object, "code")?, "code")?.to_owned(),
        field(object, "field")?.as_str().unwrap_or("").to_owned(),
        string(field(object, "finding_id")?, "finding_id")?.to_owned(),
    ))
}

fn compare_finding_values(left: &Value, right: &Value) -> RetrievalResult<Ordering> {
    let left = finding_sort_key(left)?;
    let right = finding_sort_key(right)?;
    Ok(compare_strings(&left.0, &right.0)
        .then(left.1.cmp(&right.1))
        .then(left.2.cmp(&right.2))
        .then_with(|| compare_strings(&left.3, &right.3))
        .then_with(|| compare_strings(&left.4, &right.4))
        .then_with(|| compare_strings(&left.5, &right.5)))
}

fn validate_rejection(value: &Value, normalized: &Value, profile: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "source_observation_ordinal",
        "source_path",
        "source_digest",
        "source_size_bytes",
        "canonical_assertion_time",
        "canonical_valid_from",
        "effective_sensitivity",
        "findings",
        "profile",
        "rejection_digest",
    ];
    let rejection_object = as_object(value, "rejection")?;
    exact_keys(rejection_object, KEYS, "rejection")?;
    if string(
        field(rejection_object, "contract_version")?,
        "rejection contract",
    )? != REJECTION_VERSION
        || string(
            field(rejection_object, "effective_sensitivity")?,
            "effective sensitivity",
        )? != "secret"
    {
        return Err(invalid("rejection coordinates are invalid"));
    }
    validate_profile_coordinate(field(rejection_object, "profile")?, normalized)?;
    if field(rejection_object, "profile")? != profile {
        return Err(invalid("rejection profile differs from validation profile"));
    }
    let ordinal = unsigned(
        field(rejection_object, "source_observation_ordinal")?,
        999_999,
        "rejection ordinal",
    )?;
    let path = string(field(rejection_object, "source_path")?, "rejection path")?;
    if !is_portable_source_path(path) {
        return Err(invalid("rejection path is nonportable"));
    }
    let digest = field(rejection_object, "source_digest")?;
    let size = field(rejection_object, "source_size_bytes")?;
    if !digest.is_null() && (!digest.as_str().is_some_and(is_sha256) || size.is_null()) {
        return Err(invalid("rejection source digest/size binding is invalid"));
    }
    if !size.is_null() {
        unsigned(size, 9_007_199_254_740_991, "rejection source size")?;
    }
    for key in ["canonical_assertion_time", "canonical_valid_from"] {
        let value = field(rejection_object, key)?;
        if !value.is_null() && !value.as_str().is_some_and(is_canonical_utc) {
            return Err(invalid(format!("rejection {key} is not canonical UTC")));
        }
    }
    if !field(rejection_object, "canonical_valid_from")?.is_null()
        && field(rejection_object, "canonical_valid_from")?
            != field(rejection_object, "canonical_assertion_time")?
    {
        return Err(invalid("rejection temporal binding is invalid"));
    }
    let findings = array(field(rejection_object, "findings")?, "rejection findings")?;
    if findings.is_empty() || findings.len() > 530_000 {
        return Err(invalid("rejection finding count is invalid"));
    }
    let mut ids = BTreeSet::new();
    let mut blocking = false;
    for (index, finding) in findings.iter().enumerate() {
        validate_finding(finding)?;
        validate_finding_against_profile(finding, normalized)?;
        let item = as_object(finding, "rejection finding")?;
        if string(field(item, "classification")?, "classification")? != "intrinsic"
            || field(item, "source_path")?.as_str() != Some(path)
            || field(item, "source_observation_ordinal")?.as_u64() != Some(ordinal)
            || !ids.insert(string(field(item, "finding_id")?, "finding_id")?.to_owned())
            || (index > 0
                && compare_finding_values(&findings[index - 1], finding)? != Ordering::Less)
        {
            return Err(invalid(
                "rejection findings are not an exact sorted source subset",
            ));
        }
        blocking |= matches!(
            field(item, "severity")?.as_str(),
            Some("error" | "critical")
        );
    }
    if !blocking {
        return Err(invalid("rejection has no intrinsic blocking finding"));
    }
    let rejection_digest = string(
        field(rejection_object, "rejection_digest")?,
        "rejection_digest",
    )?;
    if !is_sha256(rejection_digest)
        || digest_without(value, "rejection_digest")? != rejection_digest
    {
        return Err(invalid("rejection_digest does not bind rejection"));
    }
    Ok(())
}

fn is_canonical_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        })
        && crate::provenance::normalized_timestamp_millis(value).is_ok()
}

fn validate_observation(value: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "source_observation_ordinal",
        "source_path",
        "source_digest",
        "source_size_bytes",
        "classification",
        "finding_ids",
        "intrinsic_blocking_finding_ids",
    ];
    let object = as_object(value, "source observation")?;
    exact_keys(object, KEYS, "source observation")?;
    if string(field(object, "contract_version")?, "observation contract")? != OBSERVATION_VERSION
        || !["accepted", "rejected"]
            .contains(&string(field(object, "classification")?, "classification")?)
    {
        return Err(invalid("source observation value is invalid"));
    }
    unsigned(
        field(object, "source_observation_ordinal")?,
        999_999,
        "observation ordinal",
    )?;
    let path = string(field(object, "source_path")?, "observation path")?;
    if !is_portable_source_path(path) {
        return Err(invalid("observation path is nonportable"));
    }
    let digest = field(object, "source_digest")?;
    let size = field(object, "source_size_bytes")?;
    if !digest.is_null() && (!digest.as_str().is_some_and(is_sha256) || size.is_null()) {
        return Err(invalid("observation digest/size binding is invalid"));
    }
    if !size.is_null() {
        unsigned(size, 9_007_199_254_740_991, "observation size")?;
    }
    if field(object, "classification")?.as_str() == Some("accepted")
        && (digest.is_null() || size.is_null())
    {
        return Err(invalid(
            "accepted observation must bind sealed source bytes",
        ));
    }
    for key in ["finding_ids", "intrinsic_blocking_finding_ids"] {
        let values = array(field(object, key)?, key)?;
        let ids = sorted_unique_strings(values, key)?;
        if ids.len() > 530_000 || ids.iter().any(|value| !is_sha256(value)) {
            return Err(invalid(format!("observation {key} is invalid")));
        }
    }
    Ok(())
}

fn observation_coordinate(value: &Value) -> RetrievalResult<(String, u64)> {
    let object = as_object(value, "observation")?;
    Ok((
        string(field(object, "source_path")?, "source_path")?.to_owned(),
        unsigned(
            field(object, "source_observation_ordinal")?,
            999_999,
            "ordinal",
        )?,
    ))
}

fn validate_validation_result(value: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "status",
        "corpus_valid",
        "ingest_intrinsic_valid",
        "profile",
        "normalized_profile",
        "summary",
        "findings",
        "observations",
        "rejections",
    ];
    let root = as_object(value, "validation result")?;
    exact_keys(root, KEYS, "validation result")?;
    if string(field(root, "contract_version")?, "validation contract")? != VALIDATION_VERSION
        || !["valid", "invalid"].contains(&string(field(root, "status")?, "status")?)
    {
        return Err(invalid("validation result contract/status is invalid"));
    }
    let normalized = field(root, "normalized_profile")?;
    let profile = field(root, "profile")?;
    validate_normalized_profile(normalized)?;
    validate_profile_coordinate(profile, normalized)?;
    let findings = array(field(root, "findings")?, "validation findings")?;
    let observations = array(field(root, "observations")?, "validation observations")?;
    let rejections = array(field(root, "rejections")?, "validation rejections")?;
    if findings.len() > 1_000_000 || observations.len() > 1_000_000 || rejections.len() > 1_000_000
    {
        return Err(invalid("validation result collection exceeds frozen bound"));
    }
    let mut finding_by_id = BTreeMap::new();
    let mut finding_by_observation: BTreeMap<(String, u64), Vec<&Value>> = BTreeMap::new();
    let mut counts = [0_u64; 4];
    let mut blocking = 0_u64;
    let mut intrinsic_blocking = 0_u64;
    for (index, finding) in findings.iter().enumerate() {
        validate_finding(finding)?;
        validate_finding_against_profile(finding, normalized)?;
        if index > 0 && compare_finding_values(&findings[index - 1], finding)? != Ordering::Less {
            return Err(invalid("validation findings are not sorted and unique"));
        }
        let item = as_object(finding, "finding")?;
        let id = string(field(item, "finding_id")?, "finding_id")?.to_owned();
        if finding_by_id.insert(id, finding).is_some() {
            return Err(invalid("validation finding_id is duplicated"));
        }
        let severity = string(field(item, "severity")?, "severity")?;
        counts[["info", "warning", "error", "critical"]
            .iter()
            .position(|candidate| *candidate == severity)
            .unwrap()] += 1;
        if matches!(severity, "error" | "critical") {
            blocking += 1;
            if field(item, "classification")?.as_str() == Some("intrinsic") {
                intrinsic_blocking += 1;
            }
        }
        if let (Some(path), Some(ordinal)) = (
            field(item, "source_path")?.as_str(),
            field(item, "source_observation_ordinal")?.as_u64(),
        ) {
            finding_by_observation
                .entry((path.to_owned(), ordinal))
                .or_default()
                .push(finding);
        }
    }
    let mut observation_by_coordinate = BTreeMap::new();
    let mut next_ordinal = BTreeMap::<String, u64>::new();
    let mut valid_count = 0_u64;
    for (index, observation) in observations.iter().enumerate() {
        validate_observation(observation)?;
        let coordinate = observation_coordinate(observation)?;
        if index > 0 {
            let prior = observation_coordinate(&observations[index - 1])?;
            if compare_strings(&prior.0, &coordinate.0).then(prior.1.cmp(&coordinate.1))
                != Ordering::Less
            {
                return Err(invalid("validation observations are not sorted and unique"));
            }
        }
        let expected = next_ordinal.entry(coordinate.0.clone()).or_default();
        if coordinate.1 != *expected {
            return Err(invalid("observation ordinals are not contiguous per path"));
        }
        *expected += 1;
        let item = as_object(observation, "observation")?;
        let source_findings = finding_by_observation
            .get(&coordinate)
            .cloned()
            .unwrap_or_default();
        let mut ids: Vec<String> = source_findings
            .iter()
            .map(|item| {
                as_object(item, "finding").unwrap()["finding_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        ids.sort_by(|left, right| compare_strings(left, right));
        let mut blockers: Vec<String> = source_findings
            .iter()
            .filter(|item| {
                let item = as_object(item, "finding").unwrap();
                item["classification"] == "intrinsic"
                    && matches!(item["severity"].as_str(), Some("error" | "critical"))
            })
            .map(|item| {
                as_object(item, "finding").unwrap()["finding_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        blockers.sort_by(|left, right| compare_strings(left, right));
        let actual_ids: Vec<String> = array(field(item, "finding_ids")?, "finding_ids")?
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect();
        let actual_blockers: Vec<String> = array(
            field(item, "intrinsic_blocking_finding_ids")?,
            "blocking ids",
        )?
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect();
        let expected_class = if blockers.is_empty() {
            "accepted"
        } else {
            "rejected"
        };
        if actual_ids != ids
            || actual_blockers != blockers
            || field(item, "classification")?.as_str() != Some(expected_class)
        {
            return Err(invalid(
                "observation does not bind its exact finding partition",
            ));
        }
        valid_count += u64::from(expected_class == "accepted");
        observation_by_coordinate.insert(coordinate, observation);
    }
    if finding_by_observation
        .keys()
        .any(|coordinate| !observation_by_coordinate.contains_key(coordinate))
    {
        return Err(invalid("source finding has no matching observation"));
    }
    let mut rejection_coordinates = BTreeSet::new();
    for (index, rejection) in rejections.iter().enumerate() {
        validate_rejection(rejection, normalized, profile)?;
        if index > 0 && compare_rejections(&rejections[index - 1], rejection)? != Ordering::Less {
            return Err(invalid("validation rejections are not sorted and unique"));
        }
        let item = as_object(rejection, "rejection")?;
        let coordinate = (
            string(field(item, "source_path")?, "source_path")?.to_owned(),
            unsigned(
                field(item, "source_observation_ordinal")?,
                999_999,
                "ordinal",
            )?,
        );
        let observation = observation_by_coordinate
            .get(&coordinate)
            .ok_or_else(|| invalid("rejection has no matching observation"))?;
        let observation = as_object(observation, "observation")?;
        let actual_rejection_findings = array(field(item, "findings")?, "rejection findings")?;
        let expected_rejection_findings = finding_by_observation
            .get(&coordinate)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if !rejection_coordinates.insert(coordinate.clone())
            || field(observation, "classification")?.as_str() != Some("rejected")
            || field(item, "source_digest")? != field(observation, "source_digest")?
            || field(item, "source_size_bytes")? != field(observation, "source_size_bytes")?
            || actual_rejection_findings.len() != expected_rejection_findings.len()
            || actual_rejection_findings
                .iter()
                .zip(expected_rejection_findings)
                .any(|(actual, expected)| actual != *expected)
        {
            return Err(invalid("rejection does not bind its rejected observation"));
        }
    }
    if observation_by_coordinate
        .iter()
        .any(|(coordinate, observation)| {
            as_object(observation, "observation").unwrap()["classification"] == "rejected"
                && !rejection_coordinates.contains(coordinate)
        })
    {
        return Err(invalid("rejected observation is missing its one rejection"));
    }
    let summary = as_object(field(root, "summary")?, "validation summary")?;
    exact_keys(
        summary,
        &[
            "observed_source_count",
            "valid_source_count",
            "rejected_source_count",
            "findings",
        ],
        "validation summary",
    )?;
    let summary_findings = as_object(field(summary, "findings")?, "finding counts")?;
    exact_keys(
        summary_findings,
        &["info", "warning", "error", "critical"],
        "finding counts",
    )?;
    for (index, severity) in ["info", "warning", "error", "critical"].iter().enumerate() {
        if unsigned(field(summary_findings, severity)?, 1_000_000, severity)? != counts[index] {
            return Err(invalid("validation severity counts are not derived"));
        }
    }
    let observed = observations.len() as u64;
    let rejected = rejections.len() as u64;
    let corpus_valid = boolean(field(root, "corpus_valid")?, "corpus_valid")?;
    let intrinsic_valid = boolean(
        field(root, "ingest_intrinsic_valid")?,
        "ingest_intrinsic_valid",
    )?;
    if unsigned(
        field(summary, "observed_source_count")?,
        1_000_000,
        "observed",
    )? != observed
        || unsigned(field(summary, "valid_source_count")?, 1_000_000, "valid")? != valid_count
        || unsigned(
            field(summary, "rejected_source_count")?,
            1_000_000,
            "rejected",
        )? != rejected
        || corpus_valid != (blocking == 0)
        || intrinsic_valid != (intrinsic_blocking == 0)
        || intrinsic_valid != rejections.is_empty()
        || field(root, "status")?.as_str() != Some(if blocking == 0 { "valid" } else { "invalid" })
    {
        return Err(invalid(
            "validation predicates/counts are not derived from findings",
        ));
    }
    Ok(())
}

fn compare_rejections(left: &Value, right: &Value) -> RetrievalResult<Ordering> {
    let left = as_object(left, "rejection")?;
    let right = as_object(right, "rejection")?;
    Ok(compare_strings(
        string(field(left, "source_path")?, "source_path")?,
        string(field(right, "source_path")?, "source_path")?,
    )
    .then(
        field(left, "source_observation_ordinal")?
            .as_u64()
            .cmp(&field(right, "source_observation_ordinal")?.as_u64()),
    )
    .then_with(|| {
        compare_strings(
            field(left, "rejection_digest").unwrap().as_str().unwrap(),
            field(right, "rejection_digest").unwrap().as_str().unwrap(),
        )
    }))
}

fn validate_rejection_journal(value: &Value) -> RetrievalResult<()> {
    const KEYS: &[&str] = &[
        "contract_version",
        "observation_snapshot_digest",
        "profile",
        "normalized_profile",
        "rejection_count",
        "rejections",
        "rejection_journal_digest",
    ];
    let journal_object = as_object(value, "rejection journal")?;
    exact_keys(journal_object, KEYS, "rejection journal")?;
    if string(
        field(journal_object, "contract_version")?,
        "journal contract",
    )? != JOURNAL_VERSION
        || !field(journal_object, "observation_snapshot_digest")?
            .as_str()
            .is_some_and(is_sha256)
    {
        return Err(invalid("rejection journal coordinates are invalid"));
    }
    let normalized = field(journal_object, "normalized_profile")?;
    let profile = field(journal_object, "profile")?;
    validate_normalized_profile(normalized)?;
    validate_profile_coordinate(profile, normalized)?;
    let rejections = array(field(journal_object, "rejections")?, "journal rejections")?;
    if unsigned(
        field(journal_object, "rejection_count")?,
        1_000_000,
        "rejection_count",
    )? != rejections.len() as u64
    {
        return Err(invalid("journal rejection_count mismatch"));
    }
    let mut coordinates = BTreeSet::new();
    let mut digests = BTreeSet::new();
    for (index, rejection) in rejections.iter().enumerate() {
        validate_rejection(rejection, normalized, profile)?;
        if index > 0 && compare_rejections(&rejections[index - 1], rejection)? != Ordering::Less {
            return Err(invalid("GKX_INGEST_REJECTION_JOURNAL_MULTIPLICITY_INVALID"));
        }
        let item = as_object(rejection, "rejection")?;
        let coordinate = (
            field(item, "source_path")?.as_str().unwrap().to_owned(),
            field(item, "source_observation_ordinal")?.as_u64().unwrap(),
        );
        if !coordinates.insert(coordinate)
            || !digests.insert(
                field(item, "rejection_digest")?
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
        {
            return Err(invalid("GKX_INGEST_REJECTION_JOURNAL_MULTIPLICITY_INVALID"));
        }
    }
    let digest = string(
        field(journal_object, "rejection_journal_digest")?,
        "journal digest",
    )?;
    if !is_sha256(digest) || digest_without(value, "rejection_journal_digest")? != digest {
        return Err(invalid("rejection journal digest mismatch"));
    }
    Ok(())
}

fn digest_without_keys(value: &Value, keys: &[&str]) -> RetrievalResult<String> {
    let mut material = as_object(value, "digest envelope")?.clone();
    for key in keys {
        material.remove(*key);
    }
    canonical_digest(&Value::Object(material))
}

fn raw_canonical_digest(value: &Value) -> RetrievalResult<String> {
    let mut canonical = canonical_json(value)?.into_bytes();
    canonical.push(b'\n');
    Ok(sha256(canonical))
}

fn owner_id_matches(owner_id: &str, digest: &str) -> bool {
    is_sha256(digest)
        && owner_id == format!("ingest:{}", &digest["sha256:".len().."sha256:".len() + 24])
}

fn validate_active_projection(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "active inner projection")?;
    exact_keys(
        object,
        &[
            "database_file",
            "manifest_digest",
            "projection_id",
            "projection_digest",
        ],
        "active inner projection",
    )?;
    let manifest_digest = string(field(object, "manifest_digest")?, "manifest_digest")?;
    let projection_digest = string(field(object, "projection_digest")?, "projection_digest")?;
    let projection_id = string(field(object, "projection_id")?, "projection_id")?;
    let database_file = string(field(object, "database_file")?, "database_file")?;
    if !is_sha256(manifest_digest)
        || !is_sha256(projection_digest)
        || projection_id
            != format!(
                "retrieval:{}",
                &projection_digest["sha256:".len().."sha256:".len() + 24]
            )
        || database_file != format!("retrieval-{}.sqlite", &projection_digest["sha256:".len()..])
    {
        return Err(invalid("active inner projection binding is invalid"));
    }
    Ok(())
}

fn validate_active_pointer(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "active ingest pointer")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "owner_generation_file",
            "owner_generation_id",
            "owner_manifest_digest",
            "inner",
        ],
        "active ingest pointer",
    )?;
    let digest = string(
        field(object, "owner_manifest_digest")?,
        "owner manifest digest",
    )?;
    let owner_id = string(field(object, "owner_generation_id")?, "owner generation id")?;
    if string(field(object, "contract_version")?, "pointer contract")?
        != "gkos-ingest-active-pointer/1.0.0-draft.1"
        || !owner_id_matches(owner_id, digest)
        || field(object, "owner_generation_file")?.as_str()
            != Some(&format!(
                "ingest-generation-{}.json",
                &digest["sha256:".len()..]
            ))
    {
        return Err(invalid("GKX_INGEST_ACTIVE_POINTER_BINDING_INVALID"));
    }
    validate_active_projection(field(object, "inner")?)
}

fn active_pointer_from_owner(owner: &Value) -> RetrievalResult<Value> {
    let owner = as_object(owner, "owner generation")?;
    let inner = as_object(field(owner, "inner")?, "owner inner")?;
    let manifest = as_object(field(inner, "manifest")?, "inner manifest")?;
    let value = serde_json::json!({
        "contract_version": "gkos-ingest-active-pointer/1.0.0-draft.1",
        "owner_generation_file": format!(
            "ingest-generation-{}.json",
            &field(owner, "owner_manifest_digest")?.as_str().unwrap()["sha256:".len()..]
        ),
        "owner_generation_id": field(owner, "owner_generation_id")?,
        "owner_manifest_digest": field(owner, "owner_manifest_digest")?,
        "inner": {
            "database_file": field(inner, "database_file")?,
            "manifest_digest": field(inner, "manifest_digest")?,
            "projection_id": field(manifest, "projection_id")?,
            "projection_digest": field(manifest, "projection_digest")?,
        },
    });
    validate_active_pointer(&value)?;
    Ok(value)
}

fn validate_owner_bundle_values(
    owner: &Value,
    journal: &Value,
    pointer: &Value,
) -> RetrievalResult<()> {
    validate_owner_generation(owner)?;
    validate_rejection_journal(journal)?;
    validate_active_pointer(pointer)?;
    let owner_object = as_object(owner, "owner generation")?;
    let journal_object = as_object(journal, "rejection journal")?;
    let expected_pointer = active_pointer_from_owner(owner)?;
    let coordinate = as_object(
        field(owner_object, "rejection_journal")?,
        "journal coordinate",
    )?;
    let result = as_object(
        field(owner_object, "validation_result")?,
        "validation result",
    )?;
    if pointer != &expected_pointer
        || field(coordinate, "rejection_journal_digest")?
            != field(journal_object, "rejection_journal_digest")?
        || field(coordinate, "rejection_count")? != field(journal_object, "rejection_count")?
        || field(owner_object, "observation_snapshot_digest")?
            != field(journal_object, "observation_snapshot_digest")?
        || field(owner_object, "profile")? != field(journal_object, "profile")?
        || field(owner_object, "normalized_profile")?
            != field(journal_object, "normalized_profile")?
        || field(result, "rejections")? != field(journal_object, "rejections")?
    {
        return Err(invalid("owner bundle journal/pointer binding is invalid"));
    }
    Ok(())
}

fn derived_active_pointer_digest(
    owner_id: &str,
    digest: &str,
    inner: &Value,
) -> RetrievalResult<String> {
    let value = serde_json::json!({
        "contract_version": "gkos-ingest-active-pointer/1.0.0-draft.1",
        "inner": inner,
        "owner_generation_file": format!("ingest-generation-{}.json", &digest["sha256:".len()..]),
        "owner_generation_id": owner_id,
        "owner_manifest_digest": digest,
    });
    validate_active_pointer(&value)?;
    raw_canonical_digest(&value)
}

fn validate_prior_active(value: &Value) -> RetrievalResult<()> {
    if value.is_null() {
        return Ok(());
    }
    let object = as_object(value, "prior_active")?;
    match field(object, "kind")?.as_str() {
        Some("legacy") => {
            exact_keys(
                object,
                &[
                    "kind",
                    "projection_id",
                    "projection_digest",
                    "pointer_digest",
                ],
                "legacy prior_active",
            )?;
            let digest = string(field(object, "projection_digest")?, "projection_digest")?;
            if !is_sha256(digest)
                || field(object, "projection_id")?.as_str()
                    != Some(&format!(
                        "retrieval:{}",
                        &digest["sha256:".len().."sha256:".len() + 24]
                    ))
                || !field(object, "pointer_digest")?
                    .as_str()
                    .is_some_and(is_sha256)
            {
                return Err(invalid("legacy prior_active binding is invalid"));
            }
        }
        Some("ingest") => {
            exact_keys(
                object,
                &[
                    "kind",
                    "owner_generation_id",
                    "owner_manifest_digest",
                    "inner",
                    "pointer_digest",
                ],
                "ingest prior_active",
            )?;
            let owner_id = string(field(object, "owner_generation_id")?, "owner id")?;
            let digest = string(field(object, "owner_manifest_digest")?, "owner digest")?;
            let inner = field(object, "inner")?;
            validate_active_projection(inner)?;
            if !owner_id_matches(owner_id, digest)
                || field(object, "pointer_digest")?.as_str()
                    != Some(derived_active_pointer_digest(owner_id, digest, inner)?.as_str())
            {
                return Err(invalid("ingest prior_active binding is invalid"));
            }
        }
        _ => return Err(invalid("prior_active kind is invalid")),
    }
    Ok(())
}

fn validate_attempt_status(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "blocked attempt status")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "state",
            "availability",
            "prior_active",
            "attempt_digest",
            "effective_profile_digest",
            "observation_snapshot_digest",
            "status_digest",
        ],
        "blocked attempt status",
    )?;
    let prior = field(object, "prior_active")?;
    validate_prior_active(prior)?;
    let expected_availability = if prior.is_null() {
        "unavailable"
    } else {
        "stale"
    };
    for key in [
        "attempt_digest",
        "effective_profile_digest",
        "observation_snapshot_digest",
        "status_digest",
    ] {
        if !field(object, key)?.as_str().is_some_and(is_sha256) {
            return Err(invalid(format!("blocked attempt {key} is invalid")));
        }
    }
    if field(object, "contract_version")?.as_str()
        != Some("gkos-ingest-attempt-status/1.0.0-draft.1")
        || field(object, "state")?.as_str() != Some("blocked")
        || field(object, "availability")?.as_str() != Some(expected_availability)
    {
        return Err(invalid("blocked attempt status shape is invalid"));
    }
    let attempt_material = serde_json::json!({
        "contract_version": "gkos-ingest-attempt/1.0.0-draft.1",
        "mode": "strict",
        "observation_snapshot_digest": field(object, "observation_snapshot_digest")?,
        "effective_profile_digest": field(object, "effective_profile_digest")?,
    });
    if field(object, "attempt_digest")?.as_str()
        != Some(canonical_digest(&attempt_material)?.as_str())
        || field(object, "status_digest")?.as_str()
            != Some(digest_without(value, "status_digest")?.as_str())
    {
        return Err(invalid("GKX_INGEST_ATTEMPT_STATUS_ATTEMPT_DIGEST_INVALID"));
    }
    Ok(())
}

pub(crate) fn validate_retrieval_manifest(
    value: &Value,
    require_schema_three: bool,
) -> RetrievalResult<String> {
    let object = as_object(value, "retrieval projection manifest")?;
    let contract = string(field(object, "contract_version")?, "manifest contract")?;
    let schema_version = unsigned(
        field(object, "projection_schema_version")?,
        MAX_SAFE_INTEGER,
        "projection_schema_version",
    )?;
    let schema_three = contract == RETRIEVAL_LINEAGE_CONTRACT
        && schema_version == u64::from(LINEAGE_PROJECTION_SCHEMA_VERSION);
    if require_schema_three && !schema_three {
        return Err(invalid("inner retrieval manifest is not schema-3"));
    }
    if schema_three {
        exact_keys(
            object,
            &[
                "candidate_chunk_count",
                "candidate_declaration_count",
                "candidate_source_count",
                "chunker_version",
                "configuration_digest",
                "contract_version",
                "embedding_dimensions",
                "embedding_eligible_candidate_chunk_count",
                "embedding_model_id",
                "embedding_provider_id",
                "engine_version",
                "gkx_projection_profile",
                "gkx_standard_commit",
                "lexical_backend",
                "policy_digest",
                "projection_digest",
                "projection_id",
                "projection_schema_version",
                "provenance_contract_version",
                "represented_candidate_source_count",
                "source_snapshot_digest",
                "tokenizer_version",
                "vault_id",
            ],
            "schema-3 retrieval manifest",
        )?;
        if field(object, "provenance_contract_version")?.as_str()
            != Some(RETRIEVAL_PROVENANCE_CONTRACT)
            || field(object, "gkx_standard_commit")?.as_str() != Some(GKX_STANDARD_COMMIT)
            || field(object, "gkx_projection_profile")?.as_str() != Some(GKX_PROJECTION_PROFILE)
        {
            return Err(invalid(
                "schema-3 retrieval authority coordinate is invalid",
            ));
        }
        let candidate_sources = unsigned(
            field(object, "candidate_source_count")?,
            MAX_SAFE_INTEGER,
            "candidate_source_count",
        )?;
        unsigned(
            field(object, "candidate_declaration_count")?,
            MAX_SAFE_INTEGER,
            "candidate_declaration_count",
        )?;
        let represented = unsigned(
            field(object, "represented_candidate_source_count")?,
            MAX_SAFE_INTEGER,
            "represented_candidate_source_count",
        )?;
        let chunks = unsigned(
            field(object, "candidate_chunk_count")?,
            MAX_SAFE_INTEGER,
            "candidate_chunk_count",
        )?;
        let eligible = unsigned(
            field(object, "embedding_eligible_candidate_chunk_count")?,
            MAX_SAFE_INTEGER,
            "embedding_eligible_candidate_chunk_count",
        )?;
        if represented > candidate_sources || eligible > chunks {
            return Err(invalid(
                "schema-3 retrieval manifest count binding is invalid",
            ));
        }
    } else {
        exact_keys(
            object,
            &[
                "chunk_count",
                "chunker_version",
                "configuration_digest",
                "contract_version",
                "embedding_dimensions",
                "embedding_model_id",
                "embedding_provider_id",
                "engine_version",
                "lexical_backend",
                "policy_digest",
                "projection_digest",
                "projection_id",
                "projection_schema_version",
                "source_count",
                "source_snapshot_digest",
                "tokenizer_version",
                "vault_id",
            ],
            "schema-2 retrieval manifest",
        )?;
        if contract != RETRIEVAL_CONTRACT || schema_version != u64::from(PROJECTION_SCHEMA_VERSION)
        {
            return Err(invalid("schema-2 retrieval contract coordinate is invalid"));
        }
        unsigned(
            field(object, "source_count")?,
            MAX_SAFE_INTEGER,
            "source_count",
        )?;
        unsigned(
            field(object, "chunk_count")?,
            MAX_SAFE_INTEGER,
            "chunk_count",
        )?;
    }
    if field(object, "chunker_version")?.as_str() != Some(CHUNKER_VERSION)
        || field(object, "tokenizer_version")?.as_str() != Some(TOKENIZER_VERSION)
        || !matches!(
            field(object, "lexical_backend")?.as_str(),
            Some("sqlite_fts5" | "sqlite_lexical_scan")
        )
        || field(object, "engine_version")?.as_str() != Some(FULL_ENGINE_VERSION)
    {
        return Err(invalid("retrieval manifest runtime coordinate is invalid"));
    }
    let vault_id = string(field(object, "vault_id")?, "manifest vault_id")?;
    if vault_id.is_empty() || code_units(vault_id) > 512 {
        return Err(invalid("retrieval manifest vault identity is invalid"));
    }
    for key in [
        "source_snapshot_digest",
        "configuration_digest",
        "policy_digest",
        "projection_digest",
    ] {
        if !field(object, key)?.as_str().is_some_and(is_sha256) {
            return Err(invalid(format!("retrieval manifest {key} is invalid")));
        }
    }
    let provider = field(object, "embedding_provider_id")?;
    let model = field(object, "embedding_model_id")?;
    let dimensions = field(object, "embedding_dimensions")?;
    if !provider.is_null() && !provider.is_string() || !model.is_null() && !model.is_string() {
        return Err(invalid("retrieval vector identity type is invalid"));
    }
    let has_vector = !provider.is_null() || !model.is_null() || !dimensions.is_null();
    if has_vector
        && (provider.as_str().is_none_or(str::is_empty)
            || model.as_str().is_none_or(str::is_empty)
            || unsigned(dimensions, MAX_SAFE_INTEGER, "embedding_dimensions")? == 0)
    {
        return Err(invalid("retrieval vector identity is incomplete"));
    }
    if !has_vector && !dimensions.is_null() {
        return Err(invalid("retrieval vector identity is incomplete"));
    }
    let projection_digest = string(field(object, "projection_digest")?, "projection digest")?;
    if field(object, "projection_id")?.as_str()
        != Some(&format!(
            "retrieval:{}",
            &projection_digest["sha256:".len().."sha256:".len() + 24]
        ))
    {
        return Err(invalid("retrieval projection id is invalid"));
    }
    Ok(projection_digest.to_owned())
}

fn validate_owner_generation(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "owner generation")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "owner_generation_id",
            "owner_manifest_digest",
            "mode",
            "vault_id",
            "observation_snapshot_digest",
            "profile",
            "normalized_profile",
            "configuration_digest",
            "policy_digest",
            "chunking",
            "validation_result",
            "inner",
            "rejection_journal",
        ],
        "owner generation",
    )?;
    let mode = string(field(object, "mode")?, "owner mode")?;
    let vault_id = string(field(object, "vault_id")?, "vault_id")?;
    let owner_digest = string(field(object, "owner_manifest_digest")?, "owner digest")?;
    let owner_id = string(field(object, "owner_generation_id")?, "owner id")?;
    if field(object, "contract_version")?.as_str() != Some("gkos-ingest-generation/1.0.0-draft.1")
        || !["strict", "non_strict"].contains(&mode)
        || vault_id.is_empty()
        || code_units(vault_id) > 512
        || !owner_id_matches(owner_id, owner_digest)
    {
        return Err(invalid("owner generation coordinate is invalid"));
    }
    for key in [
        "observation_snapshot_digest",
        "configuration_digest",
        "policy_digest",
    ] {
        if !field(object, key)?.as_str().is_some_and(is_sha256) {
            return Err(invalid(format!("owner generation {key} is invalid")));
        }
    }
    let chunking = as_object(field(object, "chunking")?, "chunking")?;
    exact_keys(
        chunking,
        &[
            "chunker_version",
            "tokenizer_version",
            "max_tokens",
            "overlap_tokens",
        ],
        "chunking",
    )?;
    let maximum = unsigned(field(chunking, "max_tokens")?, 4096, "max_tokens")?;
    let overlap = unsigned(field(chunking, "overlap_tokens")?, 4095, "overlap_tokens")?;
    if field(chunking, "chunker_version")?.as_str() != Some("gkos-heading-chunker/1")
        || field(chunking, "tokenizer_version")?.as_str() != Some("gkos-ascii-whitespace/1")
        || maximum < 16
        || overlap >= maximum
    {
        return Err(invalid("owner chunking coordinate is invalid"));
    }
    let normalized = field(object, "normalized_profile")?;
    let profile = field(object, "profile")?;
    let result = field(object, "validation_result")?;
    validate_normalized_profile(normalized)?;
    validate_profile_coordinate(profile, normalized)?;
    validate_validation_result(result)?;
    let result_object = as_object(result, "validation result")?;
    if field(result_object, "profile")? != profile
        || field(result_object, "normalized_profile")? != normalized
        || (mode == "strict"
            && !boolean(
                field(result_object, "ingest_intrinsic_valid")?,
                "ingest_intrinsic_valid",
            )?)
    {
        return Err(invalid(
            "owner generation profile/result binding is invalid",
        ));
    }
    let expected_snapshot = canonical_digest(&serde_json::json!({
        "contract_version": VALIDATION_VERSION,
        "effective_profile_digest": field(as_object(profile, "profile")?, "effective_profile_digest")?,
        "sources": field(result_object, "observations")?,
    }))?;
    if field(object, "observation_snapshot_digest")?.as_str() != Some(expected_snapshot.as_str()) {
        return Err(invalid("owner observation snapshot binding is invalid"));
    }
    let inner = as_object(field(object, "inner")?, "owner inner")?;
    exact_keys(
        inner,
        &["database_file", "manifest", "manifest_digest"],
        "owner inner",
    )?;
    let inner_manifest_value = field(inner, "manifest")?;
    let projection_digest = validate_retrieval_manifest(inner_manifest_value, true)?;
    let inner_manifest = as_object(inner_manifest_value, "inner schema-3 manifest")?;
    let inner_manifest_digest = canonical_digest(inner_manifest_value)?;
    if field(inner, "database_file")?.as_str()
        != Some(&format!(
            "retrieval-{}.sqlite",
            &projection_digest["sha256:".len()..]
        ))
        || field(inner, "manifest_digest")?.as_str() != Some(inner_manifest_digest.as_str())
        || field(inner_manifest, "source_snapshot_digest")?
            != field(object, "observation_snapshot_digest")?
        || field(inner_manifest, "vault_id")?.as_str() != Some(vault_id)
        || field(inner_manifest, "configuration_digest")? != field(object, "configuration_digest")?
        || field(inner_manifest, "policy_digest")? != field(object, "policy_digest")?
        || field(inner_manifest, "candidate_source_count")?
            .as_u64()
            .unwrap()
            != field(
                as_object(field(result_object, "summary")?, "summary")?,
                "valid_source_count",
            )?
            .as_u64()
            .unwrap()
    {
        return Err(invalid("owner inner projection binding is invalid"));
    }
    let journal_coordinate = as_object(field(object, "rejection_journal")?, "journal coordinate")?;
    exact_keys(
        journal_coordinate,
        &[
            "journal_file",
            "rejection_journal_digest",
            "rejection_count",
        ],
        "journal coordinate",
    )?;
    let journal_material = serde_json::json!({
        "contract_version": JOURNAL_VERSION,
        "observation_snapshot_digest": field(object, "observation_snapshot_digest")?,
        "profile": profile,
        "normalized_profile": normalized,
        "rejection_count": array(field(result_object, "rejections")?, "rejections")?.len(),
        "rejections": field(result_object, "rejections")?,
    });
    let journal_digest = canonical_digest(&journal_material)?;
    if field(journal_coordinate, "rejection_journal_digest")?.as_str()
        != Some(journal_digest.as_str())
        || field(journal_coordinate, "journal_file")?.as_str()
            != Some(&format!(
                "ingest-rejections-{}.json",
                &journal_digest["sha256:".len()..]
            ))
        || field(journal_coordinate, "rejection_count")?.as_u64()
            != Some(array(field(result_object, "rejections")?, "rejections")?.len() as u64)
    {
        return Err(invalid("GKX_INGEST_OWNER_MANIFEST_JOURNAL_BINDING_INVALID"));
    }
    let expected_owner_digest =
        digest_without_keys(value, &["owner_generation_id", "owner_manifest_digest"])?;
    if owner_digest != expected_owner_digest {
        return Err(invalid("owner manifest digest binding is invalid"));
    }
    Ok(())
}

fn validate_tombstone(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "legacy tombstone")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "target_owner_generation_id",
            "target_owner_manifest_digest",
            "migration_file",
            "migration_digest",
            "tombstone_digest",
        ],
        "legacy tombstone",
    )?;
    let owner_id = string(
        field(object, "target_owner_generation_id")?,
        "target owner id",
    )?;
    let owner_digest = string(
        field(object, "target_owner_manifest_digest")?,
        "target owner digest",
    )?;
    let migration_digest = string(field(object, "migration_digest")?, "migration digest")?;
    if field(object, "contract_version")?.as_str()
        != Some("gkos-ingest-legacy-pointer-tombstone/1.0.0-draft.1")
        || !owner_id_matches(owner_id, owner_digest)
        || !is_sha256(migration_digest)
        || field(object, "migration_file")?.as_str()
            != Some(&format!(
                "ingest-migration-{}.json",
                &migration_digest["sha256:".len()..]
            ))
        || field(object, "tombstone_digest")?.as_str()
            != Some(digest_without(value, "tombstone_digest")?.as_str())
    {
        return Err(invalid("legacy tombstone binding is invalid"));
    }
    Ok(())
}

fn validate_legacy_pointer(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "legacy retrieval pointer")?;
    exact_keys(
        object,
        &["database_file", "manifest"],
        "legacy retrieval pointer",
    )?;
    let manifest_value = field(object, "manifest")?;
    let projection_digest = validate_retrieval_manifest(manifest_value, false)?;
    if field(object, "database_file")?.as_str()
        != Some(&format!(
            "retrieval-{}.sqlite",
            &projection_digest["sha256:".len()..]
        ))
    {
        return Err(invalid("GKX_INGEST_LEGACY_POINTER_BINDING_INVALID"));
    }
    Ok(())
}

fn validate_migration(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "ingest migration")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "target_owner_generation_id",
            "target_owner_manifest_digest",
            "legacy_pointer",
            "legacy_pointer_digest",
            "migration_digest",
        ],
        "ingest migration",
    )?;
    let owner_id = string(
        field(object, "target_owner_generation_id")?,
        "target owner id",
    )?;
    let owner_digest = string(
        field(object, "target_owner_manifest_digest")?,
        "target owner digest",
    )?;
    if field(object, "contract_version")?.as_str() != Some("gkos-ingest-migration/1.0.0-draft.1")
        || !owner_id_matches(owner_id, owner_digest)
    {
        return Err(invalid("ingest migration owner binding is invalid"));
    }
    let legacy = field(object, "legacy_pointer")?;
    let legacy_digest = field(object, "legacy_pointer_digest")?;
    if legacy.is_null() {
        if !legacy_digest.is_null() {
            return Err(invalid("migration null legacy pointer has a digest"));
        }
    } else {
        validate_legacy_pointer(legacy)?;
        if legacy_digest.as_str() != Some(raw_canonical_digest(legacy)?.as_str()) {
            return Err(invalid("migration legacy pointer digest mismatch"));
        }
    }
    let digest = string(field(object, "migration_digest")?, "migration digest")?;
    if !is_sha256(digest) || digest_without(value, "migration_digest")? != digest {
        return Err(invalid("migration digest mismatch"));
    }
    Ok(())
}

fn validate_root_common(value: &Value, witness: bool) -> RetrievalResult<()> {
    let object = as_object(
        value,
        if witness {
            "authority witness"
        } else {
            "activation root"
        },
    )?;
    let owner_id = string(
        field(object, "first_owner_generation_id")?,
        "first owner id",
    )?;
    let owner_digest = string(
        field(object, "first_owner_manifest_digest")?,
        "first owner digest",
    )?;
    let migration_digest = string(field(object, "migration_digest")?, "migration digest")?;
    let inner = field(object, "first_inner")?;
    validate_active_projection(inner)?;
    for key in [
        "tombstone_digest",
        "active_pointer_digest",
        "authority_lock_digest",
        "activation_root_digest",
    ] {
        if !field(object, key)?.as_str().is_some_and(is_sha256) {
            return Err(invalid(format!("authority {key} is invalid")));
        }
    }
    if !owner_id_matches(owner_id, owner_digest)
        || !is_sha256(migration_digest)
        || field(object, "migration_file")?.as_str()
            != Some(&format!(
                "ingest-migration-{}.json",
                &migration_digest["sha256:".len()..]
            ))
        || !(field(object, "legacy_pointer_digest")?.is_null()
            || field(object, "legacy_pointer_digest")?
                .as_str()
                .is_some_and(is_sha256))
        || field(object, "active_pointer_digest")?.as_str()
            != Some(derived_active_pointer_digest(owner_id, owner_digest, inner)?.as_str())
    {
        return Err(invalid(if witness {
            "GKX_INGEST_AUTHORITY_WITNESS_BINDING_INVALID"
        } else {
            "GKX_INGEST_ACTIVATION_ROOT_BINDING_INVALID"
        }));
    }
    let tombstone = serde_json::json!({
        "contract_version": "gkos-ingest-legacy-pointer-tombstone/1.0.0-draft.1",
        "target_owner_generation_id": owner_id,
        "target_owner_manifest_digest": owner_digest,
        "migration_file": field(object, "migration_file")?,
        "migration_digest": migration_digest,
    });
    if field(object, "tombstone_digest")?.as_str() != Some(canonical_digest(&tombstone)?.as_str()) {
        return Err(invalid("authority tombstone digest binding is invalid"));
    }
    let root_material = serde_json::json!({
        "contract_version": "gkos-ingest-activation-root/1.0.0-draft.1",
        "first_owner_generation_id": owner_id,
        "first_owner_manifest_digest": owner_digest,
        "first_inner": inner,
        "migration_file": field(object, "migration_file")?,
        "migration_digest": migration_digest,
        "legacy_pointer_digest": field(object, "legacy_pointer_digest")?,
        "tombstone_digest": field(object, "tombstone_digest")?,
        "active_pointer_digest": field(object, "active_pointer_digest")?,
        "authority_lock_digest": field(object, "authority_lock_digest")?,
    });
    if field(object, "activation_root_digest")?.as_str()
        != Some(canonical_digest(&root_material)?.as_str())
    {
        return Err(invalid(if witness {
            "GKX_INGEST_AUTHORITY_WITNESS_BINDING_INVALID"
        } else {
            "GKX_INGEST_ACTIVATION_ROOT_BINDING_INVALID"
        }));
    }
    Ok(())
}

fn validate_activation_root(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "activation root")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "first_owner_generation_id",
            "first_owner_manifest_digest",
            "first_inner",
            "migration_file",
            "migration_digest",
            "legacy_pointer_digest",
            "tombstone_digest",
            "active_pointer_digest",
            "authority_lock_digest",
            "activation_root_digest",
        ],
        "activation root",
    )?;
    if field(object, "contract_version")?.as_str()
        != Some("gkos-ingest-activation-root/1.0.0-draft.1")
    {
        return Err(invalid("activation root contract mismatch"));
    }
    validate_root_common(value, false)?;
    if field(object, "activation_root_digest")?.as_str()
        != Some(digest_without(value, "activation_root_digest")?.as_str())
    {
        return Err(invalid("activation root self digest mismatch"));
    }
    Ok(())
}

fn validate_witness(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "authority witness")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "state",
            "first_owner_generation_id",
            "first_owner_manifest_digest",
            "first_inner",
            "migration_file",
            "migration_digest",
            "legacy_pointer_digest",
            "tombstone_digest",
            "active_pointer_digest",
            "authority_lock_digest",
            "activation_root_digest",
            "witness_digest",
        ],
        "authority witness",
    )?;
    if field(object, "contract_version")?.as_str()
        != Some("gkos-ingest-authority-witness/1.0.0-draft.1")
        || !matches!(
            field(object, "state")?.as_str(),
            Some("activating" | "active")
        )
    {
        return Err(invalid("authority witness contract/state is invalid"));
    }
    validate_root_common(value, true)?;
    if field(object, "witness_digest")?.as_str()
        != Some(digest_without(value, "witness_digest")?.as_str())
    {
        return Err(invalid("authority witness self digest mismatch"));
    }
    Ok(())
}

fn validate_authority_lock(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "authority lock")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "lock_id",
            "process_id",
            "prior_active",
            "prior_authority_digest",
            "operation",
            "target",
            "lock_digest",
        ],
        "authority lock",
    )?;
    validate_prior_active(field(object, "prior_active")?)?;
    let operation = string(field(object, "operation")?, "lock operation")?;
    let target = field(object, "target")?;
    if field(object, "contract_version")?.as_str()
        != Some("gkos-ingest-authority-lock/1.0.0-draft.1")
        || !field(object, "lock_id")?.as_str().is_some_and(is_sha256)
        || unsigned(
            field(object, "process_id")?,
            9_007_199_254_740_991,
            "process_id",
        )? == 0
        || !field(object, "prior_authority_digest")?
            .as_str()
            .is_some_and(is_sha256)
        || !["preflight", "activation", "blocked"].contains(&operation)
    {
        return Err(invalid("authority lock shape is invalid"));
    }
    match operation {
        "preflight" if target.is_null() => {}
        "activation" => {
            let target = as_object(target, "activation lock target")?;
            exact_keys(
                target,
                &[
                    "kind",
                    "owner_generation_id",
                    "owner_manifest_digest",
                    "inner",
                    "pointer_digest",
                ],
                "activation lock target",
            )?;
            let owner_id = string(field(target, "owner_generation_id")?, "target owner id")?;
            let owner_digest = string(
                field(target, "owner_manifest_digest")?,
                "target owner digest",
            )?;
            let inner = field(target, "inner")?;
            validate_active_projection(inner)?;
            if field(target, "kind")?.as_str() != Some("activation")
                || !owner_id_matches(owner_id, owner_digest)
                || field(target, "pointer_digest")?.as_str()
                    != Some(derived_active_pointer_digest(owner_id, owner_digest, inner)?.as_str())
            {
                return Err(invalid(
                    "GKX_INGEST_AUTHORITY_LOCK_ACTIVATION_TARGET_INVALID",
                ));
            }
        }
        "blocked" => {
            let target = as_object(target, "blocked lock target")?;
            exact_keys(target, &["kind", "status_digest"], "blocked lock target")?;
            if field(target, "kind")?.as_str() != Some("blocked")
                || !field(target, "status_digest")?
                    .as_str()
                    .is_some_and(is_sha256)
            {
                return Err(invalid("blocked lock target is invalid"));
            }
        }
        _ => return Err(invalid("authority lock operation/target mismatch")),
    }
    if field(object, "lock_digest")?.as_str()
        != Some(digest_without(value, "lock_digest")?.as_str())
    {
        return Err(invalid("authority lock digest mismatch"));
    }
    Ok(())
}

fn validate_index_summary(value: &Value) -> RetrievalResult<()> {
    let object = as_object(value, "index summary")?;
    exact_keys(
        object,
        &[
            "observed_source_count",
            "valid_source_count",
            "rejected_source_count",
            "findings",
        ],
        "index summary",
    )?;
    let observed = unsigned(
        field(object, "observed_source_count")?,
        1_000_000,
        "observed",
    )?;
    let valid = unsigned(field(object, "valid_source_count")?, 1_000_000, "valid")?;
    let rejected = unsigned(
        field(object, "rejected_source_count")?,
        1_000_000,
        "rejected",
    )?;
    if observed != valid + rejected {
        return Err(invalid("index summary source counts are inconsistent"));
    }
    let counts = as_object(field(object, "findings")?, "index finding counts")?;
    exact_keys(
        counts,
        &["info", "warning", "error", "critical"],
        "index finding counts",
    )?;
    for severity in ["info", "warning", "error", "critical"] {
        unsigned(field(counts, severity)?, 1_000_000, severity)?;
    }
    Ok(())
}

fn validate_index_result(value: &Value, blocked_status: Option<&Value>) -> RetrievalResult<()> {
    let object = as_object(value, "index result")?;
    exact_keys(
        object,
        &[
            "contract_version",
            "status",
            "mode",
            "summary",
            "active",
            "blocked_attempt",
        ],
        "index result",
    )
    .map_err(|_| invalid("GKX_INGEST_INDEX_RESULT_FIELDS_INVALID"))?;
    let status = string(field(object, "status")?, "index status")?;
    let mode = string(field(object, "mode")?, "index mode")?;
    if field(object, "contract_version")?.as_str() != Some("gkos-ingest-index-result/1.0.0-draft.1")
        || ![
            "published",
            "published_with_rejections",
            "blocked_strict",
            "operational_failure",
        ]
        .contains(&status)
        || !["strict", "non_strict"].contains(&mode)
    {
        return Err(invalid("index result discriminant is invalid"));
    }
    let summary = field(object, "summary")?;
    let active = field(object, "active")?;
    let blocked = field(object, "blocked_attempt")?;
    if !active.is_null() {
        validate_prior_active(active)?;
    }
    match status {
        "published" | "published_with_rejections" => {
            validate_index_summary(summary)?;
            let summary = as_object(summary, "summary")?;
            let rejected = field(summary, "rejected_source_count")?.as_u64().unwrap();
            if as_object(active, "published active")?["kind"] != "ingest"
                || !blocked.is_null()
                || (status == "published") != (rejected == 0)
                || (status == "published_with_rejections"
                    && (mode != "non_strict" || rejected == 0))
            {
                return Err(invalid("published index result condition is invalid"));
            }
        }
        "blocked_strict" => {
            validate_index_summary(summary)?;
            let summary = as_object(summary, "summary")?;
            let blocked = as_object(blocked, "blocked attempt coordinate")?;
            exact_keys(
                blocked,
                &["attempt_digest", "status_digest"],
                "blocked attempt coordinate",
            )?;
            let Some(blocked_status) = blocked_status else {
                return Err(invalid("GKX_INGEST_INDEX_RESULT_BLOCKED_INVALID"));
            };
            validate_attempt_status(blocked_status)?;
            let status_object = as_object(blocked_status, "blocked status")?;
            if mode != "strict"
                || field(summary, "rejected_source_count")?.as_u64() == Some(0)
                || active != field(status_object, "prior_active")?
                || field(blocked, "attempt_digest")? != field(status_object, "attempt_digest")?
                || field(blocked, "status_digest")? != field(status_object, "status_digest")?
            {
                return Err(invalid("GKX_INGEST_INDEX_RESULT_BLOCKED_INVALID"));
            }
        }
        "operational_failure" => {
            if !summary.is_null() || !active.is_null() || !blocked.is_null() {
                return Err(invalid("operational failure must be generic and path-free"));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORAGE_FIXTURE: &str = include_str!(
        "../../../contracts/gkos-ingest-validation-1.0.0-draft.1/storage-conformance-fixture.json"
    );

    #[test]
    fn frozen_full_profile_validation_and_journal_envelopes_verify() {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).unwrap();
        let expected = &fixture["executable"]["expected_result"];
        let profile = verify_normalized_ingest_profile(
            canonical_json(&expected["normalized_profile"])
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(profile.kind(), IngestEnvelopeKind::NormalizedProfile);
        let result =
            verify_ingest_validation_result(canonical_json(expected).unwrap().as_bytes()).unwrap();
        assert_eq!(result.kind(), IngestEnvelopeKind::ValidationResult);
        let rejection = verify_ingest_rejection(
            canonical_json(&expected["rejections"][0])
                .unwrap()
                .as_bytes(),
            &profile,
        )
        .unwrap();
        assert_eq!(rejection.kind(), IngestEnvelopeKind::Rejection);

        let storage: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let journal = &storage["valid_envelopes"]["rejection_journal"];
        let journal =
            verify_ingest_rejection_journal(canonical_json(journal).unwrap().as_bytes()).unwrap();
        assert_eq!(journal.kind(), IngestEnvelopeKind::RejectionJournal);
        assert_eq!(journal.canonical_bytes().last(), Some(&b'\n'));
    }

    #[test]
    fn forged_digest_and_profile_widening_are_rejected() {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).unwrap();
        let mut result = fixture["executable"]["expected_result"].clone();
        result["findings"][0]["finding_id"] = Value::String(format!("sha256:{}", "0".repeat(64)));
        assert!(
            verify_ingest_validation_result(canonical_json(&result).unwrap().as_bytes()).is_err()
        );

        let mut profile = fixture["executable"]["expected_result"]["normalized_profile"].clone();
        profile["required_fields"] = Value::Array(Vec::new());
        assert!(
            verify_normalized_ingest_profile(canonical_json(&profile).unwrap().as_bytes()).is_err()
        );
        assert!(parse_json(br#"{"value":-0}"#, 1_024).is_err());
        assert!(parse_json(br#"{"value":-0.0}"#, 1_024).is_err());
        assert!(parse_json(br#"{ "value": 0 }"#, 1_024).is_err());
        assert!(parse_json(br#"{"value":0,"value":0}"#, 1_024).is_err());
    }

    #[test]
    fn forged_safe_coordinates_and_empty_profile_domains_are_rejected() {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).unwrap();
        let expected = &fixture["executable"]["expected_result"];
        let normalized = &expected["normalized_profile"];

        let mut finding = expected["findings"][0].clone();
        finding["code"] = Value::String("GKX-AUTHORITY-ROLE-001".to_owned());
        finding["severity"] = Value::String("critical".to_owned());
        finding["scope"] = Value::String("field".to_owned());
        finding["coordinate_basis"] = Value::String("frontmatter_field".to_owned());
        finding["line"] = Value::from(1_u64);
        finding["field"] = Value::String("gkx_assignment.authority.attacker".to_owned());
        finding["finding_id"] = Value::String(digest_without(&finding, "finding_id").unwrap());
        assert!(validate_finding(&finding).is_err());

        let mut relationship = expected["findings"][0].clone();
        relationship["code"] = Value::String("AUTHORED_RELATIONSHIP_REFERENCE_INVALID".to_owned());
        relationship["severity"] = Value::String("error".to_owned());
        relationship["scope"] = Value::String("field".to_owned());
        relationship["coordinate_basis"] = Value::String("frontmatter_field".to_owned());
        relationship["line"] = Value::from(1_u64);
        relationship["field"] = Value::String("relationships.blocks[01]".to_owned());
        relationship["finding_id"] =
            Value::String(digest_without(&relationship, "finding_id").unwrap());
        assert!(validate_finding(&relationship).is_err());

        for field in [
            "relationships",
            "supersedes",
            "superseded_by",
            "relationships.approved_by",
            "relationships.blocks",
            "relationships.cites",
            "relationships.contradicts",
            "relationships.depends_on",
            "relationships.derived_from",
            "relationships.derives_from",
            "relationships.documents",
            "relationships.extends",
            "relationships.fails_to_replicate",
            "relationships.generalizes",
            "relationships.governed_by",
            "relationships.has_part",
            "relationships.implements",
            "relationships.interprets",
            "relationships.narrows",
            "relationships.part_of",
            "relationships.quotes",
            "relationships.refines",
            "relationships.related_to",
            "relationships.replicates",
            "relationships.reviewed_by",
            "relationships.supports",
            "relationships.tests",
        ] {
            relationship["field"] = Value::String(format!("{field}[0]"));
            relationship["finding_id"] =
                Value::String(digest_without(&relationship, "finding_id").unwrap());
            validate_finding(&relationship).unwrap();
        }
        for field in [
            "relationships.forked_from[0]",
            "relationships.forked_to[0]",
            "relationships.forked_by[0]",
            "relationships.nonsense[0]",
        ] {
            relationship["field"] = Value::String(field.to_owned());
            relationship["finding_id"] =
                Value::String(digest_without(&relationship, "finding_id").unwrap());
            assert!(validate_finding(&relationship).is_err());
        }

        let mut long_path = expected["findings"][0].clone();
        long_path["source_path"] = Value::String(format!("{}.md", "a".repeat(4_094)));
        long_path["finding_id"] = Value::String(digest_without(&long_path, "finding_id").unwrap());
        assert!(validate_finding(&long_path).is_err());

        let mut profile = normalized.clone();
        let title = profile["fields"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|field| field["field"] == "title")
            .unwrap();
        title["enum"] = serde_json::json!([" "]);
        assert!(validate_normalized_profile(&profile).is_err());

        let mut sensitivity_finding = expected["findings"][0].clone();
        sensitivity_finding["code"] =
            Value::String("GKX_PROFILE_SENSITIVITY_BELOW_MINIMUM".to_owned());
        sensitivity_finding["severity"] = Value::String("error".to_owned());
        sensitivity_finding["scope"] = Value::String("field".to_owned());
        sensitivity_finding["coordinate_basis"] = Value::String("frontmatter_field".to_owned());
        sensitivity_finding["line"] = Value::from(1_u64);
        sensitivity_finding["field"] = Value::String("sensitivity.level".to_owned());
        sensitivity_finding["finding_id"] =
            Value::String(digest_without(&sensitivity_finding, "finding_id").unwrap());
        validate_finding(&sensitivity_finding).unwrap();
        assert!(validate_finding_against_profile(&sensitivity_finding, normalized).is_err());
    }

    fn apply_profile_fixture_mutations(profile: &mut Value, mutations: &[Value]) {
        for mutation in mutations {
            let target = mutation["target"].as_str().unwrap();
            let member = mutation["member"].as_str();
            let value = mutation["value"].clone();
            match target {
                "profile" => {
                    profile[member.unwrap()] = value;
                }
                "field" => {
                    let field_name = mutation["field"].as_str().unwrap();
                    let field = profile["fields"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|field| field["field"] == field_name)
                        .unwrap();
                    field[member.unwrap()] = value;
                }
                "severity" => {
                    let code = mutation["code"].as_str().unwrap();
                    let severity = profile["severity"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|row| row["code"] == code)
                        .unwrap();
                    severity[member.unwrap()] = value;
                }
                "required_fields" => {
                    assert_eq!(mutation["operation"], "insert");
                    let required = profile["required_fields"].as_array_mut().unwrap();
                    required.push(value);
                    required.sort_by(|left, right| {
                        compare_strings(left.as_str().unwrap(), right.as_str().unwrap())
                    });
                }
                _ => panic!("unknown profile fixture mutation target {target}"),
            }
        }
    }

    #[test]
    fn frozen_sensitivity_and_unicode_matrices_drive_verifier_semantics() {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).unwrap();
        let baseline = &fixture["executable"]["expected_result"]["normalized_profile"];
        for case in fixture["normalized_profile_sensitivity_cases"]
            .as_array()
            .unwrap()
        {
            let mut profile = baseline.clone();
            apply_profile_fixture_mutations(&mut profile, case["mutations"].as_array().unwrap());
            let outcome =
                verify_normalized_ingest_profile(canonical_json(&profile).unwrap().as_bytes());
            if case["semantic_outcome"] == "accept" {
                outcome
                    .unwrap_or_else(|error| panic!("{}: {error}", case["case"].as_str().unwrap()));
                assert!(case["semantic_code"].is_null());
            } else {
                let expected = case["semantic_code"].as_str().unwrap();
                assert!(
                    matches!(
                        outcome,
                        Err(RetrievalError::InvalidEnvelope(ref code)) if code == expected
                    ),
                    "{}: {outcome:?}",
                    case["case"].as_str().unwrap()
                );
            }
        }

        let missing = &fixture["missing_sensitivity_case"]["expected"];
        assert_eq!(missing["status"], "valid");
        assert_eq!(missing["ingest_intrinsic_valid"], true);
        assert_eq!(missing["finding"]["code"], "GKX-SENSITIVITY-001");
        assert_eq!(missing["finding"]["severity"], "warning");
        assert_eq!(missing["finding"]["coordinate_basis"], "missing_field");
        assert!(missing["finding"]["line"].is_null());
        assert_eq!(missing["accepted_effective_sensitivity"], "secret");

        let ordering = &fixture["unicode_ordering"];
        let mut utf16 = ordering["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        utf16.sort_by(|left, right| compare_strings(left, right));
        assert_eq!(
            utf16,
            ordering["expected_ecmascript_utf16"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
        let mut utf8 = ordering["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        utf8.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        assert_eq!(
            utf8,
            ordering["expected_utf8_bytes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
        assert_ne!(utf16, utf8);
    }

    #[test]
    fn frozen_predicate_and_decision_a_matrices_drive_mixed_result_sealing() {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).unwrap();
        assert_eq!(
            fixture["predicate_matrix"],
            serde_json::json!({
                "intrinsic_error_or_critical": {
                    "validate_invalid": true,
                    "ingest_rejected": true
                },
                "intrinsic_warning_or_info_only": {
                    "validate_invalid": false,
                    "ingest_rejected": false
                },
                "cross_record_error_or_critical": {
                    "validate_invalid": true,
                    "ingest_rejected": false
                },
                "cross_record_warning_or_info_only": {
                    "validate_invalid": false,
                    "ingest_rejected": false
                }
            })
        );

        let expected_classes = [
            "canonical_identity_collision",
            "endpoint_resolution_ambiguity_or_unresolved",
            "forward_inverse_or_conflicting_declarations",
            "branch_cycle_or_temporal_order",
        ];
        let decision_a = fixture["decision_a_matrix"].as_array().unwrap();
        assert_eq!(decision_a.len(), expected_classes.len());
        for (row, expected_class) in decision_a.iter().zip(expected_classes) {
            assert_eq!(row["class"], expected_class);
            assert_eq!(row["finding"], "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT");
            assert_eq!(row["classification"], "cross_record_report_only");
            assert_eq!(row["ingest_rejection"], false);
        }

        let mut result = fixture["executable"]["expected_result"].clone();
        let mut conflict = serde_json::json!({
            "classification": "cross_record_report_only",
            "code": "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT",
            "contract_version": FINDING_VERSION,
            "coordinate_basis": "corpus",
            "deterministic": true,
            "field": null,
            "finding_id": "",
            "line": null,
            "scope": "corpus",
            "severity": "error",
            "source_observation_ordinal": null,
            "source_path": null
        });
        conflict["finding_id"] = Value::String(digest_without(&conflict, "finding_id").unwrap());
        result["findings"] = serde_json::json!([conflict]);
        result["rejections"] = serde_json::json!([]);
        result["observations"] = serde_json::json!([result["observations"][0].clone()]);
        for observation in result["observations"].as_array_mut().unwrap() {
            observation["classification"] = Value::String("accepted".to_owned());
            observation["finding_ids"] = serde_json::json!([]);
            observation["intrinsic_blocking_finding_ids"] = serde_json::json!([]);
        }
        result["status"] = Value::String("invalid".to_owned());
        result["corpus_valid"] = Value::Bool(false);
        result["ingest_intrinsic_valid"] = Value::Bool(true);
        result["summary"]["observed_source_count"] = Value::from(1_u64);
        result["summary"]["valid_source_count"] = Value::from(1_u64);
        result["summary"]["rejected_source_count"] = Value::from(0_u64);
        result["summary"]["findings"] = serde_json::json!({
            "info": 0,
            "warning": 0,
            "error": 1,
            "critical": 0
        });

        let verified =
            verify_ingest_validation_result(canonical_json(&result).unwrap().as_bytes()).unwrap();
        assert_eq!(verified.kind(), IngestEnvelopeKind::ValidationResult);
        assert_eq!(result["corpus_valid"], false);
        assert_eq!(result["ingest_intrinsic_valid"], true);
        assert!(result["rejections"].as_array().unwrap().is_empty());
        assert_eq!(
            result["findings"][0]["classification"],
            "cross_record_report_only"
        );
    }

    #[test]
    fn frozen_validator_semantic_negative_matrix_is_executable() {
        let fixture: Value = serde_json::from_str(PROFILE_FIXTURE).unwrap();
        let expected = &fixture["executable"]["expected_result"];
        let rows = fixture["semantic_negative_matrix"].as_array().unwrap();
        let actual = rows
            .iter()
            .map(|row| {
                (
                    row["class"].as_str().unwrap(),
                    row["schema_may_be_insufficient"].as_bool().unwrap(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual,
            BTreeSet::from([
                (
                    "forged_finding_classification_scope_coordinate_or_digest",
                    true,
                ),
                ("nonportable_source_path", false),
                ("normalized_profile_widening_or_duplicate_coordinate", true),
                (
                    "rejection_temporal_content_finding_order_or_digest_mismatch",
                    true,
                ),
                (
                    "result_profile_predicate_count_partition_or_nested_subset_mismatch",
                    true,
                ),
            ])
        );
        assert_eq!(rows.len(), actual.len());

        for row in rows {
            assert_eq!(row["semantic_outcome"], "reject");
            let class = row["class"].as_str().unwrap();
            let rejected = match class {
                "normalized_profile_widening_or_duplicate_coordinate" => {
                    let mut profile = expected["normalized_profile"].clone();
                    let duplicate = profile["fields"][0].clone();
                    profile["fields"].as_array_mut().unwrap().push(duplicate);
                    verify_normalized_ingest_profile(canonical_json(&profile).unwrap().as_bytes())
                        .is_err()
                }
                "nonportable_source_path" => {
                    let mut finding = expected["findings"][0].clone();
                    finding["source_path"] = Value::String("../escape.md".to_owned());
                    finding["finding_id"] =
                        Value::String(digest_without(&finding, "finding_id").unwrap());
                    validate_finding(&finding).is_err()
                }
                "forged_finding_classification_scope_coordinate_or_digest" => {
                    let mut finding = expected["findings"][0].clone();
                    finding["scope"] = Value::String("corpus".to_owned());
                    finding["finding_id"] =
                        Value::String(digest_without(&finding, "finding_id").unwrap());
                    validate_finding(&finding).is_err()
                }
                "rejection_temporal_content_finding_order_or_digest_mismatch" => {
                    let mut rejection = expected["rejections"][0].clone();
                    rejection["findings"].as_array_mut().unwrap().reverse();
                    rejection["rejection_digest"] =
                        Value::String(digest_without(&rejection, "rejection_digest").unwrap());
                    let profile = verify_normalized_ingest_profile(
                        canonical_json(&expected["normalized_profile"])
                            .unwrap()
                            .as_bytes(),
                    )
                    .unwrap();
                    verify_ingest_rejection(
                        canonical_json(&rejection).unwrap().as_bytes(),
                        &profile,
                    )
                    .is_err()
                }
                "result_profile_predicate_count_partition_or_nested_subset_mismatch" => {
                    let mut result = expected.clone();
                    result["corpus_valid"] = Value::Bool(true);
                    verify_ingest_validation_result(canonical_json(&result).unwrap().as_bytes())
                        .is_err()
                }
                _ => unreachable!("unfrozen validator semantic-negative class {class}"),
            };
            assert!(rejected, "{class} was accepted");
        }
    }

    #[test]
    fn frozen_full_owner_and_state_union_envelopes_verify() {
        let fixture: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let valid = &fixture["valid_envelopes"];
        let owner = verify_ingest_owner_generation(
            canonical_json(&valid["owner_generation"])
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(owner.kind(), IngestEnvelopeKind::OwnerGeneration);

        for key in [
            "active_pointer",
            "migration",
            "migration_with_legacy",
            "legacy_tombstone",
            "activation_root",
            "authority_witness",
            "authority_lock",
            "attempt_status",
        ] {
            verify_ingest_state_envelope(canonical_json(&valid[key]).unwrap().as_bytes())
                .unwrap_or_else(|error| panic!("{key} failed: {error}"));
        }
        for group in [
            "authority_witnesses",
            "authority_locks",
            "attempt_statuses",
            "migrations",
        ] {
            for envelope in fixture["union_matrix"][group].as_array().unwrap() {
                verify_ingest_state_envelope(canonical_json(envelope).unwrap().as_bytes())
                    .unwrap_or_else(|error| panic!("{group} failed: {error}"));
            }
        }
        for result in valid["index_results"].as_array().unwrap() {
            let blocked = if result["status"] == "blocked_strict" {
                fixture["union_matrix"]["attempt_statuses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|status| {
                        status["status_digest"] == result["blocked_attempt"]["status_digest"]
                    })
                    .map(|status| {
                        verify_ingest_state_envelope(canonical_json(status).unwrap().as_bytes())
                            .unwrap()
                    })
            } else {
                None
            };
            verify_ingest_index_result(
                canonical_json(result).unwrap().as_bytes(),
                blocked.as_ref(),
            )
            .unwrap_or_else(|error| panic!("index result failed: {error}"));
        }
    }

    #[test]
    fn retrieval_manifest_gate_matches_full_safe_integer_and_identity_semantics() {
        let fixture: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let valid = &fixture["valid_envelopes"];
        let schema_three = &valid["owner_generation"]["inner"]["manifest"];
        validate_retrieval_manifest(schema_three, true).unwrap();

        for (field_name, replacement) in [
            ("engine_version", Value::String("forged".to_owned())),
            ("vault_id", Value::String(String::new())),
            ("vault_id", Value::String("v".repeat(513))),
            ("lexical_backend", Value::String("sqlite".to_owned())),
            ("candidate_source_count", Value::from(-1_i64)),
            ("candidate_source_count", Value::from(MAX_SAFE_INTEGER + 1)),
            (
                "embedding_provider_id",
                Value::String("provider".to_owned()),
            ),
            ("embedding_dimensions", Value::from(0_u64)),
        ] {
            let mut forged = schema_three.clone();
            forged[field_name] = replacement;
            assert!(
                validate_retrieval_manifest(&forged, true).is_err(),
                "schema-3 mutation {field_name} was accepted"
            );
        }
        let mut bad_represented = schema_three.clone();
        bad_represented["represented_candidate_source_count"] =
            Value::from(bad_represented["candidate_source_count"].as_u64().unwrap() + 1);
        assert!(validate_retrieval_manifest(&bad_represented, true).is_err());
        let mut bad_eligible = schema_three.clone();
        bad_eligible["embedding_eligible_candidate_chunk_count"] =
            Value::from(bad_eligible["candidate_chunk_count"].as_u64().unwrap() + 1);
        assert!(validate_retrieval_manifest(&bad_eligible, true).is_err());

        for field_name in [
            "candidate_source_count",
            "candidate_declaration_count",
            "candidate_chunk_count",
        ] {
            let mut wide = schema_three.clone();
            wide[field_name] = Value::from(u64::from(u32::MAX) + 1);
            if field_name == "candidate_source_count" {
                wide["represented_candidate_source_count"] = Value::from(0_u64);
            }
            validate_retrieval_manifest(&wide, true)
                .unwrap_or_else(|error| panic!("safe-integer {field_name} rejected: {error}"));
        }
        let mut wide_vector = schema_three.clone();
        wide_vector["embedding_provider_id"] = Value::String("provider".to_owned());
        wide_vector["embedding_model_id"] = Value::String("model".to_owned());
        wide_vector["embedding_dimensions"] = Value::from(u64::from(u32::MAX) + 1);
        validate_retrieval_manifest(&wide_vector, true).unwrap();

        let schema_two = serde_json::json!({
            "contract_version": RETRIEVAL_CONTRACT,
            "projection_schema_version": PROJECTION_SCHEMA_VERSION,
            "projection_id": schema_three["projection_id"],
            "engine_version": schema_three["engine_version"],
            "vault_id": schema_three["vault_id"],
            "source_snapshot_digest": schema_three["source_snapshot_digest"],
            "configuration_digest": schema_three["configuration_digest"],
            "policy_digest": schema_three["policy_digest"],
            "chunker_version": schema_three["chunker_version"],
            "tokenizer_version": schema_three["tokenizer_version"],
            "lexical_backend": schema_three["lexical_backend"],
            "embedding_provider_id": schema_three["embedding_provider_id"],
            "embedding_model_id": schema_three["embedding_model_id"],
            "embedding_dimensions": schema_three["embedding_dimensions"],
            "source_count": schema_three["candidate_source_count"],
            "chunk_count": schema_three["candidate_chunk_count"],
            "projection_digest": schema_three["projection_digest"],
        });
        validate_retrieval_manifest(&schema_two, false).unwrap();
        let mut wide_schema_two = schema_two.clone();
        wide_schema_two["source_count"] = Value::from(u64::from(u32::MAX) + 1);
        wide_schema_two["chunk_count"] = Value::from(MAX_SAFE_INTEGER);
        validate_retrieval_manifest(&wide_schema_two, false).unwrap();
        for (field_name, replacement) in [
            ("engine_version", Value::String("2.1.1".to_owned())),
            ("vault_id", Value::String("v".repeat(513))),
            ("lexical_backend", Value::String("forged".to_owned())),
            ("source_count", Value::from(-1_i64)),
            ("chunk_count", Value::from(MAX_SAFE_INTEGER + 1)),
        ] {
            let mut forged = schema_two.clone();
            forged[field_name] = replacement;
            assert!(
                validate_retrieval_manifest(&forged, false).is_err(),
                "schema-2 mutation {field_name} was accepted"
            );
        }
    }

    #[test]
    fn del_in_portable_source_path_is_rejected_after_self_resealing() {
        let fixture: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let valid = &fixture["valid_envelopes"];
        let unsafe_path = "safe\u{007f}name.md";
        assert!(!is_portable_source_path(unsafe_path));

        let mut finding = valid["owner_generation"]["validation_result"]["findings"][1].clone();
        finding["source_path"] = Value::String(unsafe_path.to_owned());
        finding["finding_id"] = Value::String(digest_without(&finding, "finding_id").unwrap());
        assert!(validate_finding(&finding).is_err());

        let mut rejection = valid["rejection_journal"]["rejections"][0].clone();
        rejection["source_path"] = Value::String(unsafe_path.to_owned());
        for nested in rejection["findings"].as_array_mut().unwrap() {
            nested["source_path"] = Value::String(unsafe_path.to_owned());
            nested["finding_id"] = Value::String(digest_without(nested, "finding_id").unwrap());
        }
        rejection["rejection_digest"] =
            Value::String(digest_without(&rejection, "rejection_digest").unwrap());
        let normalized = verify_normalized_ingest_profile(
            canonical_json(&valid["rejection_journal"]["normalized_profile"])
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        assert!(verify_ingest_rejection(
            canonical_json(&rejection).unwrap().as_bytes(),
            &normalized,
        )
        .is_err());

        let mut journal = valid["rejection_journal"].clone();
        journal["rejections"][0] = rejection;
        journal["rejection_journal_digest"] =
            Value::String(digest_without(&journal, "rejection_journal_digest").unwrap());
        assert!(
            verify_ingest_rejection_journal(canonical_json(&journal).unwrap().as_bytes()).is_err()
        );

        let mut owner = valid["owner_generation"].clone();
        owner["validation_result"]["findings"][1] = finding;
        let digest =
            digest_without_keys(&owner, &["owner_generation_id", "owner_manifest_digest"]).unwrap();
        owner["owner_manifest_digest"] = Value::String(digest.clone());
        owner["owner_generation_id"] = Value::String(format!(
            "ingest:{}",
            &digest["sha256:".len().."sha256:".len() + 24]
        ));
        assert!(
            verify_ingest_owner_generation(canonical_json(&owner).unwrap().as_bytes()).is_err()
        );
    }

    #[test]
    fn self_resealed_owner_and_authority_substitutions_are_rejected() {
        let fixture: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let valid = &fixture["valid_envelopes"];

        let mut pointer = valid["active_pointer"].clone();
        pointer["owner_generation_id"] =
            Value::String("ingest:000000000000000000000000".to_owned());
        assert!(
            verify_ingest_state_envelope(canonical_json(&pointer).unwrap().as_bytes()).is_err()
        );

        let mut status = valid["attempt_status"].clone();
        status["attempt_digest"] = Value::String(format!("sha256:{}", "0".repeat(64)));
        let digest = digest_without(&status, "status_digest").unwrap();
        status["status_digest"] = Value::String(digest);
        assert!(verify_ingest_state_envelope(canonical_json(&status).unwrap().as_bytes()).is_err());

        let mut owner = valid["owner_generation"].clone();
        owner["rejection_journal"]["rejection_journal_digest"] =
            Value::String(format!("sha256:{}", "1".repeat(64)));
        let digest =
            digest_without_keys(&owner, &["owner_generation_id", "owner_manifest_digest"]).unwrap();
        owner["owner_manifest_digest"] = Value::String(digest.clone());
        owner["owner_generation_id"] = Value::String(format!(
            "ingest:{}",
            &digest["sha256:".len().."sha256:".len() + 24]
        ));
        assert!(
            verify_ingest_owner_generation(canonical_json(&owner).unwrap().as_bytes()).is_err()
        );

        for (field_name, replacement) in [
            ("engine_version", Value::String("forged".to_owned())),
            ("vault_id", Value::String("v".repeat(513))),
        ] {
            let mut owner = valid["owner_generation"].clone();
            owner["inner"]["manifest"][field_name] = replacement;
            owner["inner"]["manifest_digest"] =
                Value::String(canonical_digest(&owner["inner"]["manifest"]).unwrap());
            let digest =
                digest_without_keys(&owner, &["owner_generation_id", "owner_manifest_digest"])
                    .unwrap();
            owner["owner_manifest_digest"] = Value::String(digest.clone());
            owner["owner_generation_id"] = Value::String(format!(
                "ingest:{}",
                &digest["sha256:".len().."sha256:".len() + 24]
            ));
            assert!(
                verify_ingest_owner_generation(canonical_json(&owner).unwrap().as_bytes()).is_err(),
                "self-resealed owner manifest {field_name} substitution was accepted"
            );
        }

        let mut migration = valid["migration_with_legacy"].clone();
        migration["legacy_pointer"]["manifest"]["engine_version"] =
            Value::String("forged".to_owned());
        migration["legacy_pointer_digest"] =
            Value::String(canonical_digest(&migration["legacy_pointer"]).unwrap());
        migration["migration_digest"] =
            Value::String(digest_without(&migration, "migration_digest").unwrap());
        assert!(
            verify_ingest_state_envelope(canonical_json(&migration).unwrap().as_bytes()).is_err()
        );
    }

    #[test]
    fn frozen_storage_semantic_negative_matrix_is_executable() {
        let fixture: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let valid = &fixture["valid_envelopes"];
        let blocked_result = valid["index_results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|result| result["status"] == "blocked_strict")
            .unwrap();
        let blocked_status = fixture["union_matrix"]["attempt_statuses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|status| {
                status["status_digest"] == blocked_result["blocked_attempt"]["status_digest"]
            })
            .unwrap();

        let rows = fixture["semantic_negative_matrix"].as_array().unwrap();
        let actual_cases = rows
            .iter()
            .map(|row| row["case"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_cases,
            BTreeSet::from([
                "active_pointer_owner_id_binding",
                "root_pointer_binding",
                "witness_root_binding",
                "lock_target_pointer_binding",
                "attempt_preimage_binding",
                "journal_duplicate_observation",
                "owner_journal_coordinate",
                "legacy_database_projection_binding",
                "index_owner_plane_forbidden",
                "index_blocked_status_context",
            ])
        );
        assert_eq!(rows.len(), actual_cases.len());

        for row in rows {
            let case = row["case"].as_str().unwrap();
            let outcome = match case {
                "active_pointer_owner_id_binding" => {
                    let mut envelope = valid["active_pointer"].clone();
                    envelope["owner_generation_id"] =
                        Value::String("ingest:000000000000000000000000".to_owned());
                    verify_ingest_state_envelope(canonical_json(&envelope).unwrap().as_bytes())
                }
                "root_pointer_binding" => {
                    let mut envelope = valid["activation_root"].clone();
                    envelope["active_pointer_digest"] =
                        Value::String(format!("sha256:{}", "0".repeat(64)));
                    envelope["activation_root_digest"] =
                        Value::String(digest_without(&envelope, "activation_root_digest").unwrap());
                    verify_ingest_state_envelope(canonical_json(&envelope).unwrap().as_bytes())
                }
                "witness_root_binding" => {
                    let mut envelope = valid["authority_witness"].clone();
                    envelope["activation_root_digest"] =
                        Value::String(format!("sha256:{}", "0".repeat(64)));
                    envelope["witness_digest"] =
                        Value::String(digest_without(&envelope, "witness_digest").unwrap());
                    verify_ingest_state_envelope(canonical_json(&envelope).unwrap().as_bytes())
                }
                "lock_target_pointer_binding" => {
                    let mut envelope = valid["authority_lock"].clone();
                    envelope["target"]["pointer_digest"] =
                        Value::String(format!("sha256:{}", "0".repeat(64)));
                    envelope["lock_digest"] =
                        Value::String(digest_without(&envelope, "lock_digest").unwrap());
                    verify_ingest_state_envelope(canonical_json(&envelope).unwrap().as_bytes())
                }
                "attempt_preimage_binding" => {
                    let mut envelope = valid["attempt_status"].clone();
                    envelope["attempt_digest"] =
                        Value::String(format!("sha256:{}", "0".repeat(64)));
                    envelope["status_digest"] =
                        Value::String(digest_without(&envelope, "status_digest").unwrap());
                    verify_ingest_state_envelope(canonical_json(&envelope).unwrap().as_bytes())
                }
                "journal_duplicate_observation" => {
                    let mut envelope = valid["rejection_journal"].clone();
                    let duplicate = envelope["rejections"][0].clone();
                    envelope["rejections"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                    envelope["rejection_count"] = Value::from(2_u64);
                    envelope["rejection_journal_digest"] = Value::String(
                        digest_without(&envelope, "rejection_journal_digest").unwrap(),
                    );
                    verify_ingest_rejection_journal(canonical_json(&envelope).unwrap().as_bytes())
                }
                "owner_journal_coordinate" => {
                    let mut envelope = valid["owner_generation"].clone();
                    envelope["rejection_journal"]["rejection_journal_digest"] =
                        Value::String(format!("sha256:{}", "1".repeat(64)));
                    let digest = digest_without_keys(
                        &envelope,
                        &["owner_generation_id", "owner_manifest_digest"],
                    )
                    .unwrap();
                    envelope["owner_manifest_digest"] = Value::String(digest.clone());
                    envelope["owner_generation_id"] = Value::String(format!(
                        "ingest:{}",
                        &digest["sha256:".len().."sha256:".len() + 24]
                    ));
                    verify_ingest_owner_generation(canonical_json(&envelope).unwrap().as_bytes())
                }
                "legacy_database_projection_binding" => {
                    let mut envelope = valid["migration_with_legacy"].clone();
                    envelope["legacy_pointer"]["database_file"] =
                        Value::String(format!("retrieval-{}.sqlite", "0".repeat(64)));
                    envelope["legacy_pointer_digest"] =
                        Value::String(canonical_digest(&envelope["legacy_pointer"]).unwrap());
                    envelope["migration_digest"] =
                        Value::String(digest_without(&envelope, "migration_digest").unwrap());
                    verify_ingest_state_envelope(canonical_json(&envelope).unwrap().as_bytes())
                }
                "index_owner_plane_forbidden" => {
                    let mut envelope = valid["index_results"][0].clone();
                    envelope["validation_result"] =
                        valid["owner_generation"]["validation_result"].clone();
                    verify_ingest_index_result(canonical_json(&envelope).unwrap().as_bytes(), None)
                }
                "index_blocked_status_context" => {
                    let mut envelope = blocked_result.clone();
                    envelope["blocked_attempt"]["status_digest"] =
                        Value::String(format!("sha256:{}", "0".repeat(64)));
                    let status = verify_ingest_state_envelope(
                        canonical_json(blocked_status).unwrap().as_bytes(),
                    )
                    .unwrap();
                    verify_ingest_index_result(
                        canonical_json(&envelope).unwrap().as_bytes(),
                        Some(&status),
                    )
                }
                _ => panic!("unconsumed storage semantic-negative fixture row {case}"),
            };
            let expected = row["semantic_code"].as_str().unwrap();
            assert!(
                matches!(
                    outcome,
                    Err(RetrievalError::InvalidEnvelope(ref code)) if code == expected
                ),
                "storage semantic-negative case {case}: {outcome:?}"
            );
        }
    }

    fn fixture_owner_bundle() -> VerifiedIngestOwnerBundle {
        let fixture: Value = serde_json::from_str(STORAGE_FIXTURE).unwrap();
        let valid = &fixture["valid_envelopes"];
        verify_ingest_owner_bundle(
            canonical_json(&valid["owner_generation"])
                .unwrap()
                .as_bytes(),
            canonical_json(&valid["rejection_journal"])
                .unwrap()
                .as_bytes(),
            canonical_json(&valid["active_pointer"]).unwrap().as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn verified_owner_bundle_is_a_pure_opaque_capability() {
        let bundle = fixture_owner_bundle();
        assert_eq!(bundle.owner.kind(), IngestEnvelopeKind::OwnerGeneration);
        assert_eq!(bundle.journal.kind(), IngestEnvelopeKind::RejectionJournal);
        assert_eq!(bundle.pointer.kind(), IngestEnvelopeKind::ActivePointer);
        let repeated = verify_ingest_owner_bundle(
            &bundle.owner.canonical_bytes(),
            &bundle.journal.canonical_bytes(),
            &bundle.pointer.canonical_bytes(),
        )
        .unwrap();
        assert_eq!(repeated, bundle);
    }

    #[test]
    fn frozen_phase3_pack_is_exact_and_pinned_to_hosted_full() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/gkos-ingest-validation-1.0.0-draft.1");
        let pin: Value =
            serde_json::from_slice(&fs::read(root.join("FULL-PIN.json")).unwrap()).unwrap();
        assert_eq!(
            pin["reference_commit"],
            "e7cc0dd478af3d0bda216c5258dec5f77932def7"
        );
        assert_eq!(pin["reference_package_version"], "2.1.2");
        assert_eq!(pin["publication_qualified"], true);
        assert_eq!(pin["reference_state"], "full_phase3_published_hosted_green");
        assert_eq!(pin["pin_kind"], "exact_full_commit_and_frozen_file_sha256");
        let files = pin["files"].as_object().unwrap();
        assert_eq!(files.len(), 21);
        let mut total = 0_usize;
        for (name, expected) in files {
            let bytes = fs::read(root.join(name)).unwrap();
            total += bytes.len();
            assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]), "BOM in {name}");
            assert!(!bytes.contains(&b'\r'), "CR in {name}");
            assert_eq!(bytes.last(), Some(&b'\n'), "terminal LF missing in {name}");
            assert_ne!(
                bytes.get(bytes.len().saturating_sub(2)),
                Some(&b'\n'),
                "extra LF in {name}"
            );
            assert_eq!(
                sha256(&bytes),
                expected.as_str().unwrap(),
                "hash mismatch for {name}"
            );
            if name.ends_with(".json") {
                serde_json::from_slice::<Value>(&bytes)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            }
        }
        assert_eq!(total, 248_079);
        let mut actual = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "FULL-PIN.json")
            .collect::<Vec<_>>();
        actual.sort_by(|left, right| compare_strings(left, right));
        let mut expected = files.keys().cloned().collect::<Vec<_>>();
        expected.sort_by(|left, right| compare_strings(left, right));
        assert_eq!(actual, expected);
    }
}
