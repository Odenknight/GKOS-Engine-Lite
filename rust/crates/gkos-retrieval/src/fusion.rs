use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::contract::{RankedCandidate, MMR_DEFAULT_LAMBDA, RRF_DEFAULT_K};
use crate::{RetrievalError, RetrievalResult};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RankedInput {
    pub chunk_id: String,
    pub source_id: String,
    pub score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<Vec<f64>>,
}

pub fn reciprocal_rank_fusion(
    lexical_input: &[RankedInput],
    semantic_input: &[RankedInput],
    k: u64,
) -> RetrievalResult<Vec<RankedCandidate>> {
    if k == 0 || k > 9_007_199_254_740_991 {
        return Err(RetrievalError::InvalidConfig(
            "RRF k must be a positive safe integer".to_owned(),
        ));
    }
    let lexical = unique_ranked(lexical_input)?;
    let semantic = unique_ranked(semantic_input)?;
    let mut candidates = BTreeMap::<String, RankedCandidate>::new();
    add_stage(&mut candidates, &lexical, k, true)?;
    add_stage(&mut candidates, &semantic, k, false)?;
    let mut output = candidates.into_values().collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .fusion_score
            .total_cmp(&left.fusion_score)
            .then_with(|| code_unit_compare(&left.chunk_id, &right.chunk_id))
    });
    Ok(output)
}

pub fn reciprocal_rank_fusion_default(
    lexical_input: &[RankedInput],
    semantic_input: &[RankedInput],
) -> RetrievalResult<Vec<RankedCandidate>> {
    reciprocal_rank_fusion(lexical_input, semantic_input, RRF_DEFAULT_K)
}

fn unique_ranked(input: &[RankedInput]) -> RetrievalResult<Vec<RankedInput>> {
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for candidate in input {
        ensure_finite(
            candidate.score,
            &format!("score for {}", candidate.chunk_id),
        )?;
        if candidate.chunk_id.is_empty() || candidate.source_id.is_empty() {
            return Err(RetrievalError::InvalidEnvelope(
                "ranked candidate identifiers must not be empty".to_owned(),
            ));
        }
        if seen.insert(candidate.chunk_id.clone()) {
            output.push(candidate.clone());
        }
    }
    Ok(output)
}

fn add_stage(
    candidates: &mut BTreeMap<String, RankedCandidate>,
    input: &[RankedInput],
    k: u64,
    lexical: bool,
) -> RetrievalResult<()> {
    for (index, item) in input.iter().enumerate() {
        let rank = u32::try_from(index + 1)
            .map_err(|_| RetrievalError::InvalidConfig("too many ranked candidates".to_owned()))?;
        let candidate =
            candidates
                .entry(item.chunk_id.clone())
                .or_insert_with(|| RankedCandidate {
                    chunk_id: item.chunk_id.clone(),
                    source_id: item.source_id.clone(),
                    lexical_rank: None,
                    lexical_score: None,
                    semantic_rank: None,
                    semantic_score: None,
                    fusion_score: 0.0,
                    vector: None,
                    mmr_score: None,
                });
        if candidate.source_id != item.source_id {
            return Err(RetrievalError::InvalidEnvelope(format!(
                "chunk {} has conflicting source identities",
                item.chunk_id
            )));
        }
        if lexical {
            candidate.lexical_rank = Some(rank);
            candidate.lexical_score = Some(item.score);
        } else {
            candidate.semantic_rank = Some(rank);
            candidate.semantic_score = Some(item.score);
        }
        candidate.fusion_score += 1.0 / (k as f64 + f64::from(rank));
        if let Some(vector) = &item.vector {
            validate_vector(vector)?;
            candidate.vector = Some(vector.clone());
        }
    }
    Ok(())
}

pub fn cosine_similarity(left: &[f64], right: &[f64]) -> RetrievalResult<f64> {
    if left.is_empty() || left.len() != right.len() {
        return Err(RetrievalError::InvalidConfig(
            "cosine vectors must have equal nonzero dimensions".to_owned(),
        ));
    }
    validate_vector(left)?;
    validate_vector(right)?;
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return Ok(0.0);
    }
    let similarity = dot / (left_norm * right_norm).sqrt();
    ensure_finite(similarity, "cosine similarity")?;
    Ok(similarity)
}

pub fn maximal_marginal_relevance(
    input: &[RankedCandidate],
    limit: usize,
    lambda: f64,
) -> RetrievalResult<Vec<RankedCandidate>> {
    maximal_marginal_relevance_with_relevance(input, limit, lambda, None)
}

pub fn maximal_marginal_relevance_with_relevance(
    input: &[RankedCandidate],
    limit: usize,
    lambda: f64,
    relevance_by_chunk: Option<&BTreeMap<String, f64>>,
) -> RetrievalResult<Vec<RankedCandidate>> {
    if !lambda.is_finite() || !(0.0..=1.0).contains(&lambda) {
        return Err(RetrievalError::InvalidConfig(
            "MMR lambda must be within [0, 1]".to_owned(),
        ));
    }
    if limit == 0 || input.is_empty() {
        return Ok(Vec::new());
    }
    for candidate in input {
        ensure_finite(candidate.fusion_score, "fusion score")?;
        if let Some(vector) = &candidate.vector {
            validate_vector(vector)?;
        }
    }
    let maximum = input
        .iter()
        .map(|candidate| candidate.fusion_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut remaining = input.to_vec();
    let mut selected = Vec::<RankedCandidate>::new();
    while !remaining.is_empty() && selected.len() < limit {
        let mut best_index = 0;
        let mut best_score = f64::NEG_INFINITY;
        for (index, candidate) in remaining.iter().enumerate() {
            let relevance =
                match relevance_by_chunk.and_then(|values| values.get(&candidate.chunk_id)) {
                    Some(relevance) => {
                        ensure_finite(*relevance, "MMR relevance")?;
                        *relevance
                    }
                    None if maximum > 0.0 => candidate.fusion_score / maximum,
                    None => 0.0,
                };
            let mut similarity = 0.0_f64;
            if let Some(vector) = &candidate.vector {
                for prior in &selected {
                    if let Some(prior_vector) = &prior.vector {
                        similarity = similarity.max(cosine_similarity(vector, prior_vector)?);
                    }
                }
            }
            let score = lambda * relevance - (1.0 - lambda) * similarity;
            ensure_finite(score, "MMR score")?;
            let current = &remaining[best_index];
            if score > best_score
                || (score == best_score
                    && (candidate.fusion_score > current.fusion_score
                        || (candidate.fusion_score == current.fusion_score
                            && code_unit_compare(&candidate.chunk_id, &current.chunk_id)
                                == Ordering::Less)))
            {
                best_index = index;
                best_score = score;
            }
        }
        let mut winner = remaining.remove(best_index);
        winner.mmr_score = Some(best_score);
        selected.push(winner);
    }
    Ok(selected)
}

pub fn maximal_marginal_relevance_default(
    input: &[RankedCandidate],
    limit: usize,
) -> RetrievalResult<Vec<RankedCandidate>> {
    maximal_marginal_relevance(input, limit, MMR_DEFAULT_LAMBDA)
}

pub fn code_unit_compare(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn validate_vector(vector: &[f64]) -> RetrievalResult<()> {
    if vector.is_empty() {
        return Err(RetrievalError::InvalidConfig(
            "vectors must have nonzero dimensions".to_owned(),
        ));
    }
    for value in vector {
        ensure_finite(*value, "vector")?;
    }
    Ok(())
}

fn ensure_finite(value: f64, stage: &str) -> RetrievalResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(RetrievalError::NonFiniteScore(stage.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(chunk_id: &str, source_id: &str, score: f64, vector: &[f64]) -> RankedInput {
        RankedInput {
            chunk_id: chunk_id.to_owned(),
            source_id: source_id.to_owned(),
            score,
            vector: Some(vector.to_vec()),
        }
    }

    #[test]
    fn rrf_uses_one_based_k60_and_collapses_first_stage_duplicate() {
        let lexical = vec![
            item("chunk-a", "source-a", 0.9, &[0.0, 1.0]),
            item("chunk-b", "source-b", 0.8, &[1.0, 0.0]),
            item("chunk-a", "source-a", 0.1, &[0.0, 1.0]),
        ];
        let semantic = vec![
            item("chunk-b", "source-b", 0.95, &[1.0, 0.0]),
            item("chunk-c", "source-c", 0.7, &[0.9, 0.1]),
            item("chunk-a", "source-a", 0.6, &[0.0, 1.0]),
        ];
        let fused = reciprocal_rank_fusion_default(&lexical, &semantic).unwrap();
        assert_eq!(
            fused
                .iter()
                .map(|item| item.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["chunk-b", "chunk-a", "chunk-c"]
        );
        assert_eq!(fused[0].fusion_score, 0.03252247488101534);
        assert_eq!(fused[1].fusion_score, 0.032266458495966696);
        assert_eq!(fused[2].fusion_score, 0.016129032258064516);
        assert_eq!(fused[1].lexical_score, Some(0.9));
    }

    #[test]
    fn mmr_lambda_point_seven_matches_frozen_values() {
        let fused = reciprocal_rank_fusion_default(
            &[
                item("chunk-a", "source-a", 0.9, &[0.0, 1.0]),
                item("chunk-b", "source-b", 0.8, &[1.0, 0.0]),
            ],
            &[
                item("chunk-b", "source-b", 0.95, &[1.0, 0.0]),
                item("chunk-c", "source-c", 0.7, &[0.9, 0.1]),
                item("chunk-a", "source-a", 0.6, &[0.0, 1.0]),
            ],
        )
        .unwrap();
        let selected = maximal_marginal_relevance_default(&fused, 3).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|item| item.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["chunk-b", "chunk-a", "chunk-c"]
        );
        assert_eq!(selected[0].mmr_score, Some(0.7));
        assert_eq!(selected[1].mmr_score, Some(0.6944896115627822));
        assert_eq!(selected[2].mmr_score, Some(0.048989351142629645));
    }

    #[test]
    fn ties_use_utf16_code_unit_order() {
        let result = reciprocal_rank_fusion(
            &[
                RankedInput {
                    chunk_id: "\u{10000}".to_owned(),
                    source_id: "one".to_owned(),
                    score: 1.0,
                    vector: None,
                },
                RankedInput {
                    chunk_id: "\u{e000}".to_owned(),
                    source_id: "two".to_owned(),
                    score: 1.0,
                    vector: None,
                },
            ],
            &[
                RankedInput {
                    chunk_id: "\u{e000}".to_owned(),
                    source_id: "two".to_owned(),
                    score: 1.0,
                    vector: None,
                },
                RankedInput {
                    chunk_id: "\u{10000}".to_owned(),
                    source_id: "one".to_owned(),
                    score: 1.0,
                    vector: None,
                },
            ],
            60,
        )
        .unwrap();
        assert_eq!(result[0].chunk_id, "\u{10000}");
    }

    #[test]
    fn nonfinite_inputs_fail() {
        assert!(reciprocal_rank_fusion(
            &[RankedInput {
                chunk_id: "a".to_owned(),
                source_id: "s".to_owned(),
                score: f64::NAN,
                vector: None
            }],
            &[],
            60,
        )
        .is_err());
    }

    #[test]
    fn negative_cosine_is_clamped_to_zero_for_diversity_penalty() {
        let input = vec![
            RankedCandidate {
                chunk_id: "first".to_owned(),
                source_id: "source-first".to_owned(),
                lexical_rank: Some(1),
                lexical_score: Some(1.0),
                semantic_rank: None,
                semantic_score: None,
                fusion_score: 1.0,
                vector: Some(vec![1.0, 0.0]),
                mmr_score: None,
            },
            RankedCandidate {
                chunk_id: "opposite".to_owned(),
                source_id: "source-opposite".to_owned(),
                lexical_rank: Some(2),
                lexical_score: Some(0.5),
                semantic_rank: None,
                semantic_score: None,
                fusion_score: 0.5,
                vector: Some(vec![-1.0, 0.0]),
                mmr_score: None,
            },
        ];
        let selected = maximal_marginal_relevance(&input, 2, 0.7).unwrap();
        assert_eq!(selected[1].mmr_score, Some(0.35));
    }
}
