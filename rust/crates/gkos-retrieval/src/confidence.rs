use std::collections::BTreeSet;

use crate::contract::{
    ConfidenceLevel, RetrievalConfidence, RetrievalProviderStageStatus, RetrievalStageScores,
    RetrievalStageState,
};

/// Draft reason-code calibration. Signals are ranking inputs, never probabilities.
pub fn assess_retrieval_confidence(
    scores: &[RetrievalStageScores],
    vector: &RetrievalProviderStageStatus,
    reranker: &RetrievalProviderStageStatus,
    eligible_count: usize,
    stale: bool,
) -> RetrievalConfidence {
    let mut reasons = BTreeSet::new();
    if scores.is_empty() {
        reasons.insert(if eligible_count == 0 {
            "NO_ELIGIBLE_RESULTS"
        } else {
            "WEAK_LEXICAL_MATCH"
        });
    }
    if vector.state == RetrievalStageState::Degraded {
        reasons.insert("VECTOR_UNAVAILABLE");
    }
    if reranker.state == RetrievalStageState::Degraded {
        reasons.insert("RERANKER_UNAVAILABLE");
    }
    if stale {
        reasons.insert("STALE_PROJECTION");
    }
    let first = scores.first();
    if first.is_some_and(|first| {
        first.lexical_rank.is_some() && !first.lexical_score.is_some_and(|value| value > 0.0)
    }) {
        reasons.insert("WEAK_LEXICAL_MATCH");
    }
    if first.is_some_and(|first| {
        first
            .lexical_rank
            .zip(first.semantic_rank)
            .is_some_and(|(lexical, semantic)| lexical.abs_diff(semantic) > 5)
    }) {
        reasons.insert("RANK_DISAGREEMENT");
    }
    let mut level = match first {
        None => ConfidenceLevel::Insufficient,
        Some(first)
            if (first.lexical_rank == Some(1)
                && first.lexical_score.is_some_and(|value| value > 0.0))
                || (first.semantic_rank == Some(1)
                    && first.semantic_score.is_some_and(|value| value > 0.0))
                || first.reranker_score.is_some_and(|value| value > 0.0) =>
        {
            ConfidenceLevel::High
        }
        Some(first)
            if first.lexical_score.is_some_and(|value| value > 0.0)
                || first.semantic_score.is_some_and(|value| value > 0.0)
                || first.reranker_score.is_some_and(|value| value > 0.0) =>
        {
            ConfidenceLevel::Medium
        }
        Some(_) => ConfidenceLevel::Low,
    };
    if stale && level == ConfidenceLevel::High {
        level = ConfidenceLevel::Medium;
    }
    RetrievalConfidence {
        level,
        low_confidence: matches!(level, ConfidenceLevel::Low | ConfidenceLevel::Insufficient),
        reason_codes: reasons.into_iter().map(str::to_owned).collect(),
        lexical_signal: first.and_then(|scores| scores.lexical_score),
        semantic_signal: first.and_then(|scores| scores.semantic_score),
        reranker_signal: first.and_then(|scores| scores.reranker_score),
        coverage_signal: (eligible_count > 0)
            .then_some((scores.len() as f64 / eligible_count as f64).min(1.0)),
    }
}

#[cfg(test)]
mod tests {
    use crate::contract::{RetrievalProviderStageKind, RetrievalStageState};

    use super::*;

    fn stage(state: RetrievalStageState) -> RetrievalProviderStageStatus {
        RetrievalProviderStageStatus {
            kind: RetrievalProviderStageKind::None,
            state,
            provider_id: None,
            model_id: None,
            reason_codes: Vec::new(),
        }
    }

    fn score() -> RetrievalStageScores {
        RetrievalStageScores {
            lexical_score: Some(2.0),
            semantic_score: None,
            fusion_score: 1.0 / 61.0,
            reranker_score: None,
            mmr_score: None,
            lexical_rank: Some(1),
            semantic_rank: None,
            fused_rank: 1,
            reranker_rank: None,
            final_rank: 1,
        }
    }

    #[test]
    fn no_hits_distinguish_no_eligibility_from_weak_match() {
        let disabled = stage(RetrievalStageState::Disabled);
        let none = assess_retrieval_confidence(&[], &disabled, &disabled, 0, false);
        assert_eq!(none.level, ConfidenceLevel::Insufficient);
        assert_eq!(none.reason_codes, ["NO_ELIGIBLE_RESULTS"]);
        let weak = assess_retrieval_confidence(&[], &disabled, &disabled, 2, false);
        assert_eq!(weak.reason_codes, ["WEAK_LEXICAL_MATCH"]);
    }

    #[test]
    fn provider_degradation_and_staleness_are_explicit() {
        let degraded = stage(RetrievalStageState::Degraded);
        let confidence = assess_retrieval_confidence(&[score()], &degraded, &degraded, 4, true);
        assert_eq!(confidence.level, ConfidenceLevel::Medium);
        assert!(!confidence.low_confidence);
        assert_eq!(
            confidence.reason_codes,
            [
                "RERANKER_UNAVAILABLE",
                "STALE_PROJECTION",
                "VECTOR_UNAVAILABLE"
            ]
        );
        assert_eq!(confidence.coverage_signal, Some(0.25));
    }

    #[test]
    fn rank_one_without_a_positive_signal_is_low_confidence() {
        let disabled = stage(RetrievalStageState::Disabled);
        let mut zero = score();
        zero.lexical_score = Some(0.0);
        let confidence = assess_retrieval_confidence(&[zero], &disabled, &disabled, 1, false);
        assert_eq!(confidence.level, ConfidenceLevel::Low);
        assert!(confidence.low_confidence);
        assert_eq!(confidence.reason_codes, ["WEAK_LEXICAL_MATCH"]);
        assert_eq!(confidence.lexical_signal, Some(0.0));
        assert_eq!(confidence.coverage_signal, Some(1.0));
    }
}
