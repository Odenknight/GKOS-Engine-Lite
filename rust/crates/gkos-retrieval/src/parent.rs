use std::collections::BTreeMap;

use crate::contract::{RetrievalChunk, RetrievalParentContext, SourceCitation};
use crate::{RetrievalError, RetrievalResult};

pub const DEFAULT_MAX_PARENT_BYTES: usize = 8_192;

/// Expand to the chunker's frozen nearest-ancestor pointer without changing
/// the winning chunk or its citation.
pub fn expand_parent(
    winner: &RetrievalChunk,
    eligible_chunks: &[RetrievalChunk],
    max_parent_bytes: usize,
) -> RetrievalResult<Option<RetrievalParentContext>> {
    if !(256..=65_536).contains(&max_parent_bytes) {
        return Err(RetrievalError::InvalidConfig(
            "max_parent_bytes must be within [256, 65536]".to_owned(),
        ));
    }
    let Some(parent_id) = winner.parent_chunk_id.as_deref() else {
        return Ok(None);
    };
    let chunks = eligible_chunks
        .iter()
        .map(|chunk| (chunk.chunk_id.as_str(), chunk))
        .collect::<BTreeMap<_, _>>();
    let Some(parent) = chunks.get(parent_id).copied() else {
        // Absence from the eligible set is indistinguishable from absence in
        // the corpus; never reveal a hidden parent's existence.
        return Ok(None);
    };
    if parent.source_id != winner.source_id || parent.heading_depth >= winner.heading_depth {
        return Err(RetrievalError::InvalidEnvelope(
            "parent expansion crosses source or structural ancestry".to_owned(),
        ));
    }
    if parent.text.len() > max_parent_bytes {
        return Ok(None);
    }
    Ok(Some(RetrievalParentContext {
        chunk_id: parent.chunk_id.clone(),
        text: parent.text.clone(),
        citation: SourceCitation::from(parent),
    }))
}

#[cfg(test)]
mod tests {
    use crate::chunker::{chunk_source, ChunkingOptions};
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, DiscoverabilityDecision,
        RetrievalChunkMetadata, RetrievalSource, RETRIEVAL_CONTRACT,
    };
    use crate::digest::sha256;

    use super::*;

    #[test]
    fn nearest_structural_parent_expands_without_changing_winner() {
        let text = "# Root\nParent body\n## Child\nWinning body";
        let source = RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: "019b2d14-4230-7db7-87d4-7d81cfaeca01".to_owned(),
            source_path: "notes/a.md".to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        };
        let chunks = chunk_source(&source, ChunkingOptions::default()).unwrap();
        let winner = chunks[1].clone();
        let winner_citation = SourceCitation::from(&winner);
        let parent = expand_parent(&winner, &chunks, DEFAULT_MAX_PARENT_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(parent.chunk_id, chunks[0].chunk_id);
        assert_eq!(SourceCitation::from(&winner), winner_citation);
    }

    #[test]
    fn absent_or_hidden_parent_is_suppressed_without_an_existence_error() {
        let text = "# Root\nParent body\n## Child\nWinning body";
        let source = RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: "019b2d14-4230-7db7-87d4-7d81cfaeca01".to_owned(),
            source_path: "notes/a.md".to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        };
        let chunks = chunk_source(&source, ChunkingOptions::default()).unwrap();
        assert!(
            expand_parent(&chunks[1], &chunks[1..], DEFAULT_MAX_PARENT_BYTES)
                .unwrap()
                .is_none()
        );
    }
}
