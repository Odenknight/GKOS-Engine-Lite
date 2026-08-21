use crate::contract::{RetrievalChunk, RetrievalFilters};
use crate::fusion::code_unit_compare;
use crate::{RetrievalError, RetrievalResult};

const MAX_FILTER_ITEMS: usize = 256;
const MAX_FILTER_STRING_BYTES: usize = 1_024;

fn invalid_filter(coordinate: &str) -> RetrievalError {
    RetrievalError::InvalidConfig(format!("RETRIEVAL_FILTER_INVALID:{coordinate}"))
}

fn validate_bounded_filter_string(value: &str, coordinate: &str) -> RetrievalResult<()> {
    if value.is_empty() || value.len() > MAX_FILTER_STRING_BYTES || value.contains('\0') {
        return Err(invalid_filter(coordinate));
    }
    Ok(())
}

fn validate_bounded_filter_strings(
    values: Option<&[String]>,
    coordinate: &str,
) -> RetrievalResult<()> {
    let Some(values) = values else {
        return Ok(());
    };
    if values.len() > MAX_FILTER_ITEMS {
        return Err(invalid_filter(coordinate));
    }
    for value in values {
        validate_bounded_filter_string(value, coordinate)?;
    }
    Ok(())
}

fn valid_portable_glob(value: &str) -> bool {
    if value.contains('\\')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || value.chars().any(|character| {
            matches!(character as u32, 0x00..=0x1f)
                || matches!(character, '<' | '>' | ':' | '"' | '|')
        })
    {
        return false;
    }
    value
        .split('/')
        .all(|segment| !matches!(segment, "." | ".."))
}

/// Validate every typed filter bound before store reads, policy evaluation, or
/// provider work. Deserialization seals field names and scalar types; this
/// preflight seals resource limits and value grammars for programmatic callers.
pub(crate) fn validate_retrieval_filters(filters: &RetrievalFilters) -> RetrievalResult<()> {
    validate_bounded_filter_string_option(filters.vault.as_deref(), "vault")?;
    for (values, coordinate) in [
        (filters.path_include.as_deref(), "path_include"),
        (filters.path_exclude.as_deref(), "path_exclude"),
        (filters.tags_any.as_deref(), "tags_any"),
        (filters.tags_all.as_deref(), "tags_all"),
        (filters.topics.as_deref(), "topics"),
        (filters.categories.as_deref(), "categories"),
        (filters.gkx_types.as_deref(), "gkx_types"),
        (filters.epistemic_states.as_deref(), "epistemic_states"),
        (filters.governance_states.as_deref(), "governance_states"),
        (filters.review_states.as_deref(), "review_states"),
        (filters.source_digests.as_deref(), "source_digests"),
    ] {
        validate_bounded_filter_strings(values, coordinate)?;
    }
    for (values, coordinate) in [
        (filters.path_include.as_deref(), "path_include"),
        (filters.path_exclude.as_deref(), "path_exclude"),
    ] {
        if values.is_some_and(|values| values.iter().any(|glob| !valid_portable_glob(glob))) {
            return Err(invalid_filter(coordinate));
        }
    }
    if filters.source_digests.as_deref().is_some_and(|digests| {
        digests.iter().any(|digest| {
            digest.len() != 71
                || !digest.starts_with("sha256:")
                || !digest[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }) {
        return Err(invalid_filter("source_digests"));
    }
    for (value, coordinate) in [
        (filters.authored_from.as_deref(), "authored_from"),
        (filters.authored_to.as_deref(), "authored_to"),
    ] {
        if value.is_some_and(|timestamp| !is_valid_retrieval_timestamp(timestamp)) {
            return Err(invalid_filter(coordinate));
        }
    }
    if filters
        .minimum_quality
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err(invalid_filter("minimum_quality"));
    }
    Ok(())
}

fn validate_bounded_filter_string_option(
    value: Option<&str>,
    coordinate: &str,
) -> RetrievalResult<()> {
    value.map_or(Ok(()), |value| {
        validate_bounded_filter_string(value, coordinate)
    })
}

/// Evaluate the typed retrieval filter contract without accepting SQL or code.
pub fn matches_retrieval_filters(
    chunk: &RetrievalChunk,
    filters: &RetrievalFilters,
    vault_id: &str,
) -> RetrievalResult<bool> {
    let metadata = &chunk.metadata;
    if filters
        .vault
        .as_deref()
        .is_some_and(|vault| vault != vault_id)
    {
        return Ok(false);
    }
    if filters
        .path_include
        .as_ref()
        .is_some_and(|globs| !globs.is_empty() && !any_glob(&chunk.source_path, globs))
    {
        return Ok(false);
    }
    if filters
        .path_exclude
        .as_ref()
        .is_some_and(|globs| !globs.is_empty() && any_glob(&chunk.source_path, globs))
    {
        return Ok(false);
    }
    if filters.include_archives != Some(true)
        && (metadata.archived == Some(true) || is_archive_path(&chunk.source_path))
    {
        return Ok(false);
    }
    if !intersects(metadata.tags.as_deref(), filters.tags_any.as_deref())
        || !contains_all(metadata.tags.as_deref(), filters.tags_all.as_deref())
        || !same(metadata.topic.as_deref(), filters.topics.as_deref())
        || !same(metadata.category.as_deref(), filters.categories.as_deref())
        || !same(metadata.gkx_type.as_deref(), filters.gkx_types.as_deref())
        || !same(
            metadata.epistemic_state.as_deref(),
            filters.epistemic_states.as_deref(),
        )
        || !same(
            metadata.governance_state.as_deref(),
            filters.governance_states.as_deref(),
        )
        || !same(
            metadata.review_state.as_deref(),
            filters.review_states.as_deref(),
        )
        || !same(
            metadata.author_agent_id.as_deref(),
            filters.author_agent_ids.as_deref(),
        )
        || !intersects(
            metadata.moc_relationships.as_deref(),
            filters.moc_relationships.as_deref(),
        )
    {
        return Ok(false);
    }
    if filters
        .source_digests
        .as_ref()
        .is_some_and(|digests| !digests.is_empty() && !digests.contains(&chunk.source_digest))
    {
        return Ok(false);
    }
    if filters
        .authoritative
        .is_some_and(|expected| metadata.authoritative != Some(expected))
    {
        return Ok(false);
    }
    if let Some(minimum) = filters.minimum_quality {
        if !minimum.is_finite() || !(0.0..=1.0).contains(&minimum) {
            return Err(RetrievalError::InvalidConfig(
                "minimum_quality must be finite and within [0, 1]".to_owned(),
            ));
        }
        if !metadata
            .quality
            .is_some_and(|quality| quality.is_finite() && quality >= minimum)
        {
            return Ok(false);
        }
    }
    if let Some(ceiling) = filters.sensitivity_ceiling {
        // Missing or invalid sensitivity is treated as the fail-closed maximum.
        let effective = metadata.sensitivity.map_or(u8::MAX, |value| value.rank());
        if effective > ceiling.rank() {
            return Ok(false);
        }
    }

    let from = parse_rfc3339_millis(filters.authored_from.as_deref()).transpose()?;
    let to = parse_rfc3339_millis(filters.authored_to.as_deref()).transpose()?;
    if from.is_some() || to.is_some() {
        let authored = parse_rfc3339_millis(metadata.authored_at.as_deref()).transpose()?;
        if authored.is_none()
            || from.is_some_and(|value| authored.is_some_and(|authored| authored < value))
            || to.is_some_and(|value| authored.is_some_and(|authored| authored >= value))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Return only contract field names, never caller-supplied filter values.
pub fn applied_filter_names(filters: &RetrievalFilters) -> Vec<String> {
    let mut names = Vec::new();
    macro_rules! optional {
        ($field:ident) => {
            if filters.$field.is_some() {
                names.push(stringify!($field).to_owned());
            }
        };
    }
    macro_rules! list {
        ($field:ident) => {
            if filters
                .$field
                .as_ref()
                .is_some_and(|values| !values.is_empty())
            {
                names.push(stringify!($field).to_owned());
            }
        };
    }
    optional!(vault);
    list!(path_include);
    list!(path_exclude);
    list!(tags_any);
    list!(tags_all);
    list!(topics);
    list!(categories);
    optional!(authored_from);
    optional!(authored_to);
    optional!(sensitivity_ceiling);
    list!(gkx_types);
    list!(epistemic_states);
    list!(governance_states);
    list!(review_states);
    optional!(authoritative);
    list!(moc_relationships);
    list!(source_digests);
    list!(author_agent_ids);
    optional!(minimum_quality);
    optional!(include_archives);
    names.sort_by(|left, right| code_unit_compare(left, right));
    names
}

fn same(value: Option<&str>, candidates: Option<&[String]>) -> bool {
    match candidates {
        None => true,
        Some(candidates) => {
            candidates.is_empty()
                || value.is_some_and(|value| candidates.iter().any(|candidate| candidate == value))
        }
    }
}

fn intersects(values: Option<&[String]>, candidates: Option<&[String]>) -> bool {
    match candidates {
        None => true,
        Some(candidates) => {
            candidates.is_empty()
                || candidates.iter().any(|candidate| {
                    values.is_some_and(|values| values.iter().any(|value| value == candidate))
                })
        }
    }
}

fn contains_all(values: Option<&[String]>, candidates: Option<&[String]>) -> bool {
    match candidates {
        None => true,
        Some(candidates) => {
            candidates.is_empty()
                || candidates.iter().all(|candidate| {
                    values.is_some_and(|values| values.iter().any(|value| value == candidate))
                })
        }
    }
}

fn is_archive_path(path: &str) -> bool {
    path.split('/').any(|segment| {
        segment.eq_ignore_ascii_case("archive") || segment.eq_ignore_ascii_case("archives")
    })
}

fn any_glob(path: &str, globs: &[String]) -> bool {
    globs
        .iter()
        .any(|glob| matches_path_glob(path, &glob.replace('\\', "/")))
}

/// Apply the frozen memoized Unicode-scalar path-glob grammar. This helper is
/// value-only; policy eligibility still belongs to `matches_retrieval_filters`.
pub fn matches_path_glob(path: &str, glob: &str) -> bool {
    fn recurse(
        path: &[char],
        glob: &[char],
        path_index: usize,
        glob_index: usize,
        memo: &mut std::collections::BTreeMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(answer) = memo.get(&(path_index, glob_index)) {
            return *answer;
        }
        let answer = match glob.get(glob_index) {
            None => path_index == path.len(),
            Some('*') if glob.get(glob_index + 1) == Some(&'*') => {
                recurse(path, glob, path_index, glob_index + 2, memo)
                    || (path_index < path.len()
                        && recurse(path, glob, path_index + 1, glob_index, memo))
            }
            Some('*') => {
                recurse(path, glob, path_index, glob_index + 1, memo)
                    || (path
                        .get(path_index)
                        .is_some_and(|character| *character != '/')
                        && recurse(path, glob, path_index + 1, glob_index, memo))
            }
            Some('?') => {
                path.get(path_index)
                    .is_some_and(|character| *character != '/')
                    && recurse(path, glob, path_index + 1, glob_index + 1, memo)
            }
            Some(expected) => {
                path.get(path_index) == Some(expected)
                    && recurse(path, glob, path_index + 1, glob_index + 1, memo)
            }
        };
        memo.insert((path_index, glob_index), answer);
        answer
    }
    let path = path.chars().collect::<Vec<_>>();
    let glob = glob.chars().collect::<Vec<_>>();
    recurse(&path, &glob, 0, 0, &mut std::collections::BTreeMap::new())
}

fn parse_rfc3339_millis(value: Option<&str>) -> Option<RetrievalResult<i64>> {
    value.map(|value| {
        let bytes = value.as_bytes();
        if bytes.len() < 17
            || bytes.get(4) != Some(&b'-')
            || bytes.get(7) != Some(&b'-')
            || bytes.get(10) != Some(&b'T')
            || bytes.get(13) != Some(&b':')
        {
            return Err(RetrievalError::InvalidConfig(
                "authored date filters must use RFC3339 timestamps".to_owned(),
            ));
        }
        let number = |start: usize, end: usize| -> Option<i64> {
            std::str::from_utf8(bytes.get(start..end)?)
                .ok()?
                .parse()
                .ok()
        };
        let year = number(0, 4).unwrap_or(-1);
        let month = number(5, 7).unwrap_or(-1);
        let day = number(8, 10).unwrap_or(-1);
        let hour = number(11, 13).unwrap_or(-1);
        let minute = number(14, 16).unwrap_or(-1);
        if year < 0
            || !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || hour > 24
            || minute > 59
        {
            return Err(RetrievalError::InvalidConfig(
                "authored date filters must use valid RFC3339 timestamps".to_owned(),
            ));
        }
        let mut cursor = 16;
        let mut second = 0;
        let mut has_seconds = false;
        if bytes.get(cursor) == Some(&b':') {
            second = number(cursor + 1, cursor + 3).unwrap_or(99);
            if second > 59 {
                return Err(RetrievalError::InvalidConfig(
                    "authored date filters must use valid RFC3339 timestamps".to_owned(),
                ));
            }
            cursor += 3;
            has_seconds = true;
        }
        let mut milliseconds = 0;
        if bytes.get(cursor) == Some(&b'.') {
            if !has_seconds {
                return Err(RetrievalError::InvalidConfig(
                    "fractional GKX timestamps require seconds".to_owned(),
                ));
            }
            cursor += 1;
            let start = cursor;
            while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                cursor += 1;
            }
            if cursor == start {
                return Err(RetrievalError::InvalidConfig(
                    "RFC3339 fractional seconds must contain digits".to_owned(),
                ));
            }
            for offset in 0..3 {
                milliseconds *= 10;
                milliseconds += bytes
                    .get(start + offset)
                    .filter(|byte| byte.is_ascii_digit())
                    .map_or(0, |byte| i64::from(*byte - b'0'));
            }
        }
        let offset_seconds = if bytes.get(cursor) == Some(&b'Z') && cursor + 1 == bytes.len() {
            0
        } else if matches!(bytes.get(cursor), Some(b'+') | Some(b'-'))
            && cursor + 6 == bytes.len()
            && bytes.get(cursor + 3) == Some(&b':')
        {
            let sign = if bytes[cursor] == b'+' { 1 } else { -1 };
            let offset_hour = number(cursor + 1, cursor + 3).unwrap_or(99);
            let offset_minute = number(cursor + 4, cursor + 6).unwrap_or(99);
            if offset_hour > 23 || offset_minute > 59 {
                return Err(RetrievalError::InvalidConfig(
                    "RFC3339 offset is invalid".to_owned(),
                ));
            }
            sign * (offset_hour * 3600 + offset_minute * 60)
        } else {
            return Err(RetrievalError::InvalidConfig(
                "authored date filters must include an RFC3339 offset".to_owned(),
            ));
        };
        if hour == 24 && (minute != 0 || second != 0 || milliseconds != 0) {
            return Err(RetrievalError::InvalidConfig(
                "24:00 is the only accepted end-of-day timestamp".to_owned(),
            ));
        }
        let adjusted_year = year - i64::from(month <= 2);
        let era = if adjusted_year >= 0 {
            adjusted_year
        } else {
            adjusted_year - 399
        } / 400;
        let year_of_era = adjusted_year - era * 400;
        let shifted_month = month + if month > 2 { -3 } else { 9 };
        let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        let days = era * 146_097 + day_of_era - 719_468;
        Ok(
            ((days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds) * 1000)
                + milliseconds,
        )
    })
}

/// Tests the exact timestamp grammar used by typed retrieval filters.
pub fn is_valid_retrieval_timestamp(value: &str) -> bool {
    matches!(parse_rfc3339_millis(Some(value)), Some(Ok(_)))
}

#[cfg(test)]
mod tests {
    use crate::chunker::{chunk_source, ChunkingOptions};
    use crate::contract::{
        CanonicalLineageEnvelope, CanonicalTemporalEnvelope, DiscoverabilityDecision,
        GkxSensitivity, RetrievalChunkMetadata, RetrievalSource, RETRIEVAL_CONTRACT,
    };
    use crate::digest::sha256;

    use super::*;

    fn chunk(path: &str) -> RetrievalChunk {
        let text = "# Policy\nAllowed";
        let source = RetrievalSource {
            contract_version: RETRIEVAL_CONTRACT.to_owned(),
            vault_id: "vault-a".to_owned(),
            source_id: "019b2d14-4230-7db7-87d4-7d81cfaeca01".to_owned(),
            source_path: path.to_owned(),
            source_digest: sha256(text.as_bytes()),
            text: text.to_owned(),
            discoverability: DiscoverabilityDecision::Allow,
            lineage: CanonicalLineageEnvelope::default(),
            temporal: CanonicalTemporalEnvelope::default(),
            metadata: RetrievalChunkMetadata {
                tags: Some(vec!["policy".to_owned(), "agents".to_owned()]),
                authored_at: Some("2026-08-20T12:00:00-04:00".to_owned()),
                sensitivity: Some(GkxSensitivity::Internal),
                quality: Some(0.9),
                ..RetrievalChunkMetadata::default()
            },
        };
        chunk_source(&source, ChunkingOptions::default())
            .unwrap()
            .remove(0)
    }

    #[test]
    fn typed_filters_compose_and_dates_are_half_open() {
        let chunk = chunk("policy/agent.md");
        let filters = RetrievalFilters {
            path_include: Some(vec!["policy/**".to_owned()]),
            tags_all: Some(vec!["policy".to_owned(), "agents".to_owned()]),
            authored_from: Some("2026-08-20T16:00:00Z".to_owned()),
            authored_to: Some("2026-08-21T00:00:00Z".to_owned()),
            sensitivity_ceiling: Some(GkxSensitivity::Internal),
            minimum_quality: Some(0.8),
            ..RetrievalFilters::default()
        };
        assert!(matches_retrieval_filters(&chunk, &filters, "vault-a").unwrap());
        let exclusive = RetrievalFilters {
            authored_to: Some("2026-08-20T16:00:00Z".to_owned()),
            ..RetrievalFilters::default()
        };
        assert!(!matches_retrieval_filters(&chunk, &exclusive, "vault-a").unwrap());
    }

    #[test]
    fn missing_sensitivity_fails_closed_and_archive_is_excluded() {
        let mut ordinary = chunk("notes/a.md");
        ordinary.metadata.sensitivity = None;
        let filters = RetrievalFilters {
            sensitivity_ceiling: Some(GkxSensitivity::Confidential),
            ..RetrievalFilters::default()
        };
        assert!(!matches_retrieval_filters(&ordinary, &filters, "vault-a").unwrap());
        assert!(!matches_retrieval_filters(
            &chunk("Archive/old.md"),
            &RetrievalFilters::default(),
            "vault-a"
        )
        .unwrap());
    }

    #[test]
    fn applied_names_never_return_values_and_use_utf16_order() {
        let filters = RetrievalFilters {
            vault: Some("secret-vault-name".to_owned()),
            tags_any: Some(vec!["sensitive-value".to_owned()]),
            include_archives: Some(false),
            ..RetrievalFilters::default()
        };
        assert_eq!(
            applied_filter_names(&filters),
            ["include_archives", "tags_any", "vault"]
        );
    }

    #[test]
    fn malformed_date_configuration_fails_instead_of_broadening_results() {
        let filters = RetrievalFilters {
            authored_from: Some("yesterday".to_owned()),
            ..RetrievalFilters::default()
        };
        assert!(matches_retrieval_filters(&chunk("a.md"), &filters, "vault-a").is_err());
    }

    #[test]
    fn malformed_authored_metadata_is_ignored_until_a_date_filter_is_active() {
        let mut malformed = chunk("policy/a.md");
        malformed.metadata.authored_at = Some("not-a-timestamp".to_owned());
        let unrelated = RetrievalFilters {
            path_include: Some(vec!["policy/**".to_owned()]),
            ..RetrievalFilters::default()
        };
        assert!(matches_retrieval_filters(&malformed, &unrelated, "vault-a").unwrap());
        let date_filter = RetrievalFilters {
            authored_from: Some("2026-08-20T00:00Z".to_owned()),
            ..RetrievalFilters::default()
        };
        assert!(matches_retrieval_filters(&malformed, &date_filter, "vault-a").is_err());
    }

    #[test]
    fn timestamp_grammar_matches_canonical_gkx_forms_and_offsets() {
        let short = parse_rfc3339_millis(Some("2026-08-20T12:00Z"))
            .unwrap()
            .unwrap();
        let offset = parse_rfc3339_millis(Some("2026-08-20T08:00-04:00"))
            .unwrap()
            .unwrap();
        assert_eq!(short, offset);
        assert!(parse_rfc3339_millis(Some("2026-08-20 12:00Z"))
            .unwrap()
            .is_err());
        assert!(parse_rfc3339_millis(Some("2026-08-20T12:00.5Z"))
            .unwrap()
            .is_err());
        assert!(parse_rfc3339_millis(Some("2026-08-20T12:00"))
            .unwrap()
            .is_err());
        // Date.parse, which the canonical validator composes, normalizes this.
        assert!(parse_rfc3339_millis(Some("2026-02-29T00:00Z"))
            .unwrap()
            .is_ok());
    }

    #[test]
    fn filter_preflight_matches_full_resource_and_value_boundaries() {
        let valid = RetrievalFilters {
            vault: Some("v".repeat(1_024)),
            path_include: Some(vec!["policy/**/😀?.md".to_owned()]),
            tags_any: Some(vec!["tag".to_owned(); 256]),
            authored_from: Some("2026-08-20T12:30Z".to_owned()),
            authored_to: Some("2026-08-20T13:30:00-04:00".to_owned()),
            source_digests: Some(vec![format!("sha256:{}", "a".repeat(64))]),
            minimum_quality: Some(1.0),
            ..RetrievalFilters::default()
        };
        validate_retrieval_filters(&valid).unwrap();

        for invalid in [
            RetrievalFilters {
                tags_any: Some(vec!["tag".to_owned(); 257]),
                ..RetrievalFilters::default()
            },
            RetrievalFilters {
                topics: Some(vec!["x".repeat(1_025)]),
                ..RetrievalFilters::default()
            },
            RetrievalFilters {
                path_include: Some(vec!["../hidden/**".to_owned()]),
                ..RetrievalFilters::default()
            },
            RetrievalFilters {
                source_digests: Some(vec![format!("sha256:{}", "A".repeat(64))]),
                ..RetrievalFilters::default()
            },
            RetrievalFilters {
                authored_from: Some("2026-13-01T00:00:00Z".to_owned()),
                ..RetrievalFilters::default()
            },
            RetrievalFilters {
                minimum_quality: Some(-f64::EPSILON),
                ..RetrievalFilters::default()
            },
        ] {
            assert!(validate_retrieval_filters(&invalid).is_err());
        }
    }
}
