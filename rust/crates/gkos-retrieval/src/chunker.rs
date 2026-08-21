use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::contract::{
    RetrievalChunk, RetrievalSource, CHUNKER_VERSION, MAX_CHUNK_BYTES, RETRIEVAL_CONTRACT,
};
use crate::digest::{canonical_digest, sha256};
use crate::{RetrievalError, RetrievalResult};

pub const DEFAULT_MAX_TOKENS: usize = 400;
pub const DEFAULT_OVERLAP_TOKENS: usize = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkingOptions {
    pub max_tokens: usize,
    pub overlap_tokens: usize,
}

impl Default for ChunkingOptions {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_MAX_TOKENS,
            overlap_tokens: DEFAULT_OVERLAP_TOKENS,
        }
    }
}

impl ChunkingOptions {
    pub fn validate(self) -> RetrievalResult<Self> {
        if !(16..=4096).contains(&self.max_tokens) {
            return Err(RetrievalError::InvalidConfig(
                "max_tokens must be within [16, 4096]".to_owned(),
            ));
        }
        if self.overlap_tokens >= self.max_tokens {
            return Err(RetrievalError::InvalidConfig(
                "overlap_tokens must be less than max_tokens".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteToken {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
struct LineRecord<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

#[derive(Clone, Debug)]
struct HeadingEvent {
    start: usize,
    depth: u8,
    title: String,
}

#[derive(Clone, Debug)]
struct Section {
    start: usize,
    end: usize,
    heading_path: Vec<String>,
    depth: u8,
    position: String,
    parent_position: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct Piece {
    start: usize,
    end: usize,
    tokens: usize,
}

#[derive(Serialize)]
struct ChunkIdentity<'a> {
    chunker: &'a str,
    content_digest: &'a str,
    contract: &'a str,
    part_ordinal: u32,
    source_id: &'a str,
    structural_position: &'a str,
}

pub fn ascii_whitespace_tokens(bytes: &[u8]) -> Vec<ByteToken> {
    let mut output = Vec::new();
    let mut start = None;
    for index in 0..=bytes.len() {
        let separated =
            index == bytes.len() || matches!(bytes[index], 0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20);
        if !separated && start.is_none() {
            start = Some(index);
        }
        if separated {
            if let Some(token_start) = start.take() {
                output.push(ByteToken {
                    start: token_start,
                    end: index,
                });
            }
        }
    }
    output
}

pub fn token_count(text: &str) -> usize {
    ascii_whitespace_tokens(text.as_bytes()).len()
}

pub(crate) fn chunk_identity(
    source_id: &str,
    structural_position: &str,
    part_ordinal: u32,
    content_digest: &str,
) -> RetrievalResult<String> {
    canonical_digest(&ChunkIdentity {
        chunker: CHUNKER_VERSION,
        content_digest,
        contract: RETRIEVAL_CONTRACT,
        part_ordinal,
        source_id,
        structural_position,
    })
}

pub(crate) fn line_number(text: &str, byte: usize) -> u32 {
    line_at(&byte_line_starts(text.as_bytes()), byte)
}

pub fn chunk_source(
    source: &RetrievalSource,
    options: ChunkingOptions,
) -> RetrievalResult<Vec<RetrievalChunk>> {
    source.validate_for_retrieval()?;
    let options = options.validate()?;
    let computed_source_digest = sha256(source.text.as_bytes());
    if source.source_digest.to_ascii_lowercase() != computed_source_digest {
        return Err(RetrievalError::InvalidEnvelope(
            "source_digest does not match the exact UTF-8 source bytes".to_owned(),
        ));
    }

    let source_bytes = source.text.as_bytes();
    let line_starts = byte_line_starts(source_bytes);
    let mut pending = Vec::<(RetrievalChunk, Option<String>)>::new();
    let mut ordinal = 0_u32;
    for section in sections_of(&source.text) {
        let section_bytes = &source_bytes[section.start..section.end];
        for (part_index, piece) in
            split_section(section_bytes, options.max_tokens, options.overlap_tokens)?
                .into_iter()
                .enumerate()
        {
            let start_byte = section.start + piece.start;
            let end_byte = section.start + piece.end;
            let text = std::str::from_utf8(&source_bytes[start_byte..end_byte])
                .map_err(|_| {
                    RetrievalError::InvalidEnvelope(
                        "chunk boundary split a UTF-8 sequence".to_owned(),
                    )
                })?
                .to_owned();
            if text.trim().is_empty() {
                continue;
            }
            let content_digest = sha256(text.as_bytes());
            let part_ordinal = u32::try_from(part_index + 1)
                .map_err(|_| RetrievalError::InvalidEnvelope("too many chunk parts".to_owned()))?;
            ordinal = ordinal.checked_add(1).ok_or_else(|| {
                RetrievalError::InvalidEnvelope("too many source chunks".to_owned())
            })?;
            let chunk_id = chunk_identity(
                &source.source_id,
                &section.position,
                part_ordinal,
                &content_digest,
            )?;
            pending.push((
                RetrievalChunk {
                    chunk_id,
                    source_id: source.source_id.clone(),
                    source_path: source.source_path.clone(),
                    source_digest: computed_source_digest.clone(),
                    heading_path: section.heading_path.clone(),
                    heading_depth: section.depth,
                    ordinal_within_source: ordinal,
                    structural_position: section.position.clone(),
                    part_ordinal,
                    start_byte: u64::try_from(start_byte).map_err(|_| {
                        RetrievalError::InvalidEnvelope("start_byte overflow".to_owned())
                    })?,
                    end_byte: u64::try_from(end_byte).map_err(|_| {
                        RetrievalError::InvalidEnvelope("end_byte overflow".to_owned())
                    })?,
                    start_line: line_at(&line_starts, start_byte),
                    end_line: line_at(&line_starts, end_byte.saturating_sub(1).max(start_byte)),
                    content_digest,
                    text,
                    token_count: u32::try_from(piece.tokens).map_err(|_| {
                        RetrievalError::InvalidEnvelope("token count overflow".to_owned())
                    })?,
                    parent_chunk_id: None,
                    lineage_id: source.lineage.lineage_id.clone(),
                    valid_from: source.temporal.valid_from.clone(),
                    valid_to: source.temporal.valid_to.clone(),
                    supersedes: source.lineage.supersedes.clone(),
                    superseded_by: source.lineage.superseded_by.clone(),
                    metadata: source.metadata.clone(),
                },
                section.parent_position.clone(),
            ));
        }
    }

    let mut first_by_position = BTreeMap::new();
    for (chunk, _) in &pending {
        first_by_position
            .entry(chunk.structural_position.clone())
            .or_insert_with(|| chunk.chunk_id.clone());
    }
    for (chunk, parent_position) in &mut pending {
        if let Some(parent_position) = parent_position {
            chunk.parent_chunk_id = first_by_position.get(parent_position).cloned();
        }
    }
    let chunks = pending
        .into_iter()
        .map(|(chunk, _)| chunk)
        .collect::<Vec<_>>();
    validate_chunk_set(source, &chunks)?;
    Ok(chunks)
}

pub fn validate_chunk_set(
    source: &RetrievalSource,
    chunks: &[RetrievalChunk],
) -> RetrievalResult<()> {
    source.validate_for_retrieval()?;
    let mut ids = BTreeSet::new();
    let mut by_id = BTreeMap::new();
    let mut first_by_position = BTreeMap::<&str, &RetrievalChunk>::new();
    let mut next_part_by_position = BTreeMap::<&str, u32>::new();
    for (index, chunk) in chunks.iter().enumerate() {
        chunk.validate_against(source)?;
        if !ids.insert(chunk.chunk_id.as_str()) {
            return Err(RetrievalError::InvalidEnvelope(
                "duplicate chunk_id in source generation".to_owned(),
            ));
        }
        let expected_ordinal = u32::try_from(index + 1)
            .map_err(|_| RetrievalError::InvalidEnvelope("too many source chunks".to_owned()))?;
        if chunk.ordinal_within_source != expected_ordinal {
            return Err(RetrievalError::InvalidEnvelope(
                "ordinal_within_source must be contiguous and one-based".to_owned(),
            ));
        }
        let next_part = next_part_by_position
            .entry(&chunk.structural_position)
            .or_insert(1);
        if chunk.part_ordinal != *next_part {
            return Err(RetrievalError::InvalidEnvelope(
                "part_ordinal must be contiguous and one-based within a structural position"
                    .to_owned(),
            ));
        }
        *next_part += 1;
        first_by_position
            .entry(&chunk.structural_position)
            .or_insert(chunk);
        by_id.insert(chunk.chunk_id.as_str(), chunk);
    }
    for chunk in chunks {
        let parent_position = chunk
            .structural_position
            .rsplit_once('.')
            .map(|(parent, _)| parent);
        match (parent_position, chunk.parent_chunk_id.as_deref()) {
            (None, None) => {}
            (Some(parent_position), Some(parent_id)) => {
                let expected_parent = first_by_position.get(parent_position).ok_or_else(|| {
                    RetrievalError::InvalidEnvelope(
                        "parent structural section has no first chunk".to_owned(),
                    )
                })?;
                if expected_parent.chunk_id != parent_id
                    || expected_parent.part_ordinal != 1
                    || !by_id.contains_key(parent_id)
                {
                    return Err(RetrievalError::InvalidEnvelope(
                        "parent_chunk_id must name the nearest ancestor's first chunk".to_owned(),
                    ));
                }
            }
            _ => {
                return Err(RetrievalError::InvalidEnvelope(
                    "parent_chunk_id does not match structural ancestry".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn lines_of(text: &str) -> Vec<LineRecord<'_>> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\n' && bytes[index] != b'\r' {
            index += 1;
            continue;
        }
        let width = if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            2
        } else {
            1
        };
        lines.push(LineRecord {
            start,
            end: index + width,
            text: &text[start..index],
        });
        index += width;
        start = index;
    }
    if start < bytes.len() || lines.is_empty() {
        lines.push(LineRecord {
            start,
            end: bytes.len(),
            text: &text[start..],
        });
    }
    lines
}

fn frontmatter_end(lines: &[LineRecord<'_>]) -> usize {
    let Some(first) = lines.first() else {
        return 0;
    };
    if first.text.trim_start_matches('\u{feff}').trim() != "---" {
        return 0;
    }
    lines
        .iter()
        .skip(1)
        .find(|line| matches!(line.text.trim(), "---" | "..."))
        .map_or(0, |line| line.end)
}

fn heading_events(lines: &[LineRecord<'_>], body_start: usize) -> Vec<HeadingEvent> {
    let mut events = Vec::new();
    let mut fence = None;
    let mut previous_eligible: Option<&LineRecord<'_>> = None;
    for line in lines.iter().filter(|line| line.start >= body_start) {
        if let Some(marker) = fence_marker(line.text) {
            match fence {
                None => fence = Some(marker),
                Some(open) if open == marker => fence = None,
                Some(_) => {}
            }
            previous_eligible = None;
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some((depth, title)) = atx_heading(line.text) {
            events.push(HeadingEvent {
                start: line.start,
                depth,
                title,
            });
            previous_eligible = None;
            continue;
        }
        if let Some(depth) = setext_depth(line.text) {
            if let Some(previous) =
                previous_eligible.filter(|previous| !previous.text.trim().is_empty())
            {
                events.push(HeadingEvent {
                    start: previous.start,
                    depth,
                    title: previous.text.trim().to_owned(),
                });
                previous_eligible = None;
                continue;
            }
        }
        previous_eligible = (!line.text.trim().is_empty()).then_some(line);
    }
    events.sort_by_key(|event| event.start);
    events
}

fn fence_marker(line: &str) -> Option<char> {
    let trimmed = line.trim_start_matches(char::is_whitespace);
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    (trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count()
        >= 3)
        .then_some(marker)
}

fn atx_heading(line: &str) -> Option<(u8, String)> {
    let mut characters = line.chars().peekable();
    let mut indent = 0;
    while characters
        .peek()
        .is_some_and(|character| character.is_whitespace())
        && indent < 4
    {
        characters.next();
        indent += 1;
    }
    if indent > 3 {
        return None;
    }
    let mut depth = 0_u8;
    while characters.peek() == Some(&'#') && depth < 7 {
        characters.next();
        depth += 1;
    }
    if !(1..=6).contains(&depth) {
        return None;
    }
    match characters.peek() {
        None => return Some((depth, String::new())),
        Some(' ' | '\t') => {}
        Some(_) => return None,
    }
    while matches!(characters.peek(), Some(' ' | '\t')) {
        characters.next();
    }
    let mut title = characters.collect::<String>();
    title = title.trim().to_owned();
    let without_hashes = title.trim_end_matches('#');
    if without_hashes.len() < title.len()
        && without_hashes
            .chars()
            .last()
            .is_some_and(|character| character == ' ' || character == '\t')
    {
        title = without_hashes.trim_end().to_owned();
    }
    Some((depth, title))
}

fn setext_depth(line: &str) -> Option<u8> {
    let trimmed_start = line.trim_start_matches(char::is_whitespace);
    if line.chars().count() - trimmed_start.chars().count() > 3 {
        return None;
    }
    let marker = trimmed_start.chars().next()?;
    if marker != '=' && marker != '-' {
        return None;
    }
    let rest = trimmed_start.trim_end_matches([' ', '\t']);
    if rest.chars().all(|character| character == marker) {
        Some(if marker == '=' { 1 } else { 2 })
    } else {
        None
    }
}

fn sections_of(text: &str) -> Vec<Section> {
    let lines = lines_of(text);
    let body_start = frontmatter_end(&lines);
    let events = heading_events(&lines, body_start);
    if events.is_empty() {
        return (!text[body_start..].trim().is_empty())
            .then_some(Section {
                start: body_start,
                end: text.len(),
                heading_path: Vec::new(),
                depth: 0,
                position: "root".to_owned(),
                parent_position: None,
            })
            .into_iter()
            .collect();
    }

    let mut sections = Vec::new();
    if !text[body_start..events[0].start].trim().is_empty() {
        sections.push(Section {
            start: body_start,
            end: events[0].start,
            heading_path: Vec::new(),
            depth: 0,
            position: "root".to_owned(),
            parent_position: None,
        });
    }
    let mut stack = Vec::<(u8, String, String)>::new();
    let mut child_counts = HashMap::<String, usize>::new();
    for (index, event) in events.iter().enumerate() {
        while stack.last().is_some_and(|entry| entry.0 >= event.depth) {
            stack.pop();
        }
        let parent_position = stack.last().map(|entry| entry.2.clone());
        let key = format!(
            "{}/{}",
            parent_position.as_deref().unwrap_or("root"),
            event.depth
        );
        let ordinal = child_counts.entry(key).or_insert(0);
        *ordinal += 1;
        let position = parent_position.as_ref().map_or_else(
            || format!("{}-{}", event.depth, ordinal),
            |parent| format!("{parent}.{}-{}", event.depth, ordinal),
        );
        let mut heading_path = stack
            .iter()
            .map(|entry| entry.1.clone())
            .collect::<Vec<_>>();
        heading_path.push(event.title.clone());
        sections.push(Section {
            start: event.start,
            end: events.get(index + 1).map_or(text.len(), |next| next.start),
            heading_path,
            depth: event.depth,
            position: position.clone(),
            parent_position,
        });
        stack.push((event.depth, event.title.clone(), position));
    }
    sections
}

fn byte_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                starts.push(index + 1);
                index += 1;
            }
            b'\r' => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
                starts.push(index + 1);
                index += 1;
            }
            _ => index += 1,
        }
    }
    starts
}

fn line_at(starts: &[usize], byte: usize) -> u32 {
    let index = starts.partition_point(|start| *start <= byte);
    u32::try_from(index.max(1)).unwrap_or(u32::MAX)
}

fn paragraph_boundaries(bytes: &[u8]) -> Vec<usize> {
    let mut output = Vec::new();
    for index in 0..bytes.len().saturating_sub(1) {
        if bytes[index] == b'\n' && bytes[index + 1] == b'\n' {
            output.push(index + 1);
        } else if index + 3 < bytes.len() && bytes[index..index + 4] == *b"\r\n\r\n" {
            output.push(index + 2);
        }
    }
    output
}

fn split_section(
    bytes: &[u8],
    max_tokens: usize,
    overlap_tokens: usize,
) -> RetrievalResult<Vec<Piece>> {
    let tokens = ascii_whitespace_tokens(bytes);
    if tokens.len() <= max_tokens && bytes.len() <= MAX_CHUNK_BYTES {
        return Ok((!bytes.is_empty())
            .then_some(Piece {
                start: 0,
                end: bytes.len(),
                tokens: tokens.len(),
            })
            .into_iter()
            .collect());
    }
    let paragraphs = paragraph_boundaries(bytes);
    let mut pieces = Vec::new();
    let mut token_start = 0;
    while token_start < tokens.len() {
        let hard_end_token = tokens.len().min(token_start + max_tokens);
        let mut end_byte = if hard_end_token == tokens.len() {
            bytes.len()
        } else {
            tokens[hard_end_token - 1].end
        };
        if hard_end_token < tokens.len() {
            let minimum = tokens
                .get(token_start + max_tokens / 2)
                .map_or(tokens[token_start].end, |token| token.end);
            if let Some(boundary) = paragraphs
                .iter()
                .copied()
                .rfind(|candidate| *candidate >= minimum && *candidate <= end_byte)
            {
                end_byte = boundary;
            }
        }
        let mut included = token_start;
        while included < tokens.len() && tokens[included].start < end_byte {
            included += 1;
        }
        if included == token_start {
            included += 1;
        }
        let start_byte = if token_start == 0 {
            0
        } else {
            tokens[token_start].start
        };
        let mut bounded_start = start_byte;
        while end_byte - bounded_start > MAX_CHUNK_BYTES {
            let mut bounded_end = bounded_start + MAX_CHUNK_BYTES;
            while bounded_end > bounded_start && bytes[bounded_end] & 0xc0 == 0x80 {
                bounded_end -= 1;
            }
            if bounded_end == bounded_start {
                return Err(RetrievalError::InvalidEnvelope(
                    "no UTF-8 chunk boundary exists within the byte limit".to_owned(),
                ));
            }
            pieces.push(Piece {
                start: bounded_start,
                end: bounded_end,
                tokens: ascii_whitespace_tokens(&bytes[bounded_start..bounded_end]).len(),
            });
            bounded_start = bounded_end;
        }
        pieces.push(Piece {
            start: bounded_start,
            end: end_byte,
            tokens: ascii_whitespace_tokens(&bytes[bounded_start..end_byte]).len(),
        });
        if included >= tokens.len() {
            break;
        }
        token_start = (included - overlap_tokens).max(token_start + 1);
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, DiscoverabilityDecision,
        RetrievalChunkMetadata, RETRIEVAL_CONTRACT,
    };

    use super::*;

    fn source(path: &str, text: &str) -> RetrievalSource {
        RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: "019b2d14-4230-7db7-87d4-7d81cfaeca01".to_owned(),
            source_path: path.to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata::default(),
        }
    }

    #[test]
    fn tokenizer_uses_only_the_six_frozen_ascii_separators() {
        let tokens = ascii_whitespace_tokens("one\ttwo\u{a0}three\r\nfour".as_bytes());
        assert_eq!(tokens.len(), 3);
        assert_eq!(
            tokens
                .iter()
                .map(|token| &"one\ttwo\u{a0}three\r\nfour".as_bytes()[token.start..token.end])
                .collect::<Vec<_>>(),
            vec![
                b"one".as_slice(),
                "two\u{a0}three".as_bytes(),
                b"four".as_slice()
            ]
        );
    }

    #[test]
    fn heading_hierarchy_ignores_fenced_hashes_and_round_trips_utf8_crlf() {
        let text = "---\r\ntitle: x\r\n---\r\n# Hé😀\r\nBody\r\n```md\r\n## not heading\r\n```\r\n## Child\r\nTail";
        let source = source("notes/a.md", text);
        let chunks = chunk_source(&source, ChunkingOptions::default()).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path, ["Hé😀"]);
        assert_eq!(chunks[1].heading_path, ["Hé😀", "Child"]);
        assert_eq!(
            chunks[1].parent_chunk_id.as_deref(),
            Some(chunks[0].chunk_id.as_str())
        );
        for chunk in &chunks {
            chunk.validate_against(&source).unwrap();
        }
    }

    #[test]
    fn setext_marker_without_a_title_remains_eligible_for_the_next_marker() {
        let text = "# H\n---\n---\nBody";
        let chunks = chunk_source(&source("setext.md", text), ChunkingOptions::default()).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks[0].chunk_id,
            "sha256:1c95f59e8ceabea706008eb667c126af043cbb7780dee6f93c700f5d4e2ff64f"
        );
        assert_eq!(chunks[0].heading_path, ["H"]);
        assert_eq!(chunks[0].text, "# H\n");
        assert_eq!(
            chunks[1].chunk_id,
            "sha256:74a96cc3bb73c080923aaa29231297c49a7ba5cb5d644c58a49a5ea17742eac2"
        );
        assert_eq!(chunks[1].heading_path, ["H", "---"]);
        assert_eq!(chunks[1].text, "---\n---\nBody");
        assert_eq!(
            chunks[1].parent_chunk_id.as_deref(),
            Some(chunks[0].chunk_id.as_str())
        );
    }

    #[test]
    fn path_rename_does_not_change_chunk_identity() {
        let text = "# Stable\nSame content";
        let before = chunk_source(&source("before.md", text), ChunkingOptions::default()).unwrap();
        let after = chunk_source(&source("after.md", text), ChunkingOptions::default()).unwrap();
        assert_eq!(before[0].chunk_id, after[0].chunk_id);
        assert_ne!(before[0].source_path, after[0].source_path);
    }

    #[test]
    fn oversized_sections_split_deterministically_with_configured_overlap() {
        let text = format!(
            "# Long\n{}",
            (0..40)
                .map(|index| format!("t{index}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let source = source("long.md", &text);
        let chunks = chunk_source(
            &source,
            ChunkingOptions {
                max_tokens: 16,
                overlap_tokens: 2,
            },
        )
        .unwrap();
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|chunk| chunk.token_count <= 16));
        assert!(chunks[0].end_byte > chunks[1].start_byte);
    }

    #[test]
    fn oversized_single_token_splits_only_on_utf8_boundaries_and_never_exceeds_byte_cap() {
        let text = "😀".repeat(5_000);
        let source = source("unicode.md", &text);
        let chunks = chunk_source(&source, ChunkingOptions::default()).unwrap();
        assert!(chunks.len() >= 2);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.text.len() <= MAX_CHUNK_BYTES));
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<String>(),
            text
        );
    }

    #[test]
    fn denied_sources_fail_closed_before_chunking() {
        let mut source = source("hidden.md", "# Hidden\nSecret");
        source.discoverability = DiscoverabilityDecision::Indeterminate;
        assert!(matches!(
            chunk_source(&source, ChunkingOptions::default()),
            Err(RetrievalError::NotDiscoverable(_))
        ));
    }
}
