use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::contract::{
    is_sha256_digest, is_valid_authored_uid, is_valid_retrieval_source_path, RetrievalChunk,
    RetrievalChunkMetadata,
};
use crate::digest::{canonical_digest, canonical_json};
use crate::fusion::code_unit_compare;
use crate::provenance::{normalized_timestamp_millis, GkxAssertionOrigin, GkxValidityOrigin};
use crate::sqlite_store::trim_ecmascript_whitespace;
use crate::{RetrievalError, RetrievalResult};

pub(crate) const RETRIEVAL_CANDIDATE_SOURCE_CONTRACT: &str =
    "gkos-retrieval-candidate-source/1.0.0-draft.1";

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const RELATION_TYPES: &[&str] = &[
    "supports",
    "contradicts",
    "depends_on",
    "derived_from",
    "derives_from",
    "cites",
    "quotes",
    "interprets",
    "tests",
    "replicates",
    "fails_to_replicate",
    "extends",
    "narrows",
    "generalizes",
    "implements",
    "governed_by",
    "reviewed_by",
    "approved_by",
    "supersedes",
    "superseded_by",
    "related_to",
    "part_of",
    "has_part",
    "refines",
    "blocks",
    "documents",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GkxCandidateCategory {
    Lineage,
    Relationship,
    Link,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GkxCandidateOrigin {
    Authored,
    Derived,
    Proposed,
    Approved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GkxResolutionBasis {
    UidExact,
    PathExact,
    PathRelative,
    PathWithoutExtensionExact,
    PathWithoutExtensionRelative,
    BasenameTitle,
    Alias,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GkxCandidateSource {
    pub(crate) contract_version: String,
    pub(crate) assertion_origin: Option<GkxAssertionOrigin>,
    pub(crate) assertion_time: Option<String>,
    pub(crate) lineage_id: (),
    pub(crate) parser_content_fingerprint: String,
    pub(crate) reason_codes: Vec<String>,
    pub(crate) record_key: String,
    pub(crate) source_digest: String,
    pub(crate) source_id: String,
    pub(crate) source_metadata: RetrievalChunkMetadata,
    pub(crate) source_path: String,
    pub(crate) valid_from: Option<String>,
    pub(crate) validity_origin: GkxValidityOrigin,
    pub(crate) candidate_digest: String,
}

impl GkxCandidateSource {
    pub(crate) fn validate(&self) -> RetrievalResult<()> {
        if self.contract_version != RETRIEVAL_CANDIDATE_SOURCE_CONTRACT
            || !is_record_key(&self.record_key)
            || !is_valid_authored_uid(&self.source_id)
            || !is_valid_retrieval_source_path(&self.source_path)
            || utf16_len(&self.source_path) > 4_096
            || !is_parser_fingerprint(&self.parser_content_fingerprint)
            || !is_sha256_digest(&self.source_digest)
            || !is_sha256_digest(&self.candidate_digest)
        {
            return Err(invalid("candidate source identity"));
        }
        canonical_digest(&self.source_metadata)?;
        self.source_metadata.validate_semantics()?;
        if self
            .source_metadata
            .title
            .as_deref()
            .is_none_or(|title| title.is_empty() || utf16_len(title) > 512)
            || self.source_metadata.sensitivity.is_none()
            || self.source_metadata.authoritative != Some(true)
        {
            return Err(invalid("candidate source metadata"));
        }
        match (&self.assertion_time, self.assertion_origin) {
            (None, None) if self.source_metadata.authored_at.is_none() => {}
            (Some(value), Some(GkxAssertionOrigin::GkxCreatedAt))
                if self.source_metadata.authored_at.as_deref() == Some(value) =>
            {
                normalized_timestamp_millis(value)?;
            }
            _ => return Err(invalid("candidate assertion binding")),
        }
        match (&self.valid_from, self.validity_origin) {
            (None, GkxValidityOrigin::Unknown) => {}
            (Some(value), origin) if origin != GkxValidityOrigin::Unknown => {
                normalized_timestamp_millis(value)?;
            }
            _ => return Err(invalid("candidate validity binding")),
        }
        match (
            self.assertion_time.as_deref(),
            self.valid_from.as_deref(),
            self.validity_origin,
        ) {
            (Some(assertion), Some(valid_from), GkxValidityOrigin::GkxAuthoredTimestamp)
                if assertion == valid_from => {}
            (Some(_), Some(_), _) | (_, _, GkxValidityOrigin::GkxAuthoredTimestamp) => {
                return Err(invalid("candidate authored validity binding"));
            }
            _ => {}
        }
        let mut expected_reasons = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            self.validity_origin.reason().to_owned(),
        ];
        if self.assertion_time.is_none() {
            expected_reasons.push("ASSERTION_TIME_UNAVAILABLE".to_owned());
        }
        expected_reasons.sort_by(|left, right| code_unit_compare(left, right));
        validate_sorted_unique_strings(&self.reason_codes, false)?;
        if self.reason_codes != expected_reasons {
            return Err(invalid("candidate reason codes"));
        }
        if self.expected_digest()? != self.candidate_digest {
            return Err(RetrievalError::ProjectionMismatch(
                "GKX_RETRIEVAL_CANDIDATE_DIGEST_MISMATCH".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn expected_digest(&self) -> RetrievalResult<String> {
        let mut value = serde_json::to_value(self)?;
        value
            .as_object_mut()
            .ok_or_else(|| invalid("candidate source object"))?
            .remove("candidate_digest");
        canonical_digest(&value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GkxResolutionTier {
    pub(crate) basis: GkxResolutionBasis,
    pub(crate) candidate_record_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GkxCandidateDeclaration {
    pub(crate) source_record_key: String,
    pub(crate) category: GkxCandidateCategory,
    pub(crate) field: String,
    pub(crate) origin: GkxCandidateOrigin,
    pub(crate) declaration_index: u64,
    pub(crate) raw_reference: String,
    pub(crate) resolution_tiers: Vec<GkxResolutionTier>,
}

impl GkxCandidateDeclaration {
    pub(crate) fn validate(&self) -> RetrievalResult<()> {
        if !is_record_key(&self.source_record_key)
            || self.declaration_index > MAX_SAFE_INTEGER
            || self.field.is_empty()
            || utf16_len(&self.field) > 128
            || has_control(&self.field)
            || self.raw_reference.is_empty()
            || utf16_len(&self.raw_reference) > 512
            || has_control(&self.raw_reference)
            || trim_ecmascript_whitespace(&self.raw_reference) != self.raw_reference
            || self.origin == GkxCandidateOrigin::Proposed
        {
            return Err(invalid("candidate declaration shape"));
        }
        let expected: &[GkxResolutionBasis] = match self.category {
            GkxCandidateCategory::Lineage => {
                if !matches!(self.field.as_str(), "supersedes" | "superseded_by") {
                    return Err(invalid("candidate lineage field"));
                }
                &[
                    GkxResolutionBasis::UidExact,
                    GkxResolutionBasis::PathExact,
                    GkxResolutionBasis::PathWithoutExtensionExact,
                    GkxResolutionBasis::BasenameTitle,
                    GkxResolutionBasis::Alias,
                ]
            }
            GkxCandidateCategory::Relationship => {
                let relation = self
                    .field
                    .strip_prefix("relationships.")
                    .filter(|relation| {
                        RELATION_TYPES.contains(relation)
                            && !matches!(*relation, "supersedes" | "superseded_by")
                    })
                    .ok_or_else(|| invalid("candidate relationship field"))?;
                if relation.is_empty() {
                    return Err(invalid("candidate relationship field"));
                }
                &[
                    GkxResolutionBasis::UidExact,
                    GkxResolutionBasis::PathExact,
                    GkxResolutionBasis::PathWithoutExtensionExact,
                    GkxResolutionBasis::BasenameTitle,
                    GkxResolutionBasis::Alias,
                ]
            }
            GkxCandidateCategory::Link => {
                if !matches!(
                    self.field.as_str(),
                    "links.wikilink" | "links.markdown" | "links.property"
                ) || self.origin != GkxCandidateOrigin::Authored
                {
                    return Err(invalid("candidate link field"));
                }
                &[
                    GkxResolutionBasis::PathExact,
                    GkxResolutionBasis::PathRelative,
                    GkxResolutionBasis::PathWithoutExtensionExact,
                    GkxResolutionBasis::PathWithoutExtensionRelative,
                    GkxResolutionBasis::Alias,
                    GkxResolutionBasis::BasenameTitle,
                ]
            }
        };
        if self.resolution_tiers.len() != expected.len() {
            return Err(invalid("candidate resolution tier count"));
        }
        for (tier, expected_basis) in self.resolution_tiers.iter().zip(expected) {
            if tier.basis != *expected_basis {
                return Err(invalid("candidate resolution tier order"));
            }
            validate_sorted_unique_strings(&tier.candidate_record_keys, true)?;
        }
        canonical_json(self)?;
        Ok(())
    }

    pub(crate) fn digest(&self) -> RetrievalResult<String> {
        self.validate()?;
        canonical_digest(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GkxCandidateChunk {
    pub(crate) candidate_chunk_key: String,
    pub(crate) record_key: String,
    pub(crate) parent_candidate_chunk_key: Option<String>,
    pub(crate) chunk: RetrievalChunk,
}

impl GkxCandidateChunk {
    pub(crate) fn validate(&self) -> RetrievalResult<()> {
        if !is_candidate_chunk_key(&self.candidate_chunk_key)
            || !is_record_key(&self.record_key)
            || self
                .parent_candidate_chunk_key
                .as_deref()
                .is_some_and(|key| !is_candidate_chunk_key(key))
            || self.chunk.valid_to.is_some()
            || self.chunk.lineage_id.is_some()
            || !self.chunk.supersedes.is_empty()
            || !self.chunk.superseded_by.is_empty()
            || candidate_chunk_key(&self.record_key, &self.chunk.chunk_id)?
                != self.candidate_chunk_key
        {
            return Err(invalid("candidate chunk binding"));
        }
        canonical_digest(&self.chunk.metadata)?;
        self.chunk.metadata.validate_semantics()?;
        canonical_json(self)?;
        Ok(())
    }

    pub(crate) fn digest(&self) -> RetrievalResult<String> {
        self.validate()?;
        canonical_digest(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GkxCandidateVector {
    pub(crate) candidate_chunk_key: String,
    pub(crate) vector: Vec<f64>,
}

impl GkxCandidateVector {
    pub(crate) fn validate(&self) -> RetrievalResult<()> {
        if !is_candidate_chunk_key(&self.candidate_chunk_key)
            || self.vector.iter().any(|value| !value.is_finite())
        {
            return Err(invalid("candidate vector"));
        }
        canonical_json(self)?;
        Ok(())
    }
}

pub(crate) fn candidate_chunk_key(record_key: &str, chunk_id: &str) -> RetrievalResult<String> {
    #[derive(Serialize)]
    struct Coordinate<'a> {
        record_key: &'a str,
        chunk_id: &'a str,
    }
    let digest = canonical_digest(&Coordinate {
        record_key,
        chunk_id,
    })?;
    Ok(format!(
        "gkx-candidate-chunk:{}",
        digest.trim_start_matches("sha256:")
    ))
}

pub(crate) fn is_record_key(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("gkx-record:") else {
        return false;
    };
    let Some((digest, ordinal)) = rest.split_once(':') else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        && !ordinal.is_empty()
        && ordinal.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn is_candidate_chunk_key(value: &str) -> bool {
    value
        .strip_prefix("gkx-candidate-chunk:")
        .is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

fn is_parser_fingerprint(value: &str) -> bool {
    let mut parts = value.split(':');
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase())
    };
    parts.next().is_some_and(valid) && parts.next().is_some_and(valid) && parts.next().is_none()
}

fn validate_sorted_unique_strings(values: &[String], record_keys: bool) -> RetrievalResult<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.is_empty()
            || utf16_len(value) > 512
            || has_control(value)
            || (record_keys && !is_record_key(value))
            || !seen.insert(value.as_str())
        {
            return Err(invalid("candidate sorted set"));
        }
    }
    if values
        .windows(2)
        .any(|pair| code_unit_compare(&pair[0], &pair[1]).is_ge())
    {
        return Err(invalid("candidate sorted set order"));
    }
    Ok(())
}

fn has_control(value: &str) -> bool {
    value
        .chars()
        .any(|character| matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}'))
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn invalid(message: &str) -> RetrievalError {
    RetrievalError::InvalidEnvelope(format!("GKX_RETRIEVAL_CANDIDATE_INVALID:{message}"))
}
