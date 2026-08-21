use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::contract::{
    is_sha256_digest, is_valid_authored_uid, is_valid_retrieval_source_path, RetrievalChunk,
    RetrievalChunkMetadata, RETRIEVAL_PROVENANCE_CONTRACT,
};
use crate::digest::canonical_digest;
use crate::fusion::code_unit_compare;
use crate::{RetrievalError, RetrievalResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GkxValidityOrigin {
    GkxAuthoredTimestamp,
    SourceCreatedTime,
    SourceModifiedTime,
    ProjectionReferenceTime,
    Unknown,
}
impl GkxValidityOrigin {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::GkxAuthoredTimestamp => "VALIDITY_FROM_GKX_AUTHORED_TIMESTAMP",
            Self::SourceCreatedTime => "VALIDITY_FROM_SOURCE_CREATED_TIME",
            Self::SourceModifiedTime => "VALIDITY_FROM_SOURCE_MODIFIED_TIME",
            Self::ProjectionReferenceTime => "VALIDITY_FROM_PROJECTION_REFERENCE_TIME",
            Self::Unknown => "VALIDITY_UNKNOWN",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GkxAssertionOrigin {
    GkxCreatedAt,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GkxTemporalState {
    Current,
    Historical,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GkxTemporalViewState {
    Current,
    Historical,
    Future,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalCoverage {
    NotRequested,
    NotEvaluated,
    Sufficient,
    Insufficient,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxStoredSourceProvenance {
    pub contract_version: String,
    pub source_id: String,
    pub source_path: String,
    pub source_digest: String,
    pub source_metadata: RetrievalChunkMetadata,
    pub assertion_time: Option<String>,
    pub assertion_origin: Option<GkxAssertionOrigin>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub validity_origin: GkxValidityOrigin,
    pub lineage_id: (),
    pub authored_supersedes: Vec<String>,
    pub authored_superseded_by: Vec<String>,
    pub resolved_supersedes: Vec<String>,
    pub resolved_superseded_by: Vec<String>,
    pub lineage_neutral: bool,
    pub temporal_state: GkxTemporalState,
    pub ledger_binding_verified: bool,
    pub reason_codes: Vec<String>,
    pub provenance_digest: String,
}

impl GkxStoredSourceProvenance {
    pub fn validate(&self) -> RetrievalResult<()> {
        if self.contract_version != RETRIEVAL_PROVENANCE_CONTRACT {
            return Err(RetrievalError::ContractMismatch {
                expected: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
                actual: self.contract_version.clone(),
            });
        }
        if !is_valid_authored_uid(&self.source_id)
            || !is_valid_retrieval_source_path(&self.source_path)
            || utf16_len(&self.source_path) > 4_096
            || !is_sha256_digest(&self.source_digest)
            || !is_sha256_digest(&self.provenance_digest)
        {
            return Err(invalid("identity or source binding"));
        }
        canonical_digest(&self.source_metadata)?;
        self.source_metadata.validate_semantics()?;
        if self
            .source_metadata
            .title
            .as_ref()
            .is_none_or(|title| title.is_empty() || utf16_len(title) > 512)
            || self.source_metadata.sensitivity.is_none()
            || self.source_metadata.authoritative != Some(true)
        {
            return Err(invalid("required source metadata"));
        }
        match (&self.assertion_time, self.assertion_origin) {
            (None, None) if self.source_metadata.authored_at.is_none() => {}
            (Some(value), Some(GkxAssertionOrigin::GkxCreatedAt))
                if self.source_metadata.authored_at.as_deref() == Some(value) =>
            {
                require_normalized_timestamp(value)?;
            }
            _ => return Err(invalid("assertion binding")),
        }
        let from = optional_normalized_timestamp(self.valid_from.as_deref())?;
        let to = optional_normalized_timestamp(self.valid_to.as_deref())?;
        if from.is_none() && to.is_some()
            || matches!((from, to), (Some(from), Some(to)) if to < from)
        {
            return Err(invalid("validity interval"));
        }
        match (
            self.assertion_time.as_deref(),
            self.valid_from.as_deref(),
            self.validity_origin,
        ) {
            (Some(assertion), Some(valid_from), GkxValidityOrigin::GkxAuthoredTimestamp)
                if assertion == valid_from => {}
            (Some(_), Some(_), _) | (_, _, GkxValidityOrigin::GkxAuthoredTimestamp) => {
                return Err(invalid("authored assertion/validity binding"));
            }
            _ => {}
        }
        match (from, to, self.validity_origin, self.temporal_state) {
            (None, None, GkxValidityOrigin::Unknown, GkxTemporalState::Unknown) => {}
            (Some(_), None, origin, GkxTemporalState::Current)
                if origin != GkxValidityOrigin::Unknown => {}
            (Some(_), Some(_), origin, GkxTemporalState::Historical)
                if origin != GkxValidityOrigin::Unknown => {}
            _ => return Err(invalid("temporal state")),
        }
        if self.ledger_binding_verified {
            return Err(invalid("derived identity or ledger authority"));
        }
        validate_set(&self.authored_supersedes, false)?;
        validate_set(&self.authored_superseded_by, false)?;
        validate_set(&self.resolved_supersedes, true)?;
        validate_set(&self.resolved_superseded_by, true)?;
        validate_set(&self.reason_codes, false)?;
        let neutral = self.authored_supersedes.is_empty()
            && self.authored_superseded_by.is_empty()
            && self.resolved_supersedes.is_empty()
            && self.resolved_superseded_by.is_empty();
        if self.lineage_neutral != neutral {
            return Err(invalid("lineage-neutral flag"));
        }
        let mut reasons = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            self.validity_origin.reason().to_owned(),
            if neutral {
                "LINEAGE_NEUTRAL".to_owned()
            } else {
                "LINEAGE_PARTICIPANT".to_owned()
            },
        ];
        if self.assertion_time.is_none() {
            reasons.push("ASSERTION_TIME_UNAVAILABLE".to_owned());
        }
        reasons.sort_by(|a, b| code_unit_compare(a, b));
        if self.reason_codes != reasons {
            return Err(invalid("reason codes"));
        }
        if digest_without_field(self, "provenance_digest")? != self.provenance_digest {
            return Err(RetrievalError::ProjectionMismatch(
                "GKX_RETRIEVAL_PROVENANCE_DIGEST_MISMATCH".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GkxAuthorizedTemporalSource {
    pub(crate) source_id: String,
    pub(crate) valid_from: Option<String>,
    pub(crate) valid_to: Option<String>,
    pub(crate) temporal_state: GkxTemporalViewState,
    pub(crate) supersedes: Vec<String>,
    pub(crate) superseded_by: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GkxAuthorizedTemporalView {
    pub(crate) sources: Vec<GkxAuthorizedTemporalSource>,
    pub(crate) eligible_source_ids: Vec<String>,
    pub(crate) authorized_source_count: u32,
    pub(crate) answerable_source_count: u32,
    pub(crate) coverage: TemporalCoverage,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxPublicProvenance {
    pub(crate) contract_version: String,
    pub(crate) source_id: String,
    pub(crate) source_path: String,
    pub(crate) source_digest: String,
    pub(crate) assertion_time: Option<String>,
    pub(crate) assertion_origin: Option<GkxAssertionOrigin>,
    pub(crate) valid_from: Option<String>,
    pub(crate) valid_to: Option<String>,
    pub(crate) validity_origin: GkxValidityOrigin,
    pub(crate) lineage_id: (),
    pub(crate) supersedes: Vec<String>,
    pub(crate) superseded_by: Vec<String>,
    pub(crate) temporal_state: GkxTemporalState,
    pub(crate) ledger_binding_verified: bool,
    pub(crate) lineage_neutral: bool,
    pub(crate) reason_codes: Vec<String>,
    pub(crate) assertion: GkxProvenanceAssertion,
    pub(crate) interval_semantics: String,
    pub(crate) provenance_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GkxProvenanceAssertion {
    pub(crate) chunk_id: String,
    pub(crate) content_digest: String,
}

impl GkxPublicProvenance {
    pub fn validate(&self) -> RetrievalResult<()> {
        if self.contract_version != RETRIEVAL_PROVENANCE_CONTRACT
            || !is_valid_authored_uid(&self.source_id)
            || !is_valid_retrieval_source_path(&self.source_path)
            || !is_sha256_digest(&self.source_digest)
            || !is_sha256_digest(&self.assertion.chunk_id)
            || !is_sha256_digest(&self.assertion.content_digest)
            || !is_sha256_digest(&self.provenance_digest)
            || self.ledger_binding_verified
            || self.interval_semantics != "[valid_from,valid_to)"
        {
            return Err(invalid("public envelope"));
        }
        match (&self.assertion_time, self.assertion_origin) {
            (None, None) => {}
            (Some(value), Some(GkxAssertionOrigin::GkxCreatedAt)) => {
                require_normalized_timestamp(value)?;
            }
            _ => return Err(invalid("public assertion binding")),
        }
        let from = optional_normalized_timestamp(self.valid_from.as_deref())?;
        let to = optional_normalized_timestamp(self.valid_to.as_deref())?;
        match (from, to, self.validity_origin, self.temporal_state) {
            (None, None, GkxValidityOrigin::Unknown, GkxTemporalState::Unknown) => {}
            (Some(_), None, origin, GkxTemporalState::Current)
                if origin != GkxValidityOrigin::Unknown => {}
            (Some(from), Some(to), origin, GkxTemporalState::Historical)
                if origin != GkxValidityOrigin::Unknown && to >= from => {}
            _ => return Err(invalid("public temporal state")),
        }
        validate_set(&self.supersedes, true)?;
        validate_set(&self.superseded_by, true)?;
        validate_set(&self.reason_codes, false)?;
        let neutral = self.supersedes.is_empty() && self.superseded_by.is_empty();
        let mut expected = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            "LINEAGE_VIEW_AUTHORIZED_ONLY".to_owned(),
            self.validity_origin.reason().to_owned(),
            (if neutral {
                "LINEAGE_NEUTRAL"
            } else {
                "LINEAGE_PARTICIPANT"
            })
            .to_owned(),
        ];
        if self.assertion_time.is_none() {
            expected.push("ASSERTION_TIME_UNAVAILABLE".to_owned());
        }
        if self
            .reason_codes
            .iter()
            .any(|code| code == "TEMPORAL_SELECTION_AS_OF")
        {
            expected.push("TEMPORAL_SELECTION_AS_OF".to_owned());
        }
        expected.sort_by(|a, b| code_unit_compare(a, b));
        if self.reason_codes != expected
            || self.lineage_neutral != neutral
            || digest_without_field(self, "provenance_digest")? != self.provenance_digest
        {
            return Err(invalid("public digest or lineage flag"));
        }
        Ok(())
    }

    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn source_path(&self) -> &str {
        &self.source_path
    }
    pub fn valid_from(&self) -> Option<&str> {
        self.valid_from.as_deref()
    }
    pub fn valid_to(&self) -> Option<&str> {
        self.valid_to.as_deref()
    }
    pub fn supersedes(&self) -> &[String] {
        &self.supersedes
    }
    pub fn superseded_by(&self) -> &[String] {
        &self.superseded_by
    }
    pub fn provenance_digest(&self) -> &str {
        &self.provenance_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointInTimeProjection {
    pub valid: Vec<String>,
    pub historical: Vec<String>,
    pub future: Vec<String>,
    pub unknown: Vec<String>,
}

pub fn normalize_retrieval_as_of(value: &str) -> RetrievalResult<String> {
    format_utc_timestamp(parse_gkx_timestamp(value)?)
}

pub(crate) fn source_valid_at(
    source: &GkxStoredSourceProvenance,
    normalized_as_of: &str,
) -> RetrievalResult<bool> {
    source.validate()?;
    let at = require_normalized_timestamp(normalized_as_of)?;
    let Some(from) = optional_normalized_timestamp(source.valid_from.as_deref())? else {
        return Ok(false);
    };
    let to = optional_normalized_timestamp(source.valid_to.as_deref())?;
    Ok(from <= at && to.is_none_or(|end| at < end))
}

pub(crate) fn project_stored_intervals_at(
    sources: &[GkxStoredSourceProvenance],
    normalized_as_of: &str,
) -> RetrievalResult<PointInTimeProjection> {
    let at = require_normalized_timestamp(normalized_as_of)?;
    let mut result = PointInTimeProjection {
        valid: vec![],
        historical: vec![],
        future: vec![],
        unknown: vec![],
    };
    for source in sources {
        source.validate()?;
        let Some(from) = optional_normalized_timestamp(source.valid_from.as_deref())? else {
            result.unknown.push(source.source_id.clone());
            continue;
        };
        let to = optional_normalized_timestamp(source.valid_to.as_deref())?;
        if from > at {
            result.future.push(source.source_id.clone());
        } else if to.is_some_and(|end| at >= end) {
            result.historical.push(source.source_id.clone());
        } else {
            result.valid.push(source.source_id.clone());
        }
    }
    for values in [
        &mut result.valid,
        &mut result.historical,
        &mut result.future,
        &mut result.unknown,
    ] {
        values.sort_by(|a, b| code_unit_compare(a, b));
    }
    Ok(result)
}

/// Restrict already-canonical host envelopes without resolving authored refs,
/// choosing lineage topology, or creating identity/authority.
pub(crate) fn build_authorized_temporal_view(
    sources: &[GkxStoredSourceProvenance],
    normalized_as_of: Option<&str>,
) -> RetrievalResult<GkxAuthorizedTemporalView> {
    let at = normalized_as_of
        .map(require_normalized_timestamp)
        .transpose()?;
    let mut by_id = BTreeMap::new();
    for source in sources {
        source.validate()?;
        if by_id.insert(source.source_id.clone(), source).is_some() {
            return Err(invalid("duplicate authorized source"));
        }
    }
    // Only already-resolved canonical endpoints within the authorized,
    // non-future corpus may influence this view. This is restriction, not
    // identity resolution or a new lineage authority.
    let mut known_visible = Vec::new();
    let mut visible_ids = BTreeSet::new();
    let mut valid_from_by_id = BTreeMap::new();
    for source in sources {
        let Some(from) = optional_normalized_timestamp(source.valid_from.as_deref())? else {
            continue;
        };
        if at.is_none_or(|instant| from <= instant) {
            known_visible.push(source);
            visible_ids.insert(source.source_id.clone());
            valid_from_by_id.insert(source.source_id.clone(), from);
        }
    }

    // If both canonical endpoints survive authorization/time restriction, the
    // inverse declarations must agree. Missing endpoints are suppressed rather
    // than treated as evidence about hidden records.
    for source in sources {
        for older in &source.resolved_supersedes {
            if let Some(endpoint) = by_id.get(older) {
                if !endpoint.resolved_superseded_by.contains(&source.source_id) {
                    return Err(invalid("canonical lineage inverse"));
                }
            }
        }
        for newer in &source.resolved_superseded_by {
            if let Some(endpoint) = by_id.get(newer) {
                if !endpoint.resolved_supersedes.contains(&source.source_id) {
                    return Err(invalid("canonical lineage inverse"));
                }
            }
        }
    }

    let mut scoped_supersedes = BTreeMap::<String, Vec<String>>::new();
    let mut scoped_superseded_by = BTreeMap::<String, Vec<String>>::new();
    for source in &known_visible {
        for older in &source.resolved_supersedes {
            if !visible_ids.contains(older) {
                continue;
            }
            scoped_supersedes
                .entry(source.source_id.clone())
                .or_default()
                .push(older.clone());
            scoped_superseded_by
                .entry(older.clone())
                .or_default()
                .push(source.source_id.clone());
        }
    }
    for values in scoped_supersedes
        .values_mut()
        .chain(scoped_superseded_by.values_mut())
    {
        values.sort_by(|a, b| code_unit_compare(a, b));
        values.dedup();
    }

    // Mirror GKX computeTemporalState: invalid_at is the earliest direct
    // successor valid_at that is not before its predecessor. Malformed earlier
    // branches remain canonical inputs but cannot create a negative interval.
    let mut scoped_invalid_at = BTreeMap::<String, Option<i64>>::new();
    for source in &known_visible {
        let from = valid_from_by_id[&source.source_id];
        let invalid_at = scoped_superseded_by
            .get(&source.source_id)
            .into_iter()
            .flatten()
            .filter_map(|successor| valid_from_by_id.get(successor).copied())
            .filter(|successor_from| *successor_from >= from)
            .min();
        scoped_invalid_at.insert(source.source_id.clone(), invalid_at);
    }
    let mut rows = Vec::new();
    let mut eligible = Vec::new();
    let mut answerable = 0_u32;
    for source in sources {
        let from = optional_normalized_timestamp(source.valid_from.as_deref())?;
        if from.is_some() {
            answerable = answerable.saturating_add(1);
        }
        let visible = visible_ids.contains(&source.source_id);
        let scoped_to = visible
            .then(|| scoped_invalid_at.get(&source.source_id).copied().flatten())
            .flatten();
        let state = match (from, visible, scoped_to) {
            (None, _, _) => GkxTemporalViewState::Unknown,
            (Some(_), false, _) => GkxTemporalViewState::Future,
            (Some(_), true, Some(_)) => GkxTemporalViewState::Historical,
            (Some(_), true, None) => GkxTemporalViewState::Current,
        };
        let valid = match (at, from, scoped_to) {
            (None, _, _) => true,
            (Some(at), Some(from), to) => visible && from <= at && to.is_none_or(|end| at < end),
            (Some(_), None, _) => false,
        };
        if valid {
            eligible.push(source.source_id.clone());
        }
        rows.push(GkxAuthorizedTemporalSource {
            source_id: source.source_id.clone(),
            valid_from: source.valid_from.clone(),
            valid_to: scoped_to.map(format_utc_timestamp).transpose()?,
            temporal_state: state,
            supersedes: if visible {
                scoped_supersedes
                    .get(&source.source_id)
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![]
            },
            superseded_by: if visible {
                scoped_superseded_by
                    .get(&source.source_id)
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![]
            },
        });
    }
    rows.sort_by(|a, b| code_unit_compare(&a.source_id, &b.source_id));
    eligible.sort_by(|a, b| code_unit_compare(a, b));
    let authorized_source_count =
        u32::try_from(sources.len()).map_err(|_| invalid("too many sources"))?;
    let coverage = match at {
        None => TemporalCoverage::NotRequested,
        Some(_) if sources.is_empty() => TemporalCoverage::NotEvaluated,
        Some(_) if answerable != authorized_source_count || eligible.is_empty() => {
            TemporalCoverage::Insufficient
        }
        Some(_) => TemporalCoverage::Sufficient,
    };
    Ok(GkxAuthorizedTemporalView {
        sources: rows,
        eligible_source_ids: eligible,
        authorized_source_count,
        answerable_source_count: answerable,
        coverage,
    })
}

pub(crate) fn build_public_provenance(
    stored: &GkxStoredSourceProvenance,
    chunk: &RetrievalChunk,
    temporal: &GkxAuthorizedTemporalSource,
    normalized_as_of: Option<&str>,
) -> RetrievalResult<GkxPublicProvenance> {
    stored.validate()?;
    if stored.source_id != chunk.source_id
        || stored.source_path != chunk.source_path
        || stored.source_digest != chunk.source_digest
        || stored.valid_from != chunk.valid_from
        || stored.valid_to != chunk.valid_to
        || chunk.lineage_id.is_some()
        || stored.resolved_supersedes != chunk.supersedes
        || stored.resolved_superseded_by != chunk.superseded_by
        || temporal.source_id != stored.source_id
        || temporal.valid_from != stored.valid_from
    {
        return Err(RetrievalError::ProjectionMismatch(
            "GKX_RETRIEVAL_PROVENANCE_BINDING_MISMATCH".to_owned(),
        ));
    }
    if temporal
        .supersedes
        .iter()
        .any(|id| !stored.resolved_supersedes.contains(id))
        || temporal
            .superseded_by
            .iter()
            .any(|id| !stored.resolved_superseded_by.contains(id))
    {
        return Err(RetrievalError::ProjectionMismatch(
            "GKX_RETRIEVAL_AUTHORIZED_TEMPORAL_ENDPOINT_BINDING_MISMATCH".to_owned(),
        ));
    }
    let temporal_state = match temporal.temporal_state {
        GkxTemporalViewState::Current => GkxTemporalState::Current,
        GkxTemporalViewState::Historical => GkxTemporalState::Historical,
        GkxTemporalViewState::Unknown => GkxTemporalState::Unknown,
        GkxTemporalViewState::Future => {
            return Err(RetrievalError::ProjectionMismatch(
                "future provenance cannot be returned".to_owned(),
            ))
        }
    };
    if let Some(as_of) = normalized_as_of {
        let at = require_normalized_timestamp(as_of)?;
        let from = optional_normalized_timestamp(temporal.valid_from.as_deref())?;
        let to = optional_normalized_timestamp(temporal.valid_to.as_deref())?;
        if from.is_none_or(|from| from > at) || to.is_some_and(|to| at >= to) {
            return Err(RetrievalError::ProjectionMismatch(
                "temporal selection does not contain as_of".to_owned(),
            ));
        }
    }
    let neutral = temporal.supersedes.is_empty() && temporal.superseded_by.is_empty();
    let mut reasons = stored
        .reason_codes
        .iter()
        .filter(|reason| !matches!(reason.as_str(), "LINEAGE_NEUTRAL" | "LINEAGE_PARTICIPANT"))
        .cloned()
        .collect::<Vec<_>>();
    reasons.extend([
        "LINEAGE_VIEW_AUTHORIZED_ONLY".to_owned(),
        (if neutral {
            "LINEAGE_NEUTRAL"
        } else {
            "LINEAGE_PARTICIPANT"
        })
        .to_owned(),
    ]);
    if normalized_as_of.is_some() {
        reasons.push("TEMPORAL_SELECTION_AS_OF".to_owned());
    }
    reasons.sort_by(|a, b| code_unit_compare(a, b));
    reasons.dedup();
    let mut result = SelfPublicBuilder {
        stored,
        chunk,
        temporal,
        temporal_state,
        neutral,
        reasons,
    }
    .build();
    result.provenance_digest = digest_without_field(&result, "provenance_digest")?;
    result.validate()?;
    Ok(result)
}

struct SelfPublicBuilder<'a> {
    stored: &'a GkxStoredSourceProvenance,
    chunk: &'a RetrievalChunk,
    temporal: &'a GkxAuthorizedTemporalSource,
    temporal_state: GkxTemporalState,
    neutral: bool,
    reasons: Vec<String>,
}
impl SelfPublicBuilder<'_> {
    fn build(self) -> GkxPublicProvenance {
        GkxPublicProvenance {
            contract_version: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
            source_id: self.stored.source_id.clone(),
            source_path: self.stored.source_path.clone(),
            source_digest: self.stored.source_digest.clone(),
            assertion_time: self.stored.assertion_time.clone(),
            assertion_origin: self.stored.assertion_origin,
            valid_from: self.temporal.valid_from.clone(),
            valid_to: self.temporal.valid_to.clone(),
            validity_origin: self.stored.validity_origin,
            lineage_id: (),
            supersedes: self.temporal.supersedes.clone(),
            superseded_by: self.temporal.superseded_by.clone(),
            temporal_state: self.temporal_state,
            ledger_binding_verified: false,
            lineage_neutral: self.neutral,
            reason_codes: self.reasons,
            assertion: GkxProvenanceAssertion {
                chunk_id: self.chunk.chunk_id.clone(),
                content_digest: self.chunk.content_digest.clone(),
            },
            interval_semantics: "[valid_from,valid_to)".to_owned(),
            provenance_digest: String::new(),
        }
    }
}

fn digest_without_field<T: Serialize>(value: &T, field: &str) -> RetrievalResult<String> {
    let mut json = serde_json::to_value(value)?;
    json.as_object_mut()
        .ok_or_else(|| invalid("digest envelope"))?
        .remove(field);
    canonical_digest(&json)
}

fn validate_set(values: &[String], canonical_uids: bool) -> RetrievalResult<()> {
    if values.len() > 65_536
        || values.iter().any(|value| {
            value.is_empty()
                || utf16_len(value) > 512
                || value
                    .chars()
                    .any(|character| character <= '\u{1f}' || character == '\u{7f}')
                || (canonical_uids && !is_valid_authored_uid(value))
        })
        || values
            .windows(2)
            .any(|pair| code_unit_compare(&pair[0], &pair[1]).is_ge())
    {
        return Err(invalid("sorted unique string set"));
    }
    Ok(())
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn invalid(message: &str) -> RetrievalError {
    RetrievalError::InvalidEnvelope(format!("GKX_RETRIEVAL_PROVENANCE_INVALID:{message}"))
}

fn optional_normalized_timestamp(value: Option<&str>) -> RetrievalResult<Option<i64>> {
    value.map(require_normalized_timestamp).transpose()
}

fn require_normalized_timestamp(value: &str) -> RetrievalResult<i64> {
    if value.len() != 24
        || value.as_bytes().get(19) != Some(&b'.')
        || value.as_bytes().get(23) != Some(&b'Z')
        || normalize_retrieval_as_of(value)? != value
    {
        return Err(invalid("timestamp is not normalized UTC"));
    }
    parse_gkx_timestamp(value)
}

pub(crate) fn normalized_timestamp_millis(value: &str) -> RetrievalResult<i64> {
    require_normalized_timestamp(value)
}

/// Exact current GKX timestamp grammar plus ECMAScript Date rollover behavior.
fn parse_gkx_timestamp(value: &str) -> RetrievalResult<i64> {
    let b = value.as_bytes();
    let fail = || RetrievalError::InvalidConfig("RETRIEVAL_AS_OF_INVALID".to_owned());
    if !value.is_ascii()
        || value
            .chars()
            .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
        || b.len() < 17
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
    {
        return Err(fail());
    }
    let year = digits(b, 0, 4)?;
    let month = digits(b, 5, 2)?;
    let day = digits(b, 8, 2)?;
    let mut hour = digits(b, 11, 2)?;
    let minute = digits(b, 14, 2)?;
    let mut cursor = 16;
    let mut second = 0;
    let mut has_seconds = false;
    if b.get(cursor) == Some(&b':') {
        has_seconds = true;
        second = digits(b, cursor + 1, 2)?;
        cursor += 3;
    }
    let mut millis = 0;
    let mut fractional_nonzero = false;
    if b.get(cursor) == Some(&b'.') {
        if !has_seconds {
            return Err(fail());
        }
        cursor += 1;
        let start = cursor;
        while b.get(cursor).is_some_and(u8::is_ascii_digit) {
            fractional_nonzero |= b[cursor] != b'0';
            cursor += 1;
        }
        if cursor == start {
            return Err(fail());
        }
        for offset in 0..3 {
            millis *= 10;
            if start + offset < cursor {
                millis += i64::from(b[start + offset] - b'0');
            }
        }
    }
    let offset = match b.get(cursor) {
        Some(b'Z') if cursor + 1 == b.len() => 0,
        Some(sign @ (b'+' | b'-')) if cursor + 6 == b.len() && b.get(cursor + 3) == Some(&b':') => {
            let h = digits(b, cursor + 1, 2)?;
            let m = digits(b, cursor + 4, 2)?;
            if h > 23 || m > 59 {
                return Err(fail());
            }
            let offset = h * 60 + m;
            if *sign == b'+' {
                offset
            } else {
                -offset
            }
        }
        _ => return Err(fail()),
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 24
        || minute > 59
        || second > 59
        || (hour == 24 && (minute != 0 || second != 0 || fractional_nonzero))
    {
        return Err(fail());
    }
    let days = days_from_civil(year, month, 1) + day - 1 + i64::from(hour == 24);
    if hour == 24 {
        hour = 0;
    }
    Ok(
        days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000 + millis
            - offset * 60_000,
    )
}

fn digits(bytes: &[u8], start: usize, length: usize) -> RetrievalResult<i64> {
    let slice = bytes
        .get(start..start + length)
        .filter(|slice| slice.iter().all(u8::is_ascii_digit))
        .ok_or_else(|| RetrievalError::InvalidConfig("RETRIEVAL_AS_OF_INVALID".to_owned()))?;
    Ok(slice
        .iter()
        .fold(0, |value, byte| value * 10 + i64::from(*byte - b'0')))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn format_utc_timestamp(milliseconds: i64) -> RetrievalResult<String> {
    let days = milliseconds.div_euclid(86_400_000);
    let within = milliseconds.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    if !(0..=9_999).contains(&year) {
        return Err(RetrievalError::InvalidConfig(
            "RETRIEVAL_AS_OF_INVALID".to_owned(),
        ));
    }
    let hour = within / 3_600_000;
    let minute = (within % 3_600_000) / 60_000;
    let second = (within % 60_000) / 1_000;
    let millis = within % 1_000;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunker::{chunk_lineage_source, ChunkingOptions};
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, GkxRetrievalSource, GkxSensitivity,
        RetrievalChunkMetadata, RETRIEVAL_CONTRACT,
    };

    const OLD: &str = "018f0000-0000-7000-8000-000000000201";
    const NEW: &str = "018f0000-0000-7000-8000-000000000202";

    fn metadata(title: &str, authored_at: &str) -> RetrievalChunkMetadata {
        RetrievalChunkMetadata {
            title: Some(title.to_owned()),
            authored_at: Some(authored_at.to_owned()),
            sensitivity: Some(GkxSensitivity::Public),
            authoritative: Some(true),
            ..RetrievalChunkMetadata::default()
        }
    }

    fn resign_stored(mut value: GkxStoredSourceProvenance) -> GkxStoredSourceProvenance {
        value.provenance_digest = digest_without_field(&value, "provenance_digest").unwrap();
        value
    }

    fn seal_stored(value: GkxStoredSourceProvenance) -> GkxStoredSourceProvenance {
        let value = resign_stored(value);
        value.validate().unwrap();
        value
    }

    fn stored(
        id: &str,
        path: &str,
        text: &str,
        valid_from: &str,
        valid_to: Option<&str>,
        supersedes: Vec<String>,
        superseded_by: Vec<String>,
    ) -> GkxStoredSourceProvenance {
        let neutral = supersedes.is_empty() && superseded_by.is_empty();
        let mut reasons = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            (if neutral {
                "LINEAGE_NEUTRAL"
            } else {
                "LINEAGE_PARTICIPANT"
            })
            .to_owned(),
            "VALIDITY_FROM_GKX_AUTHORED_TIMESTAMP".to_owned(),
        ];
        reasons.sort_by(|a, b| code_unit_compare(a, b));
        seal_stored(GkxStoredSourceProvenance {
            contract_version: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
            source_id: id.to_owned(),
            source_path: path.to_owned(),
            source_digest: crate::digest::sha256(text.as_bytes()),
            source_metadata: metadata(path, valid_from),
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
        })
    }

    fn binding(source: &GkxStoredSourceProvenance, text: &str) -> GkxRetrievalSource {
        GkxRetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "temporal-vault".to_owned(),
            source_id: source.source_id.clone(),
            source_path: source.source_path.clone(),
            source_digest: source.source_digest.clone(),
            text: text.to_owned(),
            lineage: CanonicalLineageEnvelope {
                lineage_id: None,
                lineage_neutral: source.lineage_neutral,
                supersedes: source.resolved_supersedes.clone(),
                superseded_by: source.resolved_superseded_by.clone(),
                reason_codes: vec![],
            },
            temporal: CanonicalTemporalEnvelope {
                assertion_time: source.assertion_time.clone(),
                valid_from: source.valid_from.clone(),
                valid_to: source.valid_to.clone(),
                valid_from_unix_ms: source
                    .valid_from
                    .as_deref()
                    .map(normalized_timestamp_millis)
                    .transpose()
                    .unwrap(),
                valid_to_unix_ms: source
                    .valid_to
                    .as_deref()
                    .map(normalized_timestamp_millis)
                    .transpose()
                    .unwrap(),
            },
            metadata: source.source_metadata.clone(),
        }
    }

    fn unknown_stored(id: &str, path: &str, text: &str) -> GkxStoredSourceProvenance {
        let mut reasons = vec![
            "ASSERTION_TIME_UNAVAILABLE".to_owned(),
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            "LINEAGE_NEUTRAL".to_owned(),
            "VALIDITY_UNKNOWN".to_owned(),
        ];
        reasons.sort_by(|a, b| code_unit_compare(a, b));
        seal_stored(GkxStoredSourceProvenance {
            contract_version: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
            source_id: id.to_owned(),
            source_path: path.to_owned(),
            source_digest: crate::digest::sha256(text.as_bytes()),
            source_metadata: RetrievalChunkMetadata {
                title: Some(path.to_owned()),
                sensitivity: Some(GkxSensitivity::Public),
                authoritative: Some(true),
                ..RetrievalChunkMetadata::default()
            },
            assertion_time: None,
            assertion_origin: None,
            valid_from: None,
            valid_to: None,
            validity_origin: GkxValidityOrigin::Unknown,
            lineage_id: (),
            authored_supersedes: vec![],
            authored_superseded_by: vec![],
            resolved_supersedes: vec![],
            resolved_superseded_by: vec![],
            lineage_neutral: true,
            temporal_state: GkxTemporalState::Unknown,
            ledger_binding_verified: false,
            reason_codes: reasons,
            provenance_digest: String::new(),
        })
    }

    #[test]
    fn timestamp_normalization_matches_gkx_fixture_cases() {
        for (input, expected) in [
            ("2026-08-01T12:34Z", "2026-08-01T12:34:00.000Z"),
            ("2026-08-01T08:34-04:00", "2026-08-01T12:34:00.000Z"),
            ("2026-08-01T12:34:56.123456Z", "2026-08-01T12:34:56.123Z"),
            ("2026-02-30T12:34Z", "2026-03-02T12:34:00.000Z"),
            ("2025-02-29T12:34Z", "2025-03-01T12:34:00.000Z"),
            ("2026-08-01T24:00Z", "2026-08-02T00:00:00.000Z"),
            ("2026-08-01T24:00:00.0000Z", "2026-08-02T00:00:00.000Z"),
        ] {
            assert_eq!(normalize_retrieval_as_of(input).unwrap(), expected);
        }
        for value in [
            "2026-08-01T12:34",
            "2026-08-01T24:01Z",
            "2026-08-01T12:34+24:00",
            "2026-08-01T12:34+00:60",
            "2026-08-01 12:34Z",
            "\t2026-08-01T12:34Z",
            "2026-08-01T12:34Z\0",
            "2026-08-01T12:34.5Z",
            "2026-08-01T24:00:00.0001Z",
            "0000-01-01T00:00+14:00",
            "9999-12-31T23:59-14:00",
        ] {
            assert!(normalize_retrieval_as_of(value).is_err(), "{value}");
        }
    }

    #[test]
    fn hidden_successor_is_absent_from_scoped_interval_and_public_digest() {
        let text = "# Old\nPolicy Café 😀\n";
        let with_hidden = stored(
            OLD,
            "old.md",
            text,
            "2026-07-01T00:00:00.000Z",
            Some("2026-08-01T00:00:00.000Z"),
            vec![],
            vec![NEW.to_owned()],
        );
        let absent = stored(
            OLD,
            "old.md",
            text,
            "2026-07-01T00:00:00.000Z",
            None,
            vec![],
            vec![],
        );
        let at = "2026-08-15T00:00:00.000Z";
        let related_view =
            build_authorized_temporal_view(std::slice::from_ref(&with_hidden), Some(at)).unwrap();
        let absent_view =
            build_authorized_temporal_view(std::slice::from_ref(&absent), Some(at)).unwrap();
        assert_eq!(related_view, absent_view);
        assert_eq!(related_view.eligible_source_ids, vec![OLD]);
        assert_eq!(related_view.sources[0].valid_to, None);
        assert_eq!(related_view.sources[0].superseded_by, Vec::<String>::new());

        let related_chunk =
            chunk_lineage_source(&binding(&with_hidden, text), ChunkingOptions::default())
                .unwrap()
                .remove(0);
        let absent_chunk =
            chunk_lineage_source(&binding(&absent, text), ChunkingOptions::default())
                .unwrap()
                .remove(0);
        assert_eq!(related_chunk.chunk_id, absent_chunk.chunk_id);
        let related_public = build_public_provenance(
            &with_hidden,
            &related_chunk,
            &related_view.sources[0],
            Some(at),
        )
        .unwrap();
        let absent_public =
            build_public_provenance(&absent, &absent_chunk, &absent_view.sources[0], Some(at))
                .unwrap();
        assert_eq!(related_public, absent_public);
        assert_eq!(related_public.valid_to(), None);
        assert_eq!(related_public.superseded_by(), &[] as &[String]);
    }

    #[test]
    fn half_open_boundary_selects_successor_and_suppresses_future_endpoint() {
        let old_text = "# Old\nPolicy\n";
        let new_text = "# New\nPolicy\n";
        let old = stored(
            OLD,
            "old.md",
            old_text,
            "2026-07-01T00:00:00.000Z",
            Some("2026-08-01T00:00:00.000Z"),
            vec![],
            vec![NEW.to_owned()],
        );
        let new = stored(
            NEW,
            "new.md",
            new_text,
            "2026-08-01T00:00:00.000Z",
            None,
            vec![OLD.to_owned()],
            vec![],
        );
        let before = build_authorized_temporal_view(
            &[old.clone(), new.clone()],
            Some("2026-07-31T23:59:59.999Z"),
        )
        .unwrap();
        assert_eq!(before.eligible_source_ids, vec![OLD]);
        let old_row = before
            .sources
            .iter()
            .find(|row| row.source_id == OLD)
            .unwrap();
        assert_eq!(old_row.valid_to, None);
        assert!(old_row.superseded_by.is_empty());

        let boundary =
            build_authorized_temporal_view(&[old, new], Some("2026-08-01T00:00:00.000Z")).unwrap();
        assert_eq!(boundary.eligible_source_ids, vec![NEW]);
        let old_row = boundary
            .sources
            .iter()
            .find(|row| row.source_id == OLD)
            .unwrap();
        assert_eq!(
            old_row.valid_to.as_deref(),
            Some("2026-08-01T00:00:00.000Z")
        );
    }

    #[test]
    fn insufficient_coverage_retains_known_temporal_ids_for_scoped_coordinate() {
        let known = stored(
            OLD,
            "known.md",
            "# Known\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            None,
            vec![],
            vec![],
        );
        let unknown = unknown_stored(NEW, "unknown.md", "# Unknown\nPolicy\n");
        let view =
            build_authorized_temporal_view(&[known, unknown], Some("2026-08-15T00:00:00.000Z"))
                .unwrap();
        assert_eq!(view.coverage, TemporalCoverage::Insufficient);
        assert_eq!(view.authorized_source_count, 2);
        assert_eq!(view.answerable_source_count, 1);
        assert_eq!(view.eligible_source_ids, vec![OLD]);
    }

    #[test]
    fn self_digested_forged_assertion_validity_bindings_are_rejected() {
        let base = stored(
            OLD,
            "old.md",
            "# Old\nPolicy\n",
            "2026-07-01T00:00:00.000Z",
            None,
            vec![],
            vec![],
        );

        let mut unequal = base.clone();
        unequal.assertion_time = Some("2026-06-30T00:00:00.000Z".to_owned());
        unequal.source_metadata.authored_at = unequal.assertion_time.clone();
        assert!(resign_stored(unequal).validate().is_err());

        let mut wrong_origin = base.clone();
        wrong_origin.validity_origin = GkxValidityOrigin::SourceCreatedTime;
        wrong_origin.reason_codes = vec![
            "LEDGER_BINDING_UNAVAILABLE".to_owned(),
            "LINEAGE_ID_UNAVAILABLE".to_owned(),
            "LINEAGE_NEUTRAL".to_owned(),
            "VALIDITY_FROM_SOURCE_CREATED_TIME".to_owned(),
        ];
        wrong_origin
            .reason_codes
            .sort_by(|a, b| code_unit_compare(a, b));
        assert!(resign_stored(wrong_origin).validate().is_err());

        let mut missing_validity = base;
        missing_validity.valid_from = None;
        missing_validity.temporal_state = GkxTemporalState::Unknown;
        assert!(resign_stored(missing_validity).validate().is_err());
    }
}
