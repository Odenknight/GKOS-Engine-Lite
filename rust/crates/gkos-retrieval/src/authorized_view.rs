//! Decision-A authorization-scoped projection over Full-owned receipts.
//!
//! Raw references are never interpreted here. Resolution selects only from
//! the ordered candidate tiers emitted by Full's one canonical resolver.

use std::collections::{BTreeMap, BTreeSet};

use crate::candidate::{
    GkxCandidateCategory, GkxCandidateChunk, GkxCandidateDeclaration, GkxCandidateSource,
};
use crate::contract::RETRIEVAL_PROVENANCE_CONTRACT;
use crate::digest::canonical_digest;
use crate::fusion::code_unit_compare;
use crate::provenance::{
    normalized_timestamp_millis, GkxAuthorizedTemporalSource, GkxStoredSourceProvenance,
    GkxTemporalState, GkxTemporalViewState, TemporalCoverage,
};
use crate::{RetrievalError, RetrievalResult};

const CONFLICT: &str = "RETRIEVAL_AUTHORIZED_VIEW_CONFLICT";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GkxAuthorizedCandidateView {
    pub(crate) sources: Vec<GkxStoredSourceProvenance>,
    pub(crate) temporal_sources: Vec<GkxAuthorizedTemporalSource>,
    pub(crate) eligible_record_keys: Vec<String>,
    pub(crate) eligible_candidate_chunk_keys: Vec<String>,
    pub(crate) authorized_source_count: u32,
    pub(crate) answerable_source_count: u32,
    pub(crate) coverage: TemporalCoverage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Resolution<'a> {
    Resolved(&'a str),
    SelfReference,
    Suppressed,
    Conflict,
}

pub(crate) fn build_authorized_candidate_view(
    candidate_sources: &[GkxCandidateSource],
    declarations: &[GkxCandidateDeclaration],
    candidate_chunks: &[GkxCandidateChunk],
    normalized_as_of: Option<&str>,
) -> RetrievalResult<GkxAuthorizedCandidateView> {
    let at = normalized_as_of
        .map(normalized_timestamp_millis)
        .transpose()?;
    let source_by_key = candidate_sources
        .iter()
        .map(|source| (source.record_key.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    if source_by_key.len() != candidate_sources.len()
        || candidate_chunks
            .iter()
            .any(|chunk| !source_by_key.contains_key(chunk.record_key.as_str()))
    {
        return Err(invalid("candidate input binding"));
    }

    let mut known = BTreeSet::new();
    let mut future = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    for source in candidate_sources {
        let Some(at) = at else {
            known.insert(source.record_key.as_str());
            continue;
        };
        let Some(valid_from) = source.valid_from.as_deref() else {
            unknown.insert(source.record_key.as_str());
            continue;
        };
        if normalized_timestamp_millis(valid_from)? > at {
            future.insert(source.record_key.as_str());
        } else {
            known.insert(source.record_key.as_str());
        }
    }

    let known_sources = candidate_sources
        .iter()
        .filter(|source| known.contains(source.record_key.as_str()))
        .collect::<Vec<_>>();
    unique(known_sources.iter().map(|source| source.source_id.as_str()))?;
    unique(
        known_sources
            .iter()
            .map(|source| source.source_path.as_str()),
    )?;
    let mut digest_by_fingerprint = BTreeMap::<&str, &str>::new();
    for source in &known_sources {
        if digest_by_fingerprint
            .insert(&source.parser_content_fingerprint, &source.source_digest)
            .is_some_and(|prior| prior != source.source_digest)
        {
            return Err(conflict());
        }
    }
    unique(
        candidate_chunks
            .iter()
            .filter(|chunk| known.contains(chunk.record_key.as_str()))
            .map(|chunk| chunk.chunk.chunk_id.as_str()),
    )?;

    let mut declarations_by_source = BTreeMap::<&str, Vec<&GkxCandidateDeclaration>>::new();
    let mut edges = BTreeSet::<(&str, &str)>::new();
    for declaration in declarations {
        if !source_by_key.contains_key(declaration.source_record_key.as_str()) {
            continue;
        }
        declarations_by_source
            .entry(declaration.source_record_key.as_str())
            .or_default()
            .push(declaration);
        if !known.contains(declaration.source_record_key.as_str())
            || declaration.category == GkxCandidateCategory::Link
        {
            continue;
        }
        let resolution = resolve(declaration, &known, &future, &unknown);
        let target = match resolution {
            Resolution::Suppressed => continue,
            Resolution::SelfReference
                if declaration.category == GkxCandidateCategory::Relationship
                    && declaration.field == "relationships.related_to" =>
            {
                continue
            }
            Resolution::Resolved(target) => target,
            Resolution::SelfReference | Resolution::Conflict => return Err(conflict()),
        };
        if declaration.category == GkxCandidateCategory::Lineage {
            let edge = if declaration.field == "supersedes" {
                (declaration.source_record_key.as_str(), target)
            } else {
                (target, declaration.source_record_key.as_str())
            };
            edges.insert(edge);
        }
    }

    let mut successors = BTreeMap::<&str, Vec<&str>>::new();
    let mut predecessors = BTreeMap::<&str, Vec<&str>>::new();
    for &(newer, older) in &edges {
        successors.entry(older).or_default().push(newer);
        predecessors.entry(newer).or_default().push(older);
        let newer_from = source_by_key[newer]
            .valid_from
            .as_deref()
            .map(normalized_timestamp_millis)
            .transpose()?;
        let older_from = source_by_key[older]
            .valid_from
            .as_deref()
            .map(normalized_timestamp_millis)
            .transpose()?;
        if matches!((newer_from, older_from), (Some(newer), Some(older)) if newer < older) {
            return Err(conflict());
        }
    }
    if successors.values().any(|items| items.len() > 1) || has_cycle(&edges) {
        return Err(conflict());
    }
    for values in successors.values_mut().chain(predecessors.values_mut()) {
        values.sort_by(|a, b| code_unit_compare(a, b));
        values.dedup();
    }

    let included = if at.is_none() {
        candidate_sources
            .iter()
            .map(|source| source.record_key.as_str())
            .collect::<BTreeSet<_>>()
    } else {
        known.union(&unknown).copied().collect()
    };
    let mut invalid_at = BTreeMap::<&str, Option<i64>>::new();
    for key in &included {
        // Full keeps a source whose canonical validity is unknown honestly
        // unbounded, even when an authorized scoped successor is resolved.
        // The edge remains visible lineage, but it cannot mint a temporal
        // boundary for a source with no canonical valid_from authority.
        let earliest = if source_by_key[key].valid_from.is_none() {
            None
        } else {
            successors
                .get(key)
                .into_iter()
                .flatten()
                .filter_map(|successor| {
                    source_by_key[*successor]
                        .valid_from
                        .as_deref()
                        .map(normalized_timestamp_millis)
                })
                .collect::<RetrievalResult<Vec<_>>>()?
                .into_iter()
                .min()
        };
        invalid_at.insert(key, earliest);
    }

    let mut eligible = BTreeSet::new();
    if let Some(at) = at {
        for key in &known {
            let from = source_by_key[*key]
                .valid_from
                .as_deref()
                .map(normalized_timestamp_millis)
                .transpose()?;
            let to = invalid_at.get(key).copied().flatten();
            if from.is_some_and(|from| from <= at) && to.is_none_or(|to| at < to) {
                eligible.insert(*key);
            }
        }
    } else {
        eligible.extend(included.iter().copied());
    }

    let mut stored_by_key = BTreeMap::new();
    let mut temporal_by_key = BTreeMap::new();
    for key in &included {
        let source = source_by_key[key];
        let resolved_supersedes = sorted_uids(
            predecessors
                .get(key)
                .into_iter()
                .flatten()
                .map(|endpoint| source_by_key[endpoint].source_id.clone()),
        );
        let resolved_superseded_by = sorted_uids(
            successors
                .get(key)
                .into_iter()
                .flatten()
                .map(|endpoint| source_by_key[endpoint].source_id.clone()),
        );
        let authored = declarations_by_source.get(key).cloned().unwrap_or_default();
        let authored_supersedes = sorted_uids(
            authored
                .iter()
                .filter(|item| {
                    item.category == GkxCandidateCategory::Lineage
                        && item.origin == crate::candidate::GkxCandidateOrigin::Authored
                        && item.field == "supersedes"
                })
                .map(|item| item.raw_reference.clone()),
        );
        let authored_superseded_by = sorted_uids(
            authored
                .iter()
                .filter(|item| {
                    item.category == GkxCandidateCategory::Lineage
                        && item.origin == crate::candidate::GkxCandidateOrigin::Authored
                        && item.field == "superseded_by"
                })
                .map(|item| item.raw_reference.clone()),
        );
        let to_millis = invalid_at.get(key).copied().flatten();
        let valid_to = to_millis.and_then(|_| {
            successors
                .get(key)
                .and_then(|items| items.first())
                .and_then(|endpoint| source_by_key[endpoint].valid_from.clone())
        });
        let temporal_state = if source.valid_from.is_none() {
            GkxTemporalState::Unknown
        } else if valid_to.is_some() {
            GkxTemporalState::Historical
        } else {
            GkxTemporalState::Current
        };
        let neutral = authored_supersedes.is_empty()
            && authored_superseded_by.is_empty()
            && resolved_supersedes.is_empty()
            && resolved_superseded_by.is_empty();
        let mut reasons = source.reason_codes.clone();
        reasons.push(
            if neutral {
                "LINEAGE_NEUTRAL"
            } else {
                "LINEAGE_PARTICIPANT"
            }
            .to_owned(),
        );
        reasons.sort_by(|a, b| code_unit_compare(a, b));
        reasons.dedup();
        let mut stored = GkxStoredSourceProvenance {
            contract_version: RETRIEVAL_PROVENANCE_CONTRACT.to_owned(),
            source_id: source.source_id.clone(),
            source_path: source.source_path.clone(),
            source_digest: source.source_digest.clone(),
            source_metadata: source.source_metadata.clone(),
            assertion_time: source.assertion_time.clone(),
            assertion_origin: source.assertion_origin,
            valid_from: source.valid_from.clone(),
            valid_to: valid_to.clone(),
            validity_origin: source.validity_origin,
            lineage_id: (),
            authored_supersedes,
            authored_superseded_by,
            resolved_supersedes: resolved_supersedes.clone(),
            resolved_superseded_by: resolved_superseded_by.clone(),
            lineage_neutral: neutral,
            temporal_state,
            ledger_binding_verified: false,
            reason_codes: reasons,
            provenance_digest: String::new(),
        };
        stored.provenance_digest = digest_without(&stored, "provenance_digest")?;
        stored.validate()?;
        let view_state = match temporal_state {
            GkxTemporalState::Current => GkxTemporalViewState::Current,
            GkxTemporalState::Historical => GkxTemporalViewState::Historical,
            GkxTemporalState::Unknown => GkxTemporalViewState::Unknown,
        };
        temporal_by_key.insert(
            *key,
            GkxAuthorizedTemporalSource {
                source_id: source.source_id.clone(),
                valid_from: source.valid_from.clone(),
                valid_to,
                temporal_state: view_state,
                supersedes: resolved_supersedes,
                superseded_by: resolved_superseded_by,
            },
        );
        stored_by_key.insert(*key, stored);
    }

    let mut eligible_record_keys = eligible
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    eligible_record_keys.sort_by(|a, b| code_unit_compare(a, b));
    let mut eligible_candidate_chunk_keys = candidate_chunks
        .iter()
        .filter(|chunk| eligible.contains(chunk.record_key.as_str()))
        .map(|chunk| chunk.candidate_chunk_key.clone())
        .collect::<Vec<_>>();
    eligible_candidate_chunk_keys.sort_by(|a, b| code_unit_compare(a, b));
    let mut sources = eligible
        .iter()
        .map(|key| stored_by_key[key].clone())
        .collect::<Vec<_>>();
    sources.sort_by(|a, b| code_unit_compare(&a.source_id, &b.source_id));
    let mut temporal_sources = eligible
        .iter()
        .map(|key| temporal_by_key[key].clone())
        .collect::<Vec<_>>();
    temporal_sources.sort_by(|a, b| code_unit_compare(&a.source_id, &b.source_id));
    let authorized_source_count =
        u32::try_from(included.len()).map_err(|_| invalid("too many authorized candidates"))?;
    let answerable_source_count = u32::try_from(
        included
            .iter()
            .filter(|key| source_by_key[**key].valid_from.is_some())
            .count(),
    )
    .map_err(|_| invalid("too many answerable candidates"))?;
    let coverage = match at {
        None => TemporalCoverage::NotRequested,
        Some(_) if authorized_source_count == 0 => TemporalCoverage::NotEvaluated,
        Some(_) if answerable_source_count != authorized_source_count || eligible.is_empty() => {
            TemporalCoverage::Insufficient
        }
        Some(_) => TemporalCoverage::Sufficient,
    };
    Ok(GkxAuthorizedCandidateView {
        sources,
        temporal_sources,
        eligible_record_keys,
        eligible_candidate_chunk_keys,
        authorized_source_count,
        answerable_source_count,
        coverage,
    })
}

fn resolve<'a>(
    declaration: &'a GkxCandidateDeclaration,
    known: &BTreeSet<&str>,
    future: &BTreeSet<&str>,
    unknown: &BTreeSet<&str>,
) -> Resolution<'a> {
    let mut saw_future = false;
    let mut saw_unknown = false;
    for tier in &declaration.resolution_tiers {
        let candidates = tier
            .candidate_record_keys
            .iter()
            .filter(|key| known.contains(key.as_str()))
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [one] if one.as_str() == declaration.source_record_key => {
                return Resolution::SelfReference
            }
            [one] => return Resolution::Resolved(one),
            [] => {}
            _ => return Resolution::Conflict,
        }
        for key in &tier.candidate_record_keys {
            saw_future |= future.contains(key.as_str());
            saw_unknown |= unknown.contains(key.as_str());
        }
    }
    if saw_future || saw_unknown {
        Resolution::Suppressed
    } else {
        Resolution::Conflict
    }
}

fn unique<'a>(values: impl IntoIterator<Item = &'a str>) -> RetrievalResult<()> {
    let mut seen = BTreeSet::new();
    if values.into_iter().any(|value| !seen.insert(value)) {
        return Err(conflict());
    }
    Ok(())
}

fn has_cycle(edges: &BTreeSet<(&str, &str)>) -> bool {
    let mut adjacent = BTreeMap::<&str, Vec<&str>>::new();
    for &(newer, older) in edges {
        adjacent.entry(newer).or_default().push(older);
    }
    let mut state = BTreeMap::<&str, u8>::new();
    for &start in adjacent.keys() {
        if state.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack = vec![(start, 0_usize)];
        state.insert(start, 1);
        while let Some(&(key, offset)) = stack.last() {
            let next = adjacent
                .get(key)
                .and_then(|items| items.get(offset))
                .copied();
            if let Some(next) = next {
                stack.last_mut().expect("stack is nonempty").1 += 1;
                match state.get(next).copied().unwrap_or(0) {
                    1 => return true,
                    0 => {
                        state.insert(next, 1);
                        stack.push((next, 0));
                    }
                    _ => {}
                }
            } else {
                state.insert(key, 2);
                stack.pop();
            }
        }
    }
    false
}

fn sorted_uids(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_by(|a, b| code_unit_compare(a, b));
    values.dedup();
    values
}

fn digest_without<T: serde::Serialize>(value: &T, field: &str) -> RetrievalResult<String> {
    let mut value = serde_json::to_value(value)?;
    value
        .as_object_mut()
        .ok_or_else(|| invalid("digest object"))?
        .remove(field);
    canonical_digest(&value)
}

fn conflict() -> RetrievalError {
    RetrievalError::ProjectionMismatch(CONFLICT.to_owned())
}

fn invalid(message: &str) -> RetrievalError {
    RetrievalError::ProjectionMismatch(format!("GKX_RETRIEVAL_AUTHORIZED_VIEW_INVALID:{message}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::candidate::{
        candidate_chunk_key, GkxCandidateCategory, GkxCandidateDeclaration, GkxCandidateOrigin,
        GkxResolutionBasis, GkxResolutionTier,
    };
    use crate::contract::GkxSensitivity;
    use crate::lineage_store::test_support::{generation, source};

    const A: &str = "018f0000-0000-7000-8000-000000000401";
    const B: &str = "018f0000-0000-7000-8000-000000000402";
    const C: &str = "018f0000-0000-7000-8000-000000000403";

    fn fixture() -> crate::lineage_store::GkxRetrievalGenerationInput {
        let pairs = vec![
            source(
                A,
                "a.md",
                "# A\nPolicy A\n",
                "2026-07-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            source(
                B,
                "b.md",
                "# B\nPolicy B\n",
                "2026-07-02T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
            source(
                C,
                "c.md",
                "# C\nPolicy C\n",
                "2026-08-01T00:00:00.000Z",
                GkxSensitivity::Public,
            ),
        ];
        let ids = crate::lineage_store::test_support::chunks(
            &pairs.iter().map(|pair| pair.0.clone()).collect::<Vec<_>>(),
        )
        .into_iter()
        .map(|chunk| chunk.chunk_id)
        .collect();
        generation(PathBuf::from("unused"), pairs, ids, vec![], None)
    }

    fn declaration(
        source: &str,
        category: GkxCandidateCategory,
        field: &str,
        mut targets: Vec<String>,
    ) -> GkxCandidateDeclaration {
        targets.sort_by(|a, b| code_unit_compare(a, b));
        GkxCandidateDeclaration {
            source_record_key: source.to_owned(),
            category,
            field: field.to_owned(),
            origin: GkxCandidateOrigin::Authored,
            declaration_index: 0,
            raw_reference: "reference".to_owned(),
            resolution_tiers: vec![
                GkxResolutionTier {
                    basis: GkxResolutionBasis::UidExact,
                    candidate_record_keys: targets,
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

    fn is_conflict(result: RetrievalResult<GkxAuthorizedCandidateView>) -> bool {
        matches!(result,Err(RetrievalError::ProjectionMismatch(message)) if message==CONFLICT)
    }

    #[test]
    fn future_or_hidden_identity_collision_is_exactly_absent_but_all_visible_conflicts() {
        let mut input = fixture();
        let c_index = input
            .candidate_sources
            .iter()
            .position(|item| item.source_id == C)
            .unwrap();
        input.candidate_sources[c_index].source_id = A.to_owned();
        input.candidate_sources[c_index].candidate_digest =
            input.candidate_sources[c_index].expected_digest().unwrap();
        for chunk in input
            .candidate_chunks
            .iter_mut()
            .filter(|item| item.record_key == input.candidate_sources[c_index].record_key)
        {
            chunk.chunk.source_id = A.to_owned();
        }
        let c_key = input.candidate_sources[c_index].record_key.clone();
        let baseline_sources = input
            .candidate_sources
            .iter()
            .filter(|item| item.record_key != c_key)
            .cloned()
            .collect::<Vec<_>>();
        let baseline_chunks = input
            .candidate_chunks
            .iter()
            .filter(|item| item.record_key != c_key)
            .cloned()
            .collect::<Vec<_>>();
        let baseline = build_authorized_candidate_view(
            &baseline_sources,
            &[],
            &baseline_chunks,
            Some("2026-07-15T00:00:00.000Z"),
        )
        .unwrap();
        let future = build_authorized_candidate_view(
            &input.candidate_sources,
            &[],
            &input.candidate_chunks,
            Some("2026-07-15T00:00:00.000Z"),
        )
        .unwrap();
        assert_eq!(future, baseline);
        let hidden = build_authorized_candidate_view(
            &baseline_sources,
            &[],
            &baseline_chunks,
            Some("2026-07-15T00:00:00.000Z"),
        )
        .unwrap();
        assert_eq!(hidden, baseline);
        assert!(is_conflict(build_authorized_candidate_view(
            &input.candidate_sources,
            &[],
            &input.candidate_chunks,
            Some("2026-08-15T00:00:00.000Z")
        )));
    }

    #[test]
    fn scoped_receipt_uses_first_known_tier_and_all_visible_ambiguity_is_generic() {
        let input = fixture();
        let a = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == A)
            .unwrap();
        let b = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == B)
            .unwrap();
        let c = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == C)
            .unwrap();
        let receipt = declaration(
            &a.record_key,
            GkxCandidateCategory::Relationship,
            "relationships.supports",
            vec![b.record_key.clone(), c.record_key.clone()],
        );
        let before_future = build_authorized_candidate_view(
            &input.candidate_sources,
            std::slice::from_ref(&receipt),
            &input.candidate_chunks,
            Some("2026-07-15T00:00:00.000Z"),
        )
        .unwrap();
        assert_eq!(before_future.eligible_record_keys.len(), 2);
        assert!(is_conflict(build_authorized_candidate_view(
            &input.candidate_sources,
            &[receipt],
            &input.candidate_chunks,
            Some("2026-08-15T00:00:00.000Z")
        )));
    }

    #[test]
    fn long_lineage_chain_is_checked_iteratively_without_stack_growth() {
        let nodes = (0..25_000)
            .map(|index| format!("record-{index:05}"))
            .collect::<Vec<_>>();
        let mut edges = (1..nodes.len())
            .map(|index| (nodes[index].as_str(), nodes[index - 1].as_str()))
            .collect::<BTreeSet<_>>();
        assert!(!has_cycle(&edges));
        edges.insert((nodes[0].as_str(), nodes[nodes.len() - 1].as_str()));
        assert!(has_cycle(&edges));
    }

    #[test]
    fn branch_cycle_order_fingerprint_and_public_chunk_conflicts_share_one_code() {
        let input = fixture();
        let a = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == A)
            .unwrap();
        let b = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == B)
            .unwrap();
        let c = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == C)
            .unwrap();
        let branch = vec![
            declaration(
                &b.record_key,
                GkxCandidateCategory::Lineage,
                "supersedes",
                vec![a.record_key.clone()],
            ),
            declaration(
                &c.record_key,
                GkxCandidateCategory::Lineage,
                "supersedes",
                vec![a.record_key.clone()],
            ),
        ];
        assert!(is_conflict(build_authorized_candidate_view(
            &input.candidate_sources,
            &branch,
            &input.candidate_chunks,
            None
        )));

        let cycle = vec![
            declaration(
                &a.record_key,
                GkxCandidateCategory::Lineage,
                "supersedes",
                vec![b.record_key.clone()],
            ),
            declaration(
                &b.record_key,
                GkxCandidateCategory::Lineage,
                "supersedes",
                vec![a.record_key.clone()],
            ),
        ];
        assert!(is_conflict(build_authorized_candidate_view(
            &input.candidate_sources,
            &cycle,
            &input.candidate_chunks,
            None
        )));

        let mut fingerprint_sources = input.candidate_sources.clone();
        let a_index = fingerprint_sources
            .iter()
            .position(|item| item.source_id == A)
            .unwrap();
        let b_index = fingerprint_sources
            .iter()
            .position(|item| item.source_id == B)
            .unwrap();
        fingerprint_sources[b_index].parser_content_fingerprint = fingerprint_sources[a_index]
            .parser_content_fingerprint
            .clone();
        fingerprint_sources[b_index].candidate_digest =
            fingerprint_sources[b_index].expected_digest().unwrap();
        assert!(is_conflict(build_authorized_candidate_view(
            &fingerprint_sources,
            &[],
            &input.candidate_chunks,
            None
        )));

        let mut collision_chunks = input.candidate_chunks.clone();
        let first = collision_chunks
            .iter()
            .find(|item| item.record_key == a.record_key)
            .unwrap()
            .chunk
            .chunk_id
            .clone();
        let target = collision_chunks
            .iter_mut()
            .find(|item| item.record_key == c.record_key)
            .unwrap();
        target.chunk.chunk_id = first;
        target.candidate_chunk_key =
            candidate_chunk_key(&target.record_key, &target.chunk.chunk_id).unwrap();
        assert!(is_conflict(build_authorized_candidate_view(
            &input.candidate_sources,
            &[],
            &collision_chunks,
            None
        )));
    }

    #[test]
    fn ordinary_links_do_not_mint_governed_conflicts() {
        let input = fixture();
        let source = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == A)
            .unwrap();
        let b = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == B)
            .unwrap();
        let c = input
            .candidate_sources
            .iter()
            .find(|item| item.source_id == C)
            .unwrap();
        let mut aliases = vec![b.record_key.clone(), c.record_key.clone()];
        aliases.sort_by(|a, b| code_unit_compare(a, b));
        let link = GkxCandidateDeclaration {
            source_record_key: source.record_key.clone(),
            category: GkxCandidateCategory::Link,
            field: "links.wikilink".to_owned(),
            origin: GkxCandidateOrigin::Authored,
            declaration_index: 0,
            raw_reference: "Ambiguous".to_owned(),
            resolution_tiers: vec![
                GkxResolutionTier {
                    basis: GkxResolutionBasis::PathExact,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::PathRelative,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::PathWithoutExtensionExact,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::PathWithoutExtensionRelative,
                    candidate_record_keys: vec![],
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::Alias,
                    candidate_record_keys: aliases,
                },
                GkxResolutionTier {
                    basis: GkxResolutionBasis::BasenameTitle,
                    candidate_record_keys: vec![],
                },
            ],
        };
        assert!(build_authorized_candidate_view(
            &input.candidate_sources,
            &[link],
            &input.candidate_chunks,
            None
        )
        .is_ok());
    }
}
