//! Crate-private Phase-4 normalized-envelope verifier.
//!
//! This module is intentionally pure: it accepts already-normalized canonical
//! JSON values and independently recomputes bounded integer evaluation math.
//! It has no TOML, GKX, provider, search, SQLite, path, or output authority.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{json, Map, Value};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::contract::{is_valid_authored_uid, is_valid_retrieval_source_path};
use crate::digest::{canonical_digest, canonical_json, sha256};
use crate::provenance::normalize_retrieval_as_of;
use crate::sqlite_store::lexical_query_clauses;

const GOLDEN_VERSION: &str = "gkos-retrieval-evaluation-golden/1.0.0-draft.1";
const QUERY_METRICS_VERSION: &str = "gkos-retrieval-evaluation-query-metrics/1.0.0-draft.1";
const AGGREGATE_METRICS_VERSION: &str = "gkos-retrieval-evaluation-aggregate/1.0.0-draft.1";
const QUERY_VIEW_ORACLE_VERSION: &str =
    "gkos-retrieval-evaluation-query-view-audit-oracle/1.0.0-draft.1";
const SCENARIO_OUTCOME_VERSION: &str = "gkos-retrieval-evaluation-scenario-outcome/1.0.0-draft.1";
const SCENARIO_COMPARISON_VERSION: &str =
    "gkos-retrieval-evaluation-scenario-comparison/1.0.0-draft.1";
const TUNING_AXES_VERSION: &str = "gkos-retrieval-evaluation-tuning-axes/1.0.0-draft.1";
const ENVIRONMENT_VERSION: &str = "gkos-retrieval-evaluation-environment/1.0.0-draft.1";
const ENVIRONMENT_SET_VERSION: &str = "gkos-retrieval-evaluation-environment-set/1.0.0-draft.1";
const METRICS_SET_VERSION: &str = "gkos-retrieval-evaluation-metrics-set/1.0.0-draft.1";
const QUERY_METRICS_SET_VERSION: &str = "gkos-retrieval-evaluation-query-metrics-set/1.0.0-draft.1";
const BASE_CONFIGURATION_VERSION: &str =
    "gkos-retrieval-evaluation-base-configuration/1.0.0-draft.1";
const TUNING_GRID_VERSION: &str = "gkos-retrieval-evaluation-tuning-grid/1.0.0-draft.1";
const BASELINE_VERSION: &str = "gkos-retrieval-evaluation-baseline/1.0.0-draft.1";
const COMPARISON_VERSION: &str = "gkos-retrieval-evaluation-comparison/1.0.0-draft.1";
const OBSERVATION_VERSION: &str = "gkos-retrieval-evaluation-observation/1.0.0-draft.1";
const EVALUATION_COORDINATE_VERSION: &str =
    "gkos-retrieval-evaluation-evaluation-coordinate/1.0.0-draft.1";
const METRIC_VERSION: &str = "gkos-retrieval-evaluation-metrics/1.0.0-draft.1";
const EVALUATION_VERSION: &str = "gkos-retrieval-evaluation/1.0.0-draft.1";
const NDCG_TABLE_DIGEST: &str =
    "sha256:4639dcbd6902da38cb5fa44918736f10d69714453bf57da05ecc5783ba0684a9";
const METRIC_SCALE: u128 = 1_000_000;
const DISCOUNT_SCALE: u128 = 1_000_000_000_000;
const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

const NDCG_DISCOUNTS: [u64; 100] = [
    1_000_000_000_000,
    630_929_753_571,
    500_000_000_000,
    430_676_558_073,
    386_852_807_235,
    356_207_187_108,
    333_333_333_333,
    315_464_876_786,
    301_029_995_664,
    289_064_826_318,
    278_942_945_651,
    270_238_154_427,
    262_649_535_037,
    255_958_024_810,
    250_000_000_000,
    244_650_542_118,
    239_812_466_568,
    235_408_913_367,
    231_378_213_160,
    227_670_248_697,
    224_243_824_218,
    221_064_729_458,
    218_104_291_986,
    215_338_279_037,
    212_746_053_553,
    210_309_917_857,
    208_014_597_677,
    205_846_832_460,
    203_795_047_091,
    201_849_086_582,
    200_000_000_000,
    198_239_863_171,
    196_561_632_233,
    194_959_021_894,
    193_426_403_617,
    191_958_720_007,
    190_551_412_427,
    189_200_359_517,
    187_901_824_709,
    186_652_411_239,
    185_449_023_415,
    184_288_833_149,
    183_169_250_914,
    182_087_900_470,
    181_042_596_780,
    180_031_326_657,
    179_052_231_751,
    178_103_593_554,
    177_183_820_136,
    176_291_434_389,
    175_425_063_582,
    174_583_430_048,
    173_765_342_871,
    172_969_690_445,
    172_195_433_794,
    171_441_600_574,
    170_707_279_664,
    169_991_616_287,
    169_293_807_599,
    168_613_098_690,
    167_948_778_957,
    167_300_178_810,
    166_666_666_667,
    166_047_646_216,
    165_442_553_919,
    164_850_856_722,
    164_272_049_962,
    163_705_655_445,
    163_151_219_684,
    162_608_312_272,
    162_076_524_393,
    161_555_467_443,
    161_044_771_756,
    160_544_085_434,
    160_053_073_255,
    159_571_415_670,
    159_098_807_869,
    158_634_958_916,
    158_179_590_940,
    157_732_438_393,
    157_293_247_350,
    156_861_774_859,
    156_437_788_342,
    156_021_065_022,
    155_611_391_402,
    155_208_562_770,
    154_812_382_736,
    154_422_662_801,
    154_039_221_954,
    153_661_886_290,
    153_290_488_653,
    152_924_868_303,
    152_564_870_601,
    152_210_346_713,
    151_861_153_331,
    151_517_152_410,
    151_178_210_922,
    150_844_200_623,
    150_514_997_832,
    150_190_483_224,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EvalError(&'static str);

type EvalResult<T> = Result<T, EvalError>;

impl std::fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

fn code<T>(name: &'static str) -> EvalResult<T> {
    Err(EvalError(name))
}

fn object<'a>(value: &'a Value, error: &'static str) -> EvalResult<&'a Map<String, Value>> {
    value.as_object().ok_or(EvalError(error))
}

fn array<'a>(value: &'a Value, error: &'static str) -> EvalResult<&'a Vec<Value>> {
    value.as_array().ok_or(EvalError(error))
}

fn string<'a>(value: &'a Value, error: &'static str) -> EvalResult<&'a str> {
    value.as_str().ok_or(EvalError(error))
}

fn count(value: &Value, maximum: u64, error: &'static str) -> EvalResult<u64> {
    value
        .as_u64()
        .filter(|number| *number <= maximum)
        .ok_or(EvalError(error))
}

fn field<'a>(
    record: &'a Map<String, Value>,
    key: &str,
    error: &'static str,
) -> EvalResult<&'a Value> {
    record.get(key).ok_or(EvalError(error))
}

fn exact_keys(
    record: &Map<String, Value>,
    expected: &[&str],
    error: &'static str,
) -> EvalResult<()> {
    if record.len() != expected.len() || expected.iter().any(|key| !record.contains_key(*key)) {
        return code(error);
    }
    Ok(())
}

fn is_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn digest_without(value: &Value, digest_key: &str) -> EvalResult<String> {
    let mut material = object(value, "GKX_EVAL_CANONICAL_DATA_INVALID")?.clone();
    material.remove(digest_key);
    canonical_digest(&Value::Object(material))
        .map_err(|_| EvalError("GKX_EVAL_CANONICAL_DATA_INVALID"))
}

fn verify_digest(value: &Value, digest_key: &str, error: &'static str) -> EvalResult<()> {
    let record = object(value, error)?;
    let actual = string(field(record, digest_key, error)?, error)?;
    if !is_digest(actual) || digest_without(value, digest_key)? != actual {
        return code(error);
    }
    Ok(())
}

fn utf16_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn sorted_unique_strings<'a>(
    value: &'a Value,
    maximum: usize,
    error: &'static str,
) -> EvalResult<Vec<&'a str>> {
    let items = array(value, error)?;
    if items.len() > maximum {
        return code(error);
    }
    let strings = items
        .iter()
        .map(|item| string(item, error))
        .collect::<EvalResult<Vec<_>>>()?;
    if strings
        .windows(2)
        .any(|pair| utf16_cmp(pair[0], pair[1]) != Ordering::Less)
    {
        return code(error);
    }
    Ok(strings)
}

fn bounded_id(value: &str, maximum_utf8: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_utf8
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_ecmascript_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}' | '\u{000b}' | '\u{000c}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn effective_query_text(authored: &str) -> &str {
    authored.trim_matches(is_ecmascript_whitespace)
}

fn valid_source_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 1024
        || path.contains('\u{7f}')
        || !is_valid_retrieval_source_path(path)
    {
        return false;
    }
    for component in path.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes().get(3), Some(b'1'..=b'9'))
            && stem.len() == 4);
        if reserved {
            return false;
        }
    }
    canonical_json(&path).is_ok()
}

fn round_ratio(numerator: u128, denominator: u128, scale: u128) -> EvalResult<u64> {
    if denominator == 0 {
        return code("GKX_EVAL_RATIO_INVALID");
    }
    let scaled = numerator
        .checked_mul(scale)
        .ok_or(EvalError("GKX_EVAL_RATIO_INVALID"))?;
    u64::try_from((scaled + denominator / 2) / denominator)
        .map_err(|_| EvalError("GKX_EVAL_RATIO_INVALID"))
}

fn round_integer_ratio(numerator: u128, denominator: u128) -> EvalResult<u64> {
    if denominator == 0 {
        return code("GKX_EVAL_RATIO_INVALID");
    }
    u64::try_from((numerator + denominator / 2) / denominator)
        .map_err(|_| EvalError("GKX_EVAL_RATIO_INVALID"))
}

fn ndcg_micros(relevant_ranks: &[u64], relevant_count: u64, top_k: u64) -> EvalResult<u64> {
    let dcg = relevant_ranks.iter().try_fold(0_u128, |sum, rank| {
        let index = usize::try_from(*rank - 1).map_err(|_| EvalError("GKX_EVAL_NDCG_INVALID"))?;
        let discount = *NDCG_DISCOUNTS
            .get(index)
            .ok_or(EvalError("GKX_EVAL_NDCG_INVALID"))?;
        Ok::<_, EvalError>(sum + u128::from(discount))
    })?;
    let ideal_count = usize::try_from(relevant_count.min(top_k))
        .map_err(|_| EvalError("GKX_EVAL_NDCG_INVALID"))?;
    let idcg = NDCG_DISCOUNTS[..ideal_count]
        .iter()
        .map(|value| u128::from(*value))
        .sum::<u128>();
    if idcg == 0 {
        Ok(0)
    } else {
        round_ratio(dcg, idcg, METRIC_SCALE)
    }
}

fn seal_normalized_query(value: &Value) -> EvalResult<Value> {
    let query = object(value, "GKX_EVAL_QUERY_INVALID")?;
    exact_keys(
        query,
        &[
            "id",
            "text",
            "vault_fixture",
            "expected_files",
            "expected_source_ids",
            "expected_lineage_ids",
            "forbidden_source_ids",
            "forbidden_lineage_ids",
            "expected_top_k",
            "expected_confidence",
            "as_of",
            "query_digest",
        ],
        "GKX_EVAL_QUERY_FIELDS_INVALID",
    )?;
    let id = string(
        field(query, "id", "GKX_EVAL_QUERY_ID_INVALID")?,
        "GKX_EVAL_QUERY_ID_INVALID",
    )?;
    let vault = string(
        field(query, "vault_fixture", "GKX_EVAL_VAULT_FIXTURE_INVALID")?,
        "GKX_EVAL_VAULT_FIXTURE_INVALID",
    )?;
    if !bounded_id(id, 128) || !bounded_id(vault, 128) {
        return code("GKX_EVAL_QUERY_ID_INVALID");
    }
    let text = string(
        field(query, "text", "GKX_EVAL_QUERY_TEXT_INVALID")?,
        "GKX_EVAL_QUERY_TEXT_INVALID",
    )?;
    if text.is_empty()
        || text.len() > 4096
        || text
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
        || effective_query_text(text).is_empty()
    {
        return code("GKX_EVAL_QUERY_TEXT_INVALID");
    }
    let lexical_clauses =
        lexical_query_clauses(text).map_err(|_| EvalError("GKX_EVAL_QUERY_LEXICAL_INVALID"))?;
    if lexical_clauses.len() > 64 {
        return code("GKX_EVAL_QUERY_LEXICAL_CLAUSE_COUNT_INVALID");
    }
    if lexical_clauses
        .iter()
        .any(|clause| clause.value.len() > 256)
    {
        return code("GKX_EVAL_QUERY_LEXICAL_CLAUSE_SIZE_INVALID");
    }
    let expected_files = sorted_unique_strings(
        field(query, "expected_files", "GKX_EVAL_EXPECTED_FILES_INVALID")?,
        256,
        "GKX_EVAL_EXPECTED_FILES_INVALID",
    )?;
    if expected_files.iter().any(|path| !valid_source_path(path)) {
        return code("GKX_EVAL_EXPECTED_FILE_INVALID");
    }
    let expected_sources = sorted_unique_strings(
        field(
            query,
            "expected_source_ids",
            "GKX_EVAL_EXPECTED_SOURCE_IDS_INVALID",
        )?,
        256,
        "GKX_EVAL_EXPECTED_SOURCE_IDS_INVALID",
    )?;
    let forbidden_sources = sorted_unique_strings(
        field(
            query,
            "forbidden_source_ids",
            "GKX_EVAL_FORBIDDEN_SOURCE_IDS_INVALID",
        )?,
        256,
        "GKX_EVAL_FORBIDDEN_SOURCE_IDS_INVALID",
    )?;
    if expected_sources
        .iter()
        .chain(&forbidden_sources)
        .any(|uid| !is_valid_authored_uid(uid))
    {
        return code("GKX_EVAL_SOURCE_ID_INVALID");
    }
    if !array(
        field(
            query,
            "expected_lineage_ids",
            "GKX_EVAL_LINEAGE_ID_UNAVAILABLE",
        )?,
        "GKX_EVAL_LINEAGE_ID_UNAVAILABLE",
    )?
    .is_empty()
        || !array(
            field(
                query,
                "forbidden_lineage_ids",
                "GKX_EVAL_LINEAGE_ID_UNAVAILABLE",
            )?,
            "GKX_EVAL_LINEAGE_ID_UNAVAILABLE",
        )?
        .is_empty()
    {
        return code("GKX_EVAL_LINEAGE_ID_UNAVAILABLE");
    }
    if expected_files.is_empty() && expected_sources.is_empty() {
        return code("GKX_EVAL_RELEVANCE_EMPTY");
    }
    if expected_sources
        .iter()
        .any(|uid| forbidden_sources.contains(uid))
    {
        return code("GKX_EVAL_RELEVANCE_FORBIDDEN_OVERLAP");
    }
    let top_k = count(
        field(query, "expected_top_k", "GKX_EVAL_TOP_K_INVALID")?,
        100,
        "GKX_EVAL_TOP_K_INVALID",
    )?;
    if top_k == 0 {
        return code("GKX_EVAL_TOP_K_INVALID");
    }
    let confidence = string(
        field(query, "expected_confidence", "GKX_EVAL_CONFIDENCE_INVALID")?,
        "GKX_EVAL_CONFIDENCE_INVALID",
    )?;
    if !matches!(confidence, "high" | "medium" | "low") {
        return code("GKX_EVAL_CONFIDENCE_INVALID");
    }
    match query.get("as_of") {
        Some(Value::Null) => {}
        Some(Value::String(as_of)) => {
            if normalize_retrieval_as_of(as_of).ok().as_deref() != Some(as_of) {
                return code("GKX_EVAL_AS_OF_INVALID");
            }
        }
        _ => return code("GKX_EVAL_AS_OF_INVALID"),
    }
    let query_digest = string(
        field(query, "query_digest", "GKX_EVAL_QUERY_DIGEST_INVALID")?,
        "GKX_EVAL_QUERY_DIGEST_INVALID",
    )?;
    if !is_digest(query_digest) || digest_without(value, "query_digest")? != query_digest {
        return code("GKX_EVAL_QUERY_DIGEST_MISMATCH");
    }
    Ok(value.clone())
}

fn seal_normalized_golden(value: &Value) -> EvalResult<Value> {
    let golden = object(value, "GKX_EVAL_GOLDEN_INVALID")?;
    exact_keys(
        golden,
        &["contract_version", "queries", "golden_digest"],
        "GKX_EVAL_GOLDEN_FIELDS_INVALID",
    )?;
    if field(
        golden,
        "contract_version",
        "GKX_EVAL_GOLDEN_COORDINATE_INVALID",
    )? != GOLDEN_VERSION
    {
        return code("GKX_EVAL_GOLDEN_COORDINATE_INVALID");
    }
    let queries = array(
        field(golden, "queries", "GKX_EVAL_GOLDEN_COORDINATE_INVALID")?,
        "GKX_EVAL_GOLDEN_COORDINATE_INVALID",
    )?;
    if queries.is_empty() || queries.len() > 256 {
        return code("GKX_EVAL_GOLDEN_COORDINATE_INVALID");
    }
    let mut prior: Option<&str> = None;
    for query in queries {
        seal_normalized_query(query)?;
        let id = string(
            field(
                object(query, "GKX_EVAL_QUERY_INVALID")?,
                "id",
                "GKX_EVAL_QUERY_ID_INVALID",
            )?,
            "GKX_EVAL_QUERY_ID_INVALID",
        )?;
        if prior.is_some_and(|before| utf16_cmp(before, id) != Ordering::Less) {
            return code("GKX_EVAL_GOLDEN_QUERY_ORDER_INVALID");
        }
        prior = Some(id);
    }
    let digest = string(
        field(golden, "golden_digest", "GKX_EVAL_GOLDEN_DIGEST_INVALID")?,
        "GKX_EVAL_GOLDEN_DIGEST_INVALID",
    )?;
    if !is_digest(digest) || digest_without(value, "golden_digest")? != digest {
        return code("GKX_EVAL_GOLDEN_DIGEST_MISMATCH");
    }
    Ok(value.clone())
}

#[derive(Clone, Debug)]
struct Observation {
    source_id: String,
    source_path: String,
    source_digest: String,
    bytes: Vec<u8>,
}

fn decode_base64(value: &str) -> EvalResult<Vec<u8>> {
    if value.len() % 4 != 0 {
        return code("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID");
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let bytes = value.as_bytes();
    for (block_index, block) in bytes.chunks_exact(4).enumerate() {
        let last = block_index + 1 == bytes.len() / 4;
        let decode = |byte: u8| -> Option<u8> {
            match byte {
                b'A'..=b'Z' => Some(byte - b'A'),
                b'a'..=b'z' => Some(byte - b'a' + 26),
                b'0'..=b'9' => Some(byte - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        };
        let a = decode(block[0]).ok_or(EvalError("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID"))?;
        let b = decode(block[1]).ok_or(EvalError("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID"))?;
        if block[2] == b'=' {
            if !last || block[3] != b'=' || b & 0x0f != 0 {
                return code("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID");
            }
            output.push((a << 2) | (b >> 4));
            continue;
        }
        let c = decode(block[2]).ok_or(EvalError("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID"))?;
        output.push((a << 2) | (b >> 4));
        if block[3] == b'=' {
            if !last || c & 0x03 != 0 {
                return code("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID");
            }
            output.push((b << 4) | (c >> 2));
            continue;
        }
        let d = decode(block[3]).ok_or(EvalError("GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID"))?;
        output.push((b << 4) | (c >> 2));
        output.push((c << 6) | d);
    }
    Ok(output)
}

fn decode_observation(value: &Value) -> EvalResult<Observation> {
    let record = object(value, "GKX_EVAL_SOURCE_OBSERVATION_INVALID")?;
    exact_keys(
        record,
        &[
            "source_id",
            "source_path",
            "source_digest",
            "source_bytes_base64",
        ],
        "GKX_EVAL_SOURCE_OBSERVATION_FIELDS_INVALID",
    )?;
    let source_id = string(
        field(
            record,
            "source_id",
            "GKX_EVAL_SOURCE_OBSERVATION_IDENTITY_INVALID",
        )?,
        "GKX_EVAL_SOURCE_OBSERVATION_IDENTITY_INVALID",
    )?;
    let source_path = string(
        field(
            record,
            "source_path",
            "GKX_EVAL_SOURCE_OBSERVATION_IDENTITY_INVALID",
        )?,
        "GKX_EVAL_SOURCE_OBSERVATION_IDENTITY_INVALID",
    )?;
    let source_digest = string(
        field(
            record,
            "source_digest",
            "GKX_EVAL_SOURCE_OBSERVATION_DIGEST_INVALID",
        )?,
        "GKX_EVAL_SOURCE_OBSERVATION_DIGEST_INVALID",
    )?;
    if !is_valid_authored_uid(source_id)
        || !valid_source_path(source_path)
        || !is_digest(source_digest)
    {
        return code("GKX_EVAL_SOURCE_OBSERVATION_IDENTITY_INVALID");
    }
    let encoded = string(
        field(
            record,
            "source_bytes_base64",
            "GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID",
        )?,
        "GKX_EVAL_SOURCE_OBSERVATION_BASE64_INVALID",
    )?;
    let bytes = decode_base64(encoded)?;
    if bytes.len() > 64 * 1024 * 1024 || sha256(&bytes) != source_digest {
        return code("GKX_EVAL_SOURCE_OBSERVATION_BYTES_MISMATCH");
    }
    if std::str::from_utf8(&bytes).is_err() {
        return code("GKX_EVAL_SOURCE_OBSERVATION_UTF8_INVALID");
    }
    Ok(Observation {
        source_id: source_id.to_owned(),
        source_path: source_path.to_owned(),
        source_digest: source_digest.to_owned(),
        bytes,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CitationSpan {
    start_byte: usize,
    end_byte: usize,
    text: String,
}

fn normalize_lexical(value: &str) -> String {
    value
        .nfd()
        .filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character == '\u{03c2}' {
                '\u{03c3}'
            } else {
                character
            }
        })
        .collect()
}

fn lexical_term_character(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(
            character as u32,
            0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd
        )
}

fn lexical_terms(value: &str) -> Vec<String> {
    let normalized = normalize_lexical(value);
    let mut output = Vec::new();
    let mut current = String::new();
    for character in normalized.chars() {
        if lexical_term_character(character) {
            current.push(character);
        } else if !current.is_empty() {
            output.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

fn regex_whitespace(character: char) -> bool {
    is_ecmascript_whitespace(character) || matches!(character, '\n' | '\r')
}

fn lexical_query_groups(query: &str) -> Vec<(String, Vec<String>)> {
    let mut groups = Vec::new();
    let mut offset = 0;
    while offset < query.len() {
        let Some((character_offset, character)) = query[offset..].char_indices().next() else {
            break;
        };
        offset += character_offset;
        if regex_whitespace(character) {
            offset += character.len_utf8();
            continue;
        }
        if character == '"' {
            let start = offset + 1;
            if let Some(close) = query[start..].find('"') {
                let phrase = &query[start..start + close];
                let normalized = normalize_lexical(phrase);
                if !normalized.is_empty() {
                    groups.push((normalized, lexical_terms(phrase)));
                }
                offset = start + close + 1;
            } else {
                offset += 1;
            }
            continue;
        }
        let start = offset;
        while offset < query.len() {
            let next = query[offset..].chars().next().expect("valid offset");
            if next == '"' || regex_whitespace(next) {
                break;
            }
            offset += next.len_utf8();
        }
        for term in lexical_terms(&query[start..offset]) {
            groups.push((term, Vec::new()));
        }
    }
    groups
}

struct NormalizedOriginalMap {
    units: Vec<u16>,
    start_bytes: Vec<usize>,
    end_bytes: Vec<usize>,
}

fn normalized_original_map(value: &str) -> NormalizedOriginalMap {
    let mut units = Vec::new();
    let mut start_bytes = Vec::new();
    let mut end_bytes = Vec::new();
    let mut iter = value.char_indices().peekable();
    while let Some((start, _first)) = iter.next() {
        let mut end = iter.peek().map_or(value.len(), |(offset, _)| *offset);
        while iter
            .peek()
            .is_some_and(|(_, character)| is_combining_mark(*character))
        {
            iter.next();
            end = iter.peek().map_or(value.len(), |(offset, _)| *offset);
        }
        let normalized = normalize_lexical(&value[start..end]);
        for unit in normalized.encode_utf16() {
            units.push(unit);
            start_bytes.push(start);
            end_bytes.push(end);
        }
    }
    NormalizedOriginalMap {
        units,
        start_bytes,
        end_bytes,
    }
}

fn find_units(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() || from > haystack.len() || needle.len() > haystack.len() - from {
        return None;
    }
    (from..=haystack.len() - needle.len())
        .find(|offset| haystack[*offset..*offset + needle.len()] == *needle)
}

fn lexical_citation_spans(value: &str, query: &str) -> Vec<CitationSpan> {
    let groups = lexical_query_groups(query);
    let mapped = normalized_original_map(value);
    let mut candidates = Vec::new();
    let mut returned_bytes = 0_usize;
    let mut add_needle = |needle: &str| -> usize {
        let needle = needle.encode_utf16().collect::<Vec<_>>();
        if needle.is_empty() {
            return 0;
        }
        let mut added = 0;
        let mut offset = 0;
        while let Some(found) = find_units(&mapped.units, &needle, offset) {
            if candidates.len() >= 16 {
                break;
            }
            let start = mapped.start_bytes[found];
            let end = mapped.end_bytes[found + needle.len() - 1];
            let exact_bytes = end - start;
            if exact_bytes <= 256 && returned_bytes + exact_bytes <= 1024 {
                candidates.push(CitationSpan {
                    start_byte: start,
                    end_byte: end,
                    text: value[start..end].to_owned(),
                });
                returned_bytes += exact_bytes;
                added += 1;
            }
            offset = found + needle.len().max(1);
        }
        added
    };
    for (primary, fallback) in groups {
        if add_needle(&primary) == 0 {
            for item in fallback {
                add_needle(&item);
            }
        }
    }
    candidates.sort_by_key(|span| (span.start_byte, span.end_byte));
    candidates.dedup_by_key(|span| (span.start_byte, span.end_byte));
    candidates.truncate(8);
    candidates
}

fn utf8_boundary(bytes: &[u8], offset: usize) -> bool {
    offset <= bytes.len() && (offset == bytes.len() || bytes[offset] & 0xc0 != 0x80)
}

fn line_for_position(bytes: &[u8], position: usize) -> u64 {
    let mut line = 1_u64;
    let mut index = 0;
    while index <= position && index < bytes.len() {
        if bytes[index] == b'\r' {
            if bytes.get(index + 1) == Some(&b'\n') {
                if position < index + 2 {
                    break;
                }
                index += 2;
            } else {
                if position < index + 1 {
                    break;
                }
                index += 1;
            }
            line += 1;
            continue;
        }
        if bytes[index] == b'\n' {
            if position < index + 1 {
                break;
            }
            line += 1;
        }
        index += 1;
    }
    line
}

fn span_value(span: &CitationSpan) -> Value {
    json!({
        "start_byte": span.start_byte,
        "end_byte": span.end_byte,
        "text": span.text,
    })
}

#[derive(Clone, Copy, Default)]
struct CitationVerdict {
    passed: u64,
    mismatch: u64,
    stale: u64,
}

struct CitationExpected<'a> {
    source_id: &'a str,
    source_path: &'a str,
    source_digest: &'a str,
    content_digest: &'a str,
    text: &'a str,
    heading_path: Option<&'a Value>,
    start_byte: Option<usize>,
    end_byte: Option<usize>,
    start_line: Option<u64>,
    end_line: Option<u64>,
    matched_spans: Option<Vec<Value>>,
}

fn check_citation(
    citation_value: &Value,
    expected: &CitationExpected<'_>,
    observations: &HashMap<(String, String), Observation>,
) -> CitationVerdict {
    let Some(observation) = observations.get(&(
        expected.source_id.to_owned(),
        expected.source_path.to_owned(),
    )) else {
        return CitationVerdict {
            mismatch: 1,
            ..CitationVerdict::default()
        };
    };
    if expected.source_digest != observation.source_digest {
        return CitationVerdict {
            stale: 1,
            ..CitationVerdict::default()
        };
    }
    let Ok(citation) = object(citation_value, "GKX_EVAL_CITATION_INVALID") else {
        return CitationVerdict {
            mismatch: 1,
            ..CitationVerdict::default()
        };
    };
    let citation_string = |name: &str| citation.get(name).and_then(Value::as_str);
    let citation_count = |name: &str| citation.get(name).and_then(Value::as_u64);
    let start = citation_count("start_byte").and_then(|value| usize::try_from(value).ok());
    let end = citation_count("end_byte").and_then(|value| usize::try_from(value).ok());
    let mut mismatch = citation.get("verified") != Some(&Value::Bool(true))
        || citation.get("stale") != Some(&Value::Bool(false))
        || citation_string("source_id") != Some(expected.source_id)
        || citation_string("path") != Some(expected.source_path)
        || citation_string("source_digest") != Some(expected.source_digest)
        || start.is_none()
        || end.is_none();
    if let (Some(start), Some(end)) = (start, end) {
        mismatch |= start >= end
            || end > observation.bytes.len()
            || !utf8_boundary(&observation.bytes, start)
            || !utf8_boundary(&observation.bytes, end);
        if let (Some(expected_start), Some(expected_end)) = (expected.start_byte, expected.end_byte)
        {
            mismatch |= start != expected_start
                || end != expected_end
                || citation_count("start_line") != expected.start_line
                || citation_count("end_line") != expected.end_line;
        }
        if !mismatch {
            let slice = &observation.bytes[start..end];
            mismatch |= std::str::from_utf8(slice).ok() != Some(expected.text)
                || sha256(slice) != expected.content_digest
                || citation_count("start_line")
                    != Some(line_for_position(&observation.bytes, start))
                || citation_count("end_line")
                    != Some(line_for_position(&observation.bytes, end - 1));
        }
        if let Some(spans) = citation.get("matched_spans").and_then(Value::as_array) {
            for span in spans {
                let Some(record) = span.as_object() else {
                    mismatch = true;
                    continue;
                };
                let span_start = record
                    .get("start_byte")
                    .and_then(Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok());
                let span_end = record
                    .get("end_byte")
                    .and_then(Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok());
                let text = record.get("text").and_then(Value::as_str);
                match (span_start, span_end, text) {
                    (Some(span_start), Some(span_end), Some(text))
                        if span_start >= start
                            && span_start < span_end
                            && span_end <= end
                            && utf8_boundary(&observation.bytes, span_start)
                            && utf8_boundary(&observation.bytes, span_end)
                            && std::str::from_utf8(&observation.bytes[span_start..span_end])
                                .ok()
                                == Some(text) => {}
                    _ => mismatch = true,
                }
            }
        } else {
            mismatch = true;
        }
    }
    if let Some(heading) = expected.heading_path {
        mismatch |= citation.get("heading_path") != Some(heading);
    }
    mismatch |= expected
        .matched_spans
        .as_ref()
        .is_none_or(|spans| citation.get("matched_spans") != Some(&Value::Array(spans.clone())));
    if mismatch {
        CitationVerdict {
            mismatch: 1,
            ..CitationVerdict::default()
        }
    } else {
        CitationVerdict {
            passed: 1,
            ..CitationVerdict::default()
        }
    }
}

fn string_array(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn audit_scalar(
    value: Option<&Value>,
    authorized: &HashSet<&str>,
    forbidden: &HashSet<&str>,
    endpoint: bool,
    fields: &mut u64,
    leaks: &mut u64,
    unknown: &mut u64,
) -> EvalResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_null() && endpoint {
        return Ok(());
    }
    let Some(value) = value.as_str() else {
        return code("GKX_EVAL_PUBLIC_PROVENANCE_VALUE_INVALID");
    };
    if endpoint && !is_valid_authored_uid(value) {
        return code("GKX_EVAL_PUBLIC_PROVENANCE_VALUE_INVALID");
    }
    *fields += 1;
    if forbidden.contains(value) {
        *leaks += 1;
    } else if !authorized.contains(value) {
        *unknown += 1;
    }
    Ok(())
}

struct AuditSets<'a> {
    authorized_ids: &'a HashSet<&'a str>,
    forbidden_ids: &'a HashSet<&'a str>,
    authorized_paths: &'a HashSet<&'a str>,
    forbidden_paths: &'a HashSet<&'a str>,
    authorized_endpoints: &'a HashSet<&'a str>,
    forbidden_endpoints: &'a HashSet<&'a str>,
}

fn audit_provenance(
    provenance: &Map<String, Value>,
    sets: &AuditSets<'_>,
    fields: &mut u64,
    leaks: &mut u64,
    unknown: &mut u64,
) -> EvalResult<()> {
    audit_scalar(
        provenance.get("source_id"),
        sets.authorized_ids,
        sets.forbidden_ids,
        false,
        fields,
        leaks,
        unknown,
    )?;
    audit_scalar(
        provenance.get("source_path"),
        sets.authorized_paths,
        sets.forbidden_paths,
        false,
        fields,
        leaks,
        unknown,
    )?;
    audit_scalar(
        provenance.get("lineage_id"),
        sets.authorized_endpoints,
        sets.forbidden_endpoints,
        true,
        fields,
        leaks,
        unknown,
    )?;
    for key in ["supersedes", "superseded_by"] {
        for endpoint in string_array(provenance.get(key)) {
            let endpoint = Value::String(endpoint.to_owned());
            audit_scalar(
                Some(&endpoint),
                sets.authorized_endpoints,
                sets.forbidden_endpoints,
                true,
                fields,
                leaks,
                unknown,
            )?;
        }
    }
    Ok(())
}

fn policy_metrics(
    result: &Map<String, Value>,
    query: &Map<String, Value>,
    oracle: &Map<String, Value>,
) -> EvalResult<Value> {
    let forbidden_query = string_array(query.get("forbidden_source_ids"));
    let authorized_ids = string_array(oracle.get("authorized_source_ids"));
    let forbidden_ids = string_array(oracle.get("forbidden_source_ids"));
    let authorized_paths = string_array(oracle.get("authorized_source_paths"));
    let forbidden_paths = string_array(oracle.get("forbidden_source_paths"));
    let authorized_endpoints = string_array(oracle.get("authorized_endpoint_ids"));
    let forbidden_endpoints = string_array(oracle.get("forbidden_endpoint_ids"));
    let authorized_ids = authorized_ids.into_iter().collect::<HashSet<_>>();
    let forbidden_ids = forbidden_ids
        .into_iter()
        .chain(forbidden_query)
        .collect::<HashSet<_>>();
    let authorized_paths = authorized_paths.into_iter().collect::<HashSet<_>>();
    let forbidden_paths = forbidden_paths.into_iter().collect::<HashSet<_>>();
    let authorized_endpoints = authorized_endpoints.into_iter().collect::<HashSet<_>>();
    let forbidden_endpoints = forbidden_endpoints.into_iter().collect::<HashSet<_>>();
    let mut fields = 2_u64;
    let mut leaks = 0_u64;
    let mut unknown = 0_u64;
    if result.get("projection_id") != oracle.get("expected_public_result_projection_id") {
        leaks += 1;
    }
    if result.get("projection_digest") != oracle.get("expected_public_result_projection_digest") {
        leaks += 1;
    }
    let sets = AuditSets {
        authorized_ids: &authorized_ids,
        forbidden_ids: &forbidden_ids,
        authorized_paths: &authorized_paths,
        forbidden_paths: &forbidden_paths,
        authorized_endpoints: &authorized_endpoints,
        forbidden_endpoints: &forbidden_endpoints,
    };
    for hit in array(
        field(result, "hits", "GKX_EVAL_PUBLIC_RESULT_INVALID")?,
        "GKX_EVAL_PUBLIC_RESULT_INVALID",
    )? {
        let hit = object(hit, "GKX_EVAL_PUBLIC_HIT_INVALID")?;
        let chunk = object(
            field(hit, "chunk", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let citation = object(
            field(hit, "citation", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let provenance = object(
            field(hit, "provenance", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        audit_scalar(
            chunk.get("source_id"),
            &authorized_ids,
            &forbidden_ids,
            false,
            &mut fields,
            &mut leaks,
            &mut unknown,
        )?;
        audit_scalar(
            chunk.get("source_path"),
            &authorized_paths,
            &forbidden_paths,
            false,
            &mut fields,
            &mut leaks,
            &mut unknown,
        )?;
        audit_scalar(
            chunk.get("lineage_id"),
            &authorized_endpoints,
            &forbidden_endpoints,
            true,
            &mut fields,
            &mut leaks,
            &mut unknown,
        )?;
        for key in ["supersedes", "superseded_by"] {
            for endpoint in string_array(chunk.get(key)) {
                audit_scalar(
                    Some(&Value::String(endpoint.to_owned())),
                    &authorized_endpoints,
                    &forbidden_endpoints,
                    true,
                    &mut fields,
                    &mut leaks,
                    &mut unknown,
                )?;
            }
        }
        audit_scalar(
            citation.get("source_id"),
            &authorized_ids,
            &forbidden_ids,
            false,
            &mut fields,
            &mut leaks,
            &mut unknown,
        )?;
        audit_scalar(
            citation.get("path"),
            &authorized_paths,
            &forbidden_paths,
            false,
            &mut fields,
            &mut leaks,
            &mut unknown,
        )?;
        audit_provenance(provenance, &sets, &mut fields, &mut leaks, &mut unknown)?;
        if let Some(parent) = hit.get("parent_context").filter(|value| !value.is_null()) {
            let parent = object(parent, "GKX_EVAL_PUBLIC_PARENT_INVALID")?;
            let parent_citation = object(
                field(parent, "citation", "GKX_EVAL_PUBLIC_PARENT_INVALID")?,
                "GKX_EVAL_PUBLIC_PARENT_INVALID",
            )?;
            let parent_provenance = object(
                field(parent, "provenance", "GKX_EVAL_PUBLIC_PARENT_INVALID")?,
                "GKX_EVAL_PUBLIC_PARENT_INVALID",
            )?;
            audit_scalar(
                parent_citation.get("source_id"),
                &authorized_ids,
                &forbidden_ids,
                false,
                &mut fields,
                &mut leaks,
                &mut unknown,
            )?;
            audit_scalar(
                parent_citation.get("path"),
                &authorized_paths,
                &forbidden_paths,
                false,
                &mut fields,
                &mut leaks,
                &mut unknown,
            )?;
            audit_provenance(
                parent_provenance,
                &sets,
                &mut fields,
                &mut leaks,
                &mut unknown,
            )?;
        }
    }
    if unknown != 0 {
        return code("GKX_EVAL_ORACLE_PARTITION_INCOMPLETE");
    }
    Ok(json!({
        "policy_identity_field_count": fields,
        "policy_leak_count": leaks,
        "policy_leak_rate_micros": if fields == 0 { 0 } else { round_ratio(u128::from(leaks), u128::from(fields), METRIC_SCALE)? },
    }))
}

fn seal_oracle(value: &Value) -> EvalResult<Value> {
    let oracle = object(value, "GKX_EVAL_ORACLE_INVALID")?;
    exact_keys(
        oracle,
        &[
            "contract_version",
            "authorized_source_ids",
            "authorized_source_paths",
            "forbidden_source_ids",
            "forbidden_source_paths",
            "authorized_endpoint_ids",
            "forbidden_endpoint_ids",
            "expected_public_result_projection_id",
            "expected_public_result_projection_digest",
            "oracle_digest",
        ],
        "GKX_EVAL_ORACLE_FIELDS_INVALID",
    )?;
    if oracle.get("contract_version") != Some(&Value::String(QUERY_VIEW_ORACLE_VERSION.to_owned()))
    {
        return code("GKX_EVAL_ORACLE_CONTRACT_VERSION_INVALID");
    }
    let authorized_ids = sorted_unique_strings(
        field(
            oracle,
            "authorized_source_ids",
            "GKX_EVAL_ORACLE_IDENTITY_INVALID",
        )?,
        4096,
        "GKX_EVAL_ORACLE_IDENTITY_INVALID",
    )?;
    let forbidden_ids = sorted_unique_strings(
        field(
            oracle,
            "forbidden_source_ids",
            "GKX_EVAL_ORACLE_IDENTITY_INVALID",
        )?,
        4096,
        "GKX_EVAL_ORACLE_IDENTITY_INVALID",
    )?;
    let authorized_paths = sorted_unique_strings(
        field(
            oracle,
            "authorized_source_paths",
            "GKX_EVAL_ORACLE_IDENTITY_INVALID",
        )?,
        4096,
        "GKX_EVAL_ORACLE_IDENTITY_INVALID",
    )?;
    let forbidden_paths = sorted_unique_strings(
        field(
            oracle,
            "forbidden_source_paths",
            "GKX_EVAL_ORACLE_IDENTITY_INVALID",
        )?,
        4096,
        "GKX_EVAL_ORACLE_IDENTITY_INVALID",
    )?;
    let authorized_endpoints = sorted_unique_strings(
        field(
            oracle,
            "authorized_endpoint_ids",
            "GKX_EVAL_ORACLE_IDENTITY_INVALID",
        )?,
        4096,
        "GKX_EVAL_ORACLE_IDENTITY_INVALID",
    )?;
    let forbidden_endpoints = sorted_unique_strings(
        field(
            oracle,
            "forbidden_endpoint_ids",
            "GKX_EVAL_ORACLE_IDENTITY_INVALID",
        )?,
        4096,
        "GKX_EVAL_ORACLE_IDENTITY_INVALID",
    )?;
    if authorized_ids
        .iter()
        .chain(&forbidden_ids)
        .chain(&authorized_endpoints)
        .chain(&forbidden_endpoints)
        .any(|uid| !is_valid_authored_uid(uid))
        || authorized_paths
            .iter()
            .chain(&forbidden_paths)
            .any(|path| !valid_source_path(path))
    {
        return code("GKX_EVAL_ORACLE_IDENTITY_INVALID");
    }
    if authorized_ids
        .iter()
        .any(|value| forbidden_ids.contains(value))
        || authorized_paths
            .iter()
            .any(|value| forbidden_paths.contains(value))
        || authorized_endpoints
            .iter()
            .any(|value| forbidden_endpoints.contains(value))
    {
        return code("GKX_EVAL_ORACLE_AUTHORIZATION_OVERLAP");
    }
    let projection_id = string(
        field(
            oracle,
            "expected_public_result_projection_id",
            "GKX_EVAL_ORACLE_PROJECTION_ID_INVALID",
        )?,
        "GKX_EVAL_ORACLE_PROJECTION_ID_INVALID",
    )?;
    let projection_digest = string(
        field(
            oracle,
            "expected_public_result_projection_digest",
            "GKX_EVAL_ORACLE_PROJECTION_DIGEST_INVALID",
        )?,
        "GKX_EVAL_ORACLE_PROJECTION_DIGEST_INVALID",
    )?;
    if !is_digest(projection_digest)
        || projection_id != format!("retrieval:{}", &projection_digest[7..31])
    {
        return code("GKX_EVAL_ORACLE_PROJECTION_BINDING_INVALID");
    }
    verify_digest(value, "oracle_digest", "GKX_EVAL_ORACLE_DIGEST_MISMATCH")?;
    Ok(value.clone())
}

fn seal_public_result(value: &Value, query: &Map<String, Value>) -> EvalResult<Value> {
    let result = object(value, "GKX_EVAL_PUBLIC_RESULT_INVALID")?;
    exact_keys(
        result,
        &[
            "contract_version",
            "query_digest",
            "projection_id",
            "projection_digest",
            "projection_freshness",
            "hits",
            "confidence",
            "temporal",
            "applied_filters",
            "eligible_result_count",
            "stages",
        ],
        "GKX_EVAL_PUBLIC_RESULT_FIELDS_INVALID",
    )?;
    if result.get("contract_version")
        != Some(&Value::String("gkos-retrieval/1.0.0-draft.2".to_owned()))
    {
        return code("GKX_EVAL_PUBLIC_RESULT_COORDINATE_INVALID");
    }
    let authored = string(
        field(query, "text", "GKX_EVAL_QUERY_TEXT_INVALID")?,
        "GKX_EVAL_QUERY_TEXT_INVALID",
    )?;
    let effective = effective_query_text(authored);
    let expected_query_digest = canonical_digest(&json!({
        "as_of": field(query, "as_of", "GKX_EVAL_QUERY_INVALID")?,
        "query": effective,
    }))
    .map_err(|_| EvalError("GKX_EVAL_PUBLIC_RESULT_QUERY_DIGEST_INVALID"))?;
    if result.get("query_digest") != Some(&Value::String(expected_query_digest)) {
        return code("GKX_EVAL_PUBLIC_RESULT_QUERY_DIGEST_INVALID");
    }
    let projection_id = result
        .get("projection_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let projection_digest = result
        .get("projection_digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !is_digest(projection_digest)
        || projection_id != format!("retrieval:{}", &projection_digest[7..31])
        || !matches!(
            result.get("projection_freshness").and_then(Value::as_str),
            Some("fresh" | "stale" | "unverified")
        )
    {
        return code("GKX_EVAL_PUBLIC_RESULT_PROJECTION_INVALID");
    }
    let hits = array(
        field(result, "hits", "GKX_EVAL_PUBLIC_RESULT_HITS_INVALID")?,
        "GKX_EVAL_PUBLIC_RESULT_HITS_INVALID",
    )?;
    let top_k = field(query, "expected_top_k", "GKX_EVAL_TOP_K_INVALID")?
        .as_u64()
        .ok_or(EvalError("GKX_EVAL_TOP_K_INVALID"))?;
    if hits.len() > usize::try_from(top_k).unwrap_or(usize::MAX) {
        return code("GKX_EVAL_PUBLIC_RESULT_HITS_INVALID");
    }
    for (index, hit_value) in hits.iter().enumerate() {
        let hit = object(hit_value, "GKX_EVAL_PUBLIC_HIT_INVALID")?;
        let chunk = object(
            field(hit, "chunk", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let citation = object(
            field(hit, "citation", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let provenance = object(
            field(hit, "provenance", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let source_id = chunk
            .get("source_id")
            .and_then(Value::as_str)
            .ok_or(EvalError("GKX_EVAL_PUBLIC_RESULT_IDENTITY_MISMATCH"))?;
        let source_path = chunk
            .get("source_path")
            .and_then(Value::as_str)
            .ok_or(EvalError("GKX_EVAL_PUBLIC_RESULT_IDENTITY_MISMATCH"))?;
        if citation.get("source_id").and_then(Value::as_str) != Some(source_id)
            || citation.get("path").and_then(Value::as_str) != Some(source_path)
            || provenance.get("source_id").and_then(Value::as_str) != Some(source_id)
            || provenance.get("source_path").and_then(Value::as_str) != Some(source_path)
        {
            return code("GKX_EVAL_PUBLIC_RESULT_IDENTITY_MISMATCH");
        }
        let text = chunk
            .get("text")
            .and_then(Value::as_str)
            .ok_or(EvalError("GKX_EVAL_PUBLIC_CHUNK_INVALID"))?;
        if chunk.get("content_digest").and_then(Value::as_str)
            != Some(sha256(text.as_bytes()).as_str())
        {
            return code("GKX_EVAL_PUBLIC_CHUNK_DIGEST_MISMATCH");
        }
        for endpoint_key in ["lineage_id", "supersedes", "superseded_by"] {
            if endpoint_key == "lineage_id" {
                if let Some(endpoint) = chunk.get(endpoint_key).filter(|item| !item.is_null()) {
                    if !endpoint.as_str().is_some_and(is_valid_authored_uid) {
                        return code("GKX_EVAL_PUBLIC_PROVENANCE_VALUE_INVALID");
                    }
                }
            } else if string_array(chunk.get(endpoint_key))
                .iter()
                .any(|endpoint| !is_valid_authored_uid(endpoint))
            {
                return code("GKX_EVAL_PUBLIC_PROVENANCE_VALUE_INVALID");
            }
        }
        let scores = object(
            field(hit, "stage_scores", "GKX_EVAL_PUBLIC_STAGE_SCORES_INVALID")?,
            "GKX_EVAL_PUBLIC_STAGE_SCORES_INVALID",
        )?;
        if scores.get("final_rank").and_then(Value::as_u64) != Some(index as u64 + 1) {
            return code("GKX_EVAL_PUBLIC_FINAL_RANK_INVALID");
        }
    }
    let temporal = object(
        field(result, "temporal", "GKX_EVAL_PUBLIC_TEMPORAL_INVALID")?,
        "GKX_EVAL_PUBLIC_TEMPORAL_INVALID",
    )?;
    if temporal.get("as_of") != query.get("as_of")
        || !matches!(
            temporal.get("coverage").and_then(Value::as_str),
            Some("not_requested" | "not_evaluated" | "sufficient" | "insufficient")
        )
    {
        return code("GKX_EVAL_PUBLIC_TEMPORAL_INVALID");
    }
    let confidence = object(
        field(result, "confidence", "GKX_EVAL_PUBLIC_CONFIDENCE_INVALID")?,
        "GKX_EVAL_PUBLIC_CONFIDENCE_INVALID",
    )?;
    if !matches!(
        confidence.get("level").and_then(Value::as_str),
        Some("high" | "medium" | "low" | "insufficient")
    ) {
        return code("GKX_EVAL_PUBLIC_CONFIDENCE_INVALID");
    }
    Ok(value.clone())
}

fn public_temporal_projection(result: &Map<String, Value>) -> EvalResult<Value> {
    let temporal = object(
        field(result, "temporal", "GKX_EVAL_PUBLIC_TEMPORAL_INVALID")?,
        "GKX_EVAL_PUBLIC_TEMPORAL_INVALID",
    )?;
    let hits = array(
        field(result, "hits", "GKX_EVAL_PUBLIC_RESULT_INVALID")?,
        "GKX_EVAL_PUBLIC_RESULT_INVALID",
    )?;
    let projections = hits
        .iter()
        .map(|hit| {
            let hit = object(hit, "GKX_EVAL_PUBLIC_HIT_INVALID")?;
            let provenance = object(field(hit, "provenance", "GKX_EVAL_PUBLIC_HIT_INVALID")?, "GKX_EVAL_PUBLIC_HIT_INVALID")?;
            Ok(json!({
                "source_id": field(provenance, "source_id", "GKX_EVAL_PUBLIC_PROVENANCE_INVALID")?,
                "temporal_state": field(provenance, "temporal_state", "GKX_EVAL_PUBLIC_PROVENANCE_INVALID")?,
                "valid_from": field(provenance, "valid_from", "GKX_EVAL_PUBLIC_PROVENANCE_INVALID")?,
                "valid_to": field(provenance, "valid_to", "GKX_EVAL_PUBLIC_PROVENANCE_INVALID")?,
                "supersedes": field(provenance, "supersedes", "GKX_EVAL_PUBLIC_PROVENANCE_INVALID")?,
                "superseded_by": field(provenance, "superseded_by", "GKX_EVAL_PUBLIC_PROVENANCE_INVALID")?,
            }))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    Ok(json!({
        "coverage": field(temporal, "coverage", "GKX_EVAL_PUBLIC_TEMPORAL_INVALID")?,
        "hits": projections,
    }))
}

fn compute_query_metrics(input: &Value) -> EvalResult<Value> {
    let input_record = object(input, "GKX_EVAL_QUERY_INPUT_INVALID")?;
    exact_keys(
        input_record,
        &[
            "query",
            "result",
            "source_observations",
            "audit_oracle",
            "expected_temporal",
        ],
        "GKX_EVAL_QUERY_INPUT_FIELDS_INVALID",
    )?;
    let query_value = field(input_record, "query", "GKX_EVAL_QUERY_INPUT_INVALID")?;
    seal_normalized_query(query_value)?;
    let query = object(query_value, "GKX_EVAL_QUERY_INVALID")?;
    let oracle_value = field(input_record, "audit_oracle", "GKX_EVAL_QUERY_INPUT_INVALID")?;
    seal_oracle(oracle_value)?;
    let oracle = object(oracle_value, "GKX_EVAL_ORACLE_INVALID")?;
    let result_value = field(input_record, "result", "GKX_EVAL_QUERY_INPUT_INVALID")?;
    seal_public_result(result_value, query)?;
    let result = object(result_value, "GKX_EVAL_PUBLIC_RESULT_INVALID")?;

    let observation_values = array(
        field(
            input_record,
            "source_observations",
            "GKX_EVAL_SOURCE_OBSERVATION_COUNT_INVALID",
        )?,
        "GKX_EVAL_SOURCE_OBSERVATION_COUNT_INVALID",
    )?;
    if observation_values.len() > 4096 {
        return code("GKX_EVAL_SOURCE_OBSERVATION_COUNT_INVALID");
    }
    let mut observations = HashMap::new();
    let mut by_source = HashMap::new();
    let mut by_path = HashMap::new();
    let mut total_bytes = 0_usize;
    for raw in observation_values {
        let observation = decode_observation(raw)?;
        total_bytes = total_bytes
            .checked_add(observation.bytes.len())
            .ok_or(EvalError("GKX_EVAL_SOURCE_OBSERVATION_TOTAL_SIZE_INVALID"))?;
        if total_bytes > 512 * 1024 * 1024 {
            return code("GKX_EVAL_SOURCE_OBSERVATION_TOTAL_SIZE_INVALID");
        }
        if observations
            .insert(
                (
                    observation.source_id.clone(),
                    observation.source_path.clone(),
                ),
                observation.clone(),
            )
            .is_some()
            || by_source
                .insert(observation.source_id.clone(), observation.clone())
                .is_some()
            || by_path
                .insert(observation.source_path.clone(), observation.clone())
                .is_some()
        {
            return code("GKX_EVAL_SOURCE_OBSERVATION_ONE_TO_ONE_INVALID");
        }
    }
    let mut observed_ids = by_source.keys().cloned().collect::<Vec<_>>();
    observed_ids.sort_by(|left, right| utf16_cmp(left, right));
    let mut observed_paths = by_path.keys().cloned().collect::<Vec<_>>();
    observed_paths.sort_by(|left, right| utf16_cmp(left, right));
    let mut oracle_ids = string_array(oracle.get("authorized_source_ids"))
        .into_iter()
        .chain(string_array(oracle.get("forbidden_source_ids")))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    oracle_ids.sort_by(|left, right| utf16_cmp(left, right));
    let mut oracle_paths = string_array(oracle.get("authorized_source_paths"))
        .into_iter()
        .chain(string_array(oracle.get("forbidden_source_paths")))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    oracle_paths.sort_by(|left, right| utf16_cmp(left, right));
    if observed_ids != oracle_ids || observed_paths != oracle_paths {
        return code("GKX_EVAL_ORACLE_CATALOG_PARTITION_INCOMPLETE");
    }
    let authorized_ids = string_array(oracle.get("authorized_source_ids"));
    let authorized_paths = string_array(oracle.get("authorized_source_paths"));
    let forbidden_ids = string_array(oracle.get("forbidden_source_ids"));
    let forbidden_paths = string_array(oracle.get("forbidden_source_paths"));
    for observation in observations.values() {
        let authorized_id = authorized_ids.contains(&observation.source_id.as_str());
        let authorized_path = authorized_paths.contains(&observation.source_path.as_str());
        let forbidden_id = forbidden_ids.contains(&observation.source_id.as_str());
        let forbidden_path = forbidden_paths.contains(&observation.source_path.as_str());
        if authorized_id != authorized_path
            || forbidden_id != forbidden_path
            || authorized_id == forbidden_id
        {
            return code("GKX_EVAL_ORACLE_CATALOG_PARTITION_INCOMPLETE");
        }
    }

    let mut relevant = BTreeSet::new();
    for source in string_array(query.get("expected_source_ids")) {
        if !by_source.contains_key(source) {
            return code("GKX_EVAL_EXPECTED_SOURCE_RESOLUTION_INVALID");
        }
        relevant.insert(source.to_owned());
    }
    for source in string_array(query.get("forbidden_source_ids")) {
        let Some(observation) = by_source.get(source) else {
            return code("GKX_EVAL_FORBIDDEN_SOURCE_RESOLUTION_INVALID");
        };
        if !forbidden_ids.contains(&source)
            || !forbidden_paths.contains(&observation.source_path.as_str())
        {
            return code("GKX_EVAL_FORBIDDEN_SOURCE_RESOLUTION_INVALID");
        }
    }
    for path in string_array(query.get("expected_files")) {
        let Some(observation) = by_path.get(path) else {
            return code("GKX_EVAL_EXPECTED_FILE_RESOLUTION_INVALID");
        };
        relevant.insert(observation.source_id.clone());
    }
    if relevant.is_empty()
        || relevant
            .iter()
            .any(|source| forbidden_ids.contains(&source.as_str()))
        || relevant.iter().any(|source| {
            by_source.get(source).is_some_and(|observation| {
                forbidden_paths.contains(&observation.source_path.as_str())
            })
        })
    {
        return code("GKX_EVAL_RELEVANCE_INVALID");
    }

    let hits = array(
        field(result, "hits", "GKX_EVAL_PUBLIC_RESULT_INVALID")?,
        "GKX_EVAL_PUBLIC_RESULT_INVALID",
    )?;
    let mut first_source_ranks = BTreeMap::new();
    for (index, hit) in hits.iter().enumerate() {
        let chunk = object(
            field(
                object(hit, "GKX_EVAL_PUBLIC_HIT_INVALID")?,
                "chunk",
                "GKX_EVAL_PUBLIC_HIT_INVALID",
            )?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let source = string(
            field(
                chunk,
                "source_id",
                "GKX_EVAL_PUBLIC_RESULT_IDENTITY_MISMATCH",
            )?,
            "GKX_EVAL_PUBLIC_RESULT_IDENTITY_MISMATCH",
        )?;
        first_source_ranks
            .entry(source.to_owned())
            .or_insert(index as u64 + 1);
    }
    let relevant_ranks = first_source_ranks
        .iter()
        .filter_map(|(source, rank)| relevant.contains(source).then_some(*rank))
        .collect::<Vec<_>>();
    let top_k = query
        .get("expected_top_k")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let recall = round_ratio(
        relevant_ranks.len() as u128,
        relevant.len() as u128,
        METRIC_SCALE,
    )?;
    let first_relevant = relevant_ranks.first().copied();
    let mrr =
        first_relevant.map_or(Ok(0), |rank| round_ratio(1, u128::from(rank), METRIC_SCALE))?;
    let ndcg = ndcg_micros(&relevant_ranks, relevant.len() as u64, top_k)?;

    let effective_query = effective_query_text(
        query
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let mut citation_checked = 0_u64;
    let mut citation_passed = 0_u64;
    let mut citation_mismatch = 0_u64;
    let mut citation_stale = 0_u64;
    let mut claimed_spans = HashSet::new();
    let mut accepted_intervals = Vec::<(String, usize, usize)>::new();
    for hit_value in hits {
        let hit = object(hit_value, "GKX_EVAL_PUBLIC_HIT_INVALID")?;
        let chunk = object(
            field(hit, "chunk", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            "GKX_EVAL_PUBLIC_HIT_INVALID",
        )?;
        let source_id = chunk
            .get("source_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source_path = chunk
            .get("source_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source_digest = chunk
            .get("source_digest")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content_digest = chunk
            .get("content_digest")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = chunk
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let chunk_start = chunk
            .get("start_byte")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(usize::MAX);
        let chunk_end = chunk
            .get("end_byte")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(usize::MAX);
        let mut spans = lexical_citation_spans(text, effective_query)
            .into_iter()
            .map(|mut span| {
                span.start_byte += chunk_start;
                span.end_byte += chunk_start;
                span
            })
            .collect::<Vec<_>>();
        let expected_spans = if spans.is_empty() {
            let overlap = accepted_intervals
                .iter()
                .any(|(accepted_source, start, end)| {
                    accepted_source == source_id
                        && (*start).max(chunk_start) < (*end).min(chunk_end)
                });
            (!overlap).then_some(Vec::new())
        } else {
            spans.retain(|span| {
                !claimed_spans.contains(&(source_id.to_owned(), span.start_byte, span.end_byte))
            });
            (!spans.is_empty()).then(|| spans.iter().map(span_value).collect::<Vec<_>>())
        };
        let child = check_citation(
            field(hit, "citation", "GKX_EVAL_PUBLIC_HIT_INVALID")?,
            &CitationExpected {
                source_id,
                source_path,
                source_digest,
                content_digest,
                text,
                heading_path: chunk.get("heading_path"),
                start_byte: Some(chunk_start),
                end_byte: Some(chunk_end),
                start_line: chunk.get("start_line").and_then(Value::as_u64),
                end_line: chunk.get("end_line").and_then(Value::as_u64),
                matched_spans: expected_spans.clone(),
            },
            &observations,
        );
        citation_checked += 1;
        citation_passed += child.passed;
        citation_mismatch += child.mismatch;
        citation_stale += child.stale;
        if expected_spans.is_some() {
            for span in spans {
                claimed_spans.insert((source_id.to_owned(), span.start_byte, span.end_byte));
            }
            accepted_intervals.push((source_id.to_owned(), chunk_start, chunk_end));
        }
        if let Some(parent_value) = hit.get("parent_context").filter(|value| !value.is_null()) {
            let parent = object(parent_value, "GKX_EVAL_PUBLIC_PARENT_INVALID")?;
            let provenance = object(
                field(parent, "provenance", "GKX_EVAL_PUBLIC_PARENT_INVALID")?,
                "GKX_EVAL_PUBLIC_PARENT_INVALID",
            )?;
            let assertion = object(
                field(
                    provenance,
                    "assertion",
                    "GKX_EVAL_PUBLIC_PROVENANCE_INVALID",
                )?,
                "GKX_EVAL_PUBLIC_PROVENANCE_INVALID",
            )?;
            let parent_text = parent
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let parent_citation = object(
                field(parent, "citation", "GKX_EVAL_PUBLIC_PARENT_INVALID")?,
                "GKX_EVAL_PUBLIC_PARENT_INVALID",
            )?;
            let parent_source = provenance
                .get("source_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let parent_path = parent_citation
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let parent_digest = provenance
                .get("source_digest")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let parent_content_digest = assertion
                .get("content_digest")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let parent = check_citation(
                field(parent, "citation", "GKX_EVAL_PUBLIC_PARENT_INVALID")?,
                &CitationExpected {
                    source_id: parent_source,
                    source_path: parent_path,
                    source_digest: parent_digest,
                    content_digest: parent_content_digest,
                    text: parent_text,
                    heading_path: None,
                    start_byte: None,
                    end_byte: None,
                    start_line: None,
                    end_line: None,
                    matched_spans: Some(Vec::new()),
                },
                &observations,
            );
            citation_checked += 1;
            citation_passed += parent.passed;
            citation_mismatch += parent.mismatch;
            citation_stale += parent.stale;
        }
    }
    let citation = json!({
        "applicability": if hits.is_empty() { "not_applicable" } else { "required" },
        "checked": citation_checked,
        "passed": citation_passed,
        "mismatch": citation_mismatch,
        "stale": citation_stale,
        "correctness_micros": if citation_checked == 0 { Value::Null } else { json!(round_ratio(u128::from(citation_passed), u128::from(citation_checked), METRIC_SCALE)?) },
    });
    let policy = policy_metrics(result, query, oracle)?;
    let temporal_mismatch = u64::from(
        public_temporal_projection(result)?
            != *field(
                input_record,
                "expected_temporal",
                "GKX_EVAL_QUERY_INPUT_INVALID",
            )?,
    );
    let confidence = object(
        field(result, "confidence", "GKX_EVAL_PUBLIC_CONFIDENCE_INVALID")?,
        "GKX_EVAL_PUBLIC_CONFIDENCE_INVALID",
    )?;
    let confidence_mismatch =
        u64::from(confidence.get("level") != query.get("expected_confidence"));
    let material = json!({
        "contract_version": QUERY_METRICS_VERSION,
        "query_id": field(query, "id", "GKX_EVAL_QUERY_ID_INVALID")?,
        "expected_top_k": top_k,
        "relevant_source_count": relevant.len(),
        "returned_unique_source_count": first_source_ranks.len(),
        "relevant_returned_source_count": relevant_ranks.len(),
        "relevant_source_ranks": relevant_ranks,
        "first_relevant_rank": first_relevant,
        "recall_at_k_micros": recall,
        "mrr_micros": mrr,
        "ndcg_at_k_micros": ndcg,
        "citation": citation,
        "policy": policy,
        "confidence_mismatch_count": confidence_mismatch,
        "temporal_mismatch_count": temporal_mismatch,
        "stale_citation_query_count": u64::from(citation_stale > 0),
        "stale_projection_query_count": u64::from(result.get("projection_freshness").and_then(Value::as_str) == Some("stale")),
        "unverified_projection_query_count": u64::from(result.get("projection_freshness").and_then(Value::as_str) == Some("unverified")),
    });
    let mut output = object(&material, "GKX_EVAL_QUERY_METRICS_INVALID")?.clone();
    output.insert(
        "query_metrics_digest".to_owned(),
        Value::String(
            canonical_digest(&material)
                .map_err(|_| EvalError("GKX_EVAL_QUERY_METRICS_DIGEST_INVALID"))?,
        ),
    );
    let output = Value::Object(output);
    seal_query_metrics(&output)?;
    Ok(output)
}

fn seal_citation_metrics(value: &Value) -> EvalResult<()> {
    let citation = object(value, "GKX_EVAL_CITATION_METRICS_INVALID")?;
    exact_keys(
        citation,
        &[
            "applicability",
            "checked",
            "passed",
            "mismatch",
            "stale",
            "correctness_micros",
        ],
        "GKX_EVAL_CITATION_METRICS_FIELDS_INVALID",
    )?;
    let applicability = citation.get("applicability").and_then(Value::as_str);
    if !matches!(applicability, Some("required" | "not_applicable")) {
        return code("GKX_EVAL_CITATION_APPLICABILITY_INVALID");
    }
    let checked = count(
        field(citation, "checked", "GKX_EVAL_CITATION_CHECKED_INVALID")?,
        51_200,
        "GKX_EVAL_CITATION_CHECKED_INVALID",
    )?;
    let passed = count(
        field(citation, "passed", "GKX_EVAL_CITATION_PASSED_INVALID")?,
        51_200,
        "GKX_EVAL_CITATION_PASSED_INVALID",
    )?;
    let mismatch = count(
        field(citation, "mismatch", "GKX_EVAL_CITATION_MISMATCH_INVALID")?,
        51_200,
        "GKX_EVAL_CITATION_MISMATCH_INVALID",
    )?;
    let stale = count(
        field(citation, "stale", "GKX_EVAL_CITATION_STALE_INVALID")?,
        51_200,
        "GKX_EVAL_CITATION_STALE_INVALID",
    )?;
    if passed + mismatch + stale != checked {
        return code("GKX_EVAL_CITATION_COUNT_MISMATCH");
    }
    let correctness = citation
        .get("correctness_micros")
        .ok_or(EvalError("GKX_EVAL_CITATION_CORRECTNESS_INVALID"))?;
    let expected = if checked == 0 {
        Value::Null
    } else {
        json!(round_ratio(
            u128::from(passed),
            u128::from(checked),
            METRIC_SCALE
        )?)
    };
    if *correctness != expected {
        return code("GKX_EVAL_CITATION_CORRECTNESS_MISMATCH");
    }
    if applicability == Some("not_applicable") && checked != 0 {
        return code("GKX_EVAL_CITATION_NOT_APPLICABLE_INVALID");
    }
    Ok(())
}

fn seal_policy_metrics(value: &Value) -> EvalResult<()> {
    let policy = object(value, "GKX_EVAL_POLICY_METRICS_INVALID")?;
    exact_keys(
        policy,
        &[
            "policy_identity_field_count",
            "policy_leak_count",
            "policy_leak_rate_micros",
        ],
        "GKX_EVAL_POLICY_METRICS_FIELDS_INVALID",
    )?;
    let fields = count(
        field(
            policy,
            "policy_identity_field_count",
            "GKX_EVAL_POLICY_FIELD_COUNT_INVALID",
        )?,
        JS_MAX_SAFE_INTEGER,
        "GKX_EVAL_POLICY_FIELD_COUNT_INVALID",
    )?;
    let leaks = count(
        field(
            policy,
            "policy_leak_count",
            "GKX_EVAL_POLICY_LEAK_COUNT_INVALID",
        )?,
        JS_MAX_SAFE_INTEGER,
        "GKX_EVAL_POLICY_LEAK_COUNT_INVALID",
    )?;
    let rate = count(
        field(
            policy,
            "policy_leak_rate_micros",
            "GKX_EVAL_POLICY_LEAK_RATE_INVALID",
        )?,
        1_000_000,
        "GKX_EVAL_POLICY_LEAK_RATE_INVALID",
    )?;
    if leaks > fields
        || rate
            != if fields == 0 {
                0
            } else {
                round_ratio(u128::from(leaks), u128::from(fields), METRIC_SCALE)?
            }
    {
        return code("GKX_EVAL_POLICY_RATE_MISMATCH");
    }
    Ok(())
}

fn seal_query_metrics(value: &Value) -> EvalResult<Value> {
    let metrics = object(value, "GKX_EVAL_QUERY_METRICS_INVALID")?;
    exact_keys(
        metrics,
        &[
            "contract_version",
            "query_id",
            "expected_top_k",
            "relevant_source_count",
            "returned_unique_source_count",
            "relevant_returned_source_count",
            "relevant_source_ranks",
            "first_relevant_rank",
            "recall_at_k_micros",
            "mrr_micros",
            "ndcg_at_k_micros",
            "citation",
            "policy",
            "confidence_mismatch_count",
            "temporal_mismatch_count",
            "stale_citation_query_count",
            "stale_projection_query_count",
            "unverified_projection_query_count",
            "query_metrics_digest",
        ],
        "GKX_EVAL_QUERY_METRICS_FIELDS_INVALID",
    )?;
    if metrics.get("contract_version") != Some(&Value::String(QUERY_METRICS_VERSION.to_owned()))
        || !metrics
            .get("query_id")
            .and_then(Value::as_str)
            .is_some_and(|id| bounded_id(id, 128))
    {
        return code("GKX_EVAL_QUERY_METRICS_COORDINATE_INVALID");
    }
    let top_k = count(
        field(
            metrics,
            "expected_top_k",
            "GKX_EVAL_QUERY_METRICS_TOP_K_INVALID",
        )?,
        100,
        "GKX_EVAL_QUERY_METRICS_TOP_K_INVALID",
    )?;
    if top_k == 0 {
        return code("GKX_EVAL_QUERY_METRICS_TOP_K_INVALID");
    }
    let relevant_count = count(
        field(
            metrics,
            "relevant_source_count",
            "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
        )?,
        512,
        "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
    )?;
    let returned_count = count(
        field(
            metrics,
            "returned_unique_source_count",
            "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
        )?,
        100,
        "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
    )?;
    let relevant_returned = count(
        field(
            metrics,
            "relevant_returned_source_count",
            "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
        )?,
        100,
        "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
    )?;
    if relevant_count == 0
        || returned_count > top_k
        || relevant_returned > relevant_count
        || relevant_returned > returned_count
    {
        return code("GKX_EVAL_QUERY_METRICS_RELATION_INVALID");
    }
    let ranks = array(
        field(
            metrics,
            "relevant_source_ranks",
            "GKX_EVAL_RELEVANT_RANKS_INVALID",
        )?,
        "GKX_EVAL_RELEVANT_RANKS_INVALID",
    )?
    .iter()
    .map(|rank| count(rank, top_k, "GKX_EVAL_RELEVANT_RANKS_INVALID"))
    .collect::<EvalResult<Vec<_>>>()?;
    if ranks.len() != relevant_returned as usize
        || ranks.first() == Some(&0)
        || ranks.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return code("GKX_EVAL_RELEVANT_RANKS_INVALID");
    }
    let expected_first = ranks.first().copied().map_or(Value::Null, Value::from);
    if metrics.get("first_relevant_rank") != Some(&expected_first)
        || metrics.get("recall_at_k_micros")
            != Some(&json!(round_ratio(
                u128::from(relevant_returned),
                u128::from(relevant_count),
                METRIC_SCALE
            )?))
        || metrics.get("mrr_micros")
            != Some(&json!(ranks.first().map_or(Ok(0), |rank| round_ratio(
                1,
                u128::from(*rank),
                METRIC_SCALE
            ))?))
        || metrics.get("ndcg_at_k_micros")
            != Some(&json!(ndcg_micros(&ranks, relevant_count, top_k)?))
    {
        return code("GKX_EVAL_QUERY_METRICS_RELATION_INVALID");
    }
    seal_citation_metrics(field(
        metrics,
        "citation",
        "GKX_EVAL_QUERY_METRICS_INVALID",
    )?)?;
    seal_policy_metrics(field(metrics, "policy", "GKX_EVAL_QUERY_METRICS_INVALID")?)?;
    let policy = object(
        field(metrics, "policy", "GKX_EVAL_QUERY_METRICS_INVALID")?,
        "GKX_EVAL_QUERY_METRICS_INVALID",
    )?;
    let policy_field_count = policy
        .get("policy_identity_field_count")
        .and_then(Value::as_u64)
        .ok_or(EvalError("GKX_EVAL_QUERY_POLICY_FIELD_COUNT_INVALID"))?;
    if policy_field_count < 2 + 6 * returned_count {
        return code("GKX_EVAL_QUERY_POLICY_FIELD_COUNT_INVALID");
    }
    let citation = object(
        field(metrics, "citation", "GKX_EVAL_QUERY_METRICS_INVALID")?,
        "GKX_EVAL_QUERY_METRICS_INVALID",
    )?;
    let stale = citation
        .get("stale")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if metrics.get("stale_citation_query_count") != Some(&json!(u64::from(stale > 0))) {
        return code("GKX_EVAL_STALE_CITATION_QUERY_COUNT_MISMATCH");
    }
    for key in [
        "confidence_mismatch_count",
        "temporal_mismatch_count",
        "stale_citation_query_count",
        "stale_projection_query_count",
        "unverified_projection_query_count",
    ] {
        count(
            field(metrics, key, "GKX_EVAL_QUERY_METRICS_RELATION_INVALID")?,
            1,
            "GKX_EVAL_QUERY_METRICS_RELATION_INVALID",
        )?;
    }
    let checked = citation
        .get("checked")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if returned_count == 0 {
        if citation.get("applicability").and_then(Value::as_str) != Some("not_applicable")
            || checked != 0
        {
            return code("GKX_EVAL_QUERY_CITATION_APPLICABILITY_RELATION_INVALID");
        }
    } else if citation.get("applicability").and_then(Value::as_str) != Some("required")
        || checked == 0
        || checked > top_k * 2
    {
        return code("GKX_EVAL_QUERY_CITATION_APPLICABILITY_RELATION_INVALID");
    }
    verify_digest(
        value,
        "query_metrics_digest",
        "GKX_EVAL_QUERY_METRICS_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn aggregate_query_metrics(values: &[Value]) -> EvalResult<Value> {
    if values.is_empty() || values.len() > 256 {
        return code("GKX_EVAL_AGGREGATE_QUERY_COUNT_INVALID");
    }
    let values = values
        .iter()
        .map(seal_query_metrics)
        .collect::<EvalResult<Vec<_>>>()?;
    let mut query_ids = HashSet::new();
    for value in &values {
        let id = value
            .get("query_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !query_ids.insert(id) {
            return code("GKX_EVAL_AGGREGATE_QUERY_DUPLICATE");
        }
    }
    let sum = |key: &str| -> u128 {
        values
            .iter()
            .map(|value| u128::from(value.get(key).and_then(Value::as_u64).unwrap_or_default()))
            .sum()
    };
    let nested_sum = |section: &str, key: &str| -> u128 {
        values
            .iter()
            .map(|value| {
                u128::from(
                    value
                        .get(section)
                        .and_then(Value::as_object)
                        .and_then(|item| item.get(key))
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                )
            })
            .sum()
    };
    let query_count = values.len() as u128;
    let checked = nested_sum("citation", "checked");
    let passed = nested_sum("citation", "passed");
    let mismatch = nested_sum("citation", "mismatch");
    let stale = nested_sum("citation", "stale");
    let policy_fields = nested_sum("policy", "policy_identity_field_count");
    let policy_leaks = nested_sum("policy", "policy_leak_count");
    if policy_fields > u128::from(JS_MAX_SAFE_INTEGER)
        || policy_leaks > u128::from(JS_MAX_SAFE_INTEGER)
    {
        return code("GKX_EVAL_AGGREGATE_POLICY_COUNT_OVERFLOW");
    }
    let stale_citation_queries = sum("stale_citation_query_count");
    let unverified_queries = sum("unverified_projection_query_count");
    let material = json!({
        "contract_version": AGGREGATE_METRICS_VERSION,
        "query_count": values.len(),
        "recall_at_k_micros": round_integer_ratio(sum("recall_at_k_micros"), query_count)?,
        "mrr_micros": round_integer_ratio(sum("mrr_micros"), query_count)?,
        "ndcg_at_k_micros": round_integer_ratio(sum("ndcg_at_k_micros"), query_count)?,
        "citation": {
            "applicability": if checked == 0 { "not_applicable" } else { "required" },
            "checked": checked,
            "passed": passed,
            "mismatch": mismatch,
            "stale": stale,
            "correctness_micros": if checked == 0 { Value::Null } else { json!(round_ratio(passed, checked, METRIC_SCALE)?) },
        },
        "policy": {
            "policy_identity_field_count": policy_fields,
            "policy_leak_count": policy_leaks,
            "policy_leak_rate_micros": if policy_fields == 0 { 0 } else { round_ratio(policy_leaks, policy_fields, METRIC_SCALE)? },
        },
        "confidence_mismatch_count": sum("confidence_mismatch_count"),
        "temporal_mismatch_count": sum("temporal_mismatch_count"),
        "stale_citation_query_count": stale_citation_queries,
        "stale_citation_query_rate_micros": round_ratio(stale_citation_queries, query_count, METRIC_SCALE)?,
        "stale_projection_query_count": sum("stale_projection_query_count"),
        "unverified_projection_query_count": unverified_queries,
        "unverified_projection_rate_micros": round_ratio(unverified_queries, query_count, METRIC_SCALE)?,
    });
    let mut output = object(&material, "GKX_EVAL_AGGREGATE_INVALID")?.clone();
    output.insert(
        "aggregate_metrics_digest".to_owned(),
        Value::String(
            canonical_digest(&material)
                .map_err(|_| EvalError("GKX_EVAL_AGGREGATE_DIGEST_INVALID"))?,
        ),
    );
    let output = Value::Object(output);
    seal_aggregate_metrics(&output)?;
    Ok(output)
}

fn seal_aggregate_metrics(value: &Value) -> EvalResult<Value> {
    let aggregate = object(value, "GKX_EVAL_AGGREGATE_INVALID")?;
    exact_keys(
        aggregate,
        &[
            "contract_version",
            "query_count",
            "recall_at_k_micros",
            "mrr_micros",
            "ndcg_at_k_micros",
            "citation",
            "policy",
            "confidence_mismatch_count",
            "temporal_mismatch_count",
            "stale_citation_query_count",
            "stale_citation_query_rate_micros",
            "stale_projection_query_count",
            "unverified_projection_query_count",
            "unverified_projection_rate_micros",
            "aggregate_metrics_digest",
        ],
        "GKX_EVAL_AGGREGATE_FIELDS_INVALID",
    )?;
    if aggregate.get("contract_version")
        != Some(&Value::String(AGGREGATE_METRICS_VERSION.to_owned()))
    {
        return code("GKX_EVAL_AGGREGATE_COORDINATE_INVALID");
    }
    let query_count = count(
        field(
            aggregate,
            "query_count",
            "GKX_EVAL_AGGREGATE_QUERY_COUNT_INVALID",
        )?,
        256,
        "GKX_EVAL_AGGREGATE_QUERY_COUNT_INVALID",
    )?;
    if query_count == 0 {
        return code("GKX_EVAL_AGGREGATE_QUERY_COUNT_INVALID");
    }
    for key in ["recall_at_k_micros", "mrr_micros", "ndcg_at_k_micros"] {
        count(
            field(aggregate, key, "GKX_EVAL_AGGREGATE_METRIC_INVALID")?,
            1_000_000,
            "GKX_EVAL_AGGREGATE_METRIC_INVALID",
        )?;
    }
    for key in [
        "confidence_mismatch_count",
        "temporal_mismatch_count",
        "stale_citation_query_count",
        "stale_projection_query_count",
        "unverified_projection_query_count",
    ] {
        count(
            field(
                aggregate,
                key,
                "GKX_EVAL_AGGREGATE_QUERY_FAILURE_COUNT_INVALID",
            )?,
            query_count,
            "GKX_EVAL_AGGREGATE_QUERY_FAILURE_COUNT_INVALID",
        )?;
    }
    let stale_count = aggregate
        .get("stale_citation_query_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let unverified_count = aggregate
        .get("unverified_projection_query_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if aggregate.get("stale_citation_query_rate_micros")
        != Some(&json!(round_ratio(
            u128::from(stale_count),
            u128::from(query_count),
            METRIC_SCALE
        )?))
        || aggregate.get("unverified_projection_rate_micros")
            != Some(&json!(round_ratio(
                u128::from(unverified_count),
                u128::from(query_count),
                METRIC_SCALE
            )?))
    {
        return code("GKX_EVAL_AGGREGATE_QUERY_RATE_MISMATCH");
    }
    seal_citation_metrics(field(aggregate, "citation", "GKX_EVAL_AGGREGATE_INVALID")?)?;
    seal_policy_metrics(field(aggregate, "policy", "GKX_EVAL_AGGREGATE_INVALID")?)?;
    verify_digest(
        value,
        "aggregate_metrics_digest",
        "GKX_EVAL_AGGREGATE_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn compare_ndcg(baseline: u64, current: u64) -> &'static str {
    if u128::from(current) * 100 >= u128::from(baseline) * 98 {
        "pass"
    } else {
        "regression"
    }
}

fn valid_opaque_identity(value: &str) -> bool {
    let utf16_count = value.encode_utf16().count();
    (1..=512).contains(&utf16_count)
        && !effective_query_text(value).is_empty()
        && !value
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
        && canonical_json(&value).is_ok()
}

fn valid_observation_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn seal_provider_role(value: &Value, embedding: bool) -> EvalResult<()> {
    let role = object(value, "GKX_EVAL_PROVIDER_ROLE_INVALID")?;
    let embedding_fields = [
        "state",
        "provider_scenario_id",
        "provider_kind",
        "provider_id",
        "model_id",
        "dimensions",
        "fixed_provider_transcript_digest",
    ];
    let reranker_fields = [
        "state",
        "provider_scenario_id",
        "provider_kind",
        "provider_id",
        "model_id",
        "fixed_provider_transcript_digest",
    ];
    exact_keys(
        role,
        if embedding {
            &embedding_fields
        } else {
            &reranker_fields
        },
        "GKX_EVAL_PROVIDER_ROLE_FIELDS_INVALID",
    )?;
    match role.get("state").and_then(Value::as_str) {
        Some("disabled") => {
            if role.get("provider_scenario_id").and_then(Value::as_str) != Some("disabled")
                || [
                    "provider_kind",
                    "provider_id",
                    "model_id",
                    "fixed_provider_transcript_digest",
                ]
                .iter()
                .any(|key| !role.get(*key).is_some_and(Value::is_null))
                || (embedding && !role.get("dimensions").is_some_and(Value::is_null))
            {
                return code("GKX_EVAL_PROVIDER_ROLE_DISABLED_INVALID");
            }
        }
        Some("active") => {
            let scenario = role
                .get("provider_scenario_id")
                .and_then(Value::as_str)
                .ok_or(EvalError("GKX_EVAL_PROVIDER_ROLE_SCENARIO_INVALID"))?;
            if scenario == "disabled" || !bounded_id(scenario, 128) {
                return code("GKX_EVAL_PROVIDER_ROLE_SCENARIO_INVALID");
            }
            if !matches!(
                role.get("provider_kind").and_then(Value::as_str),
                Some("openai_compatible" | "local_onnx" | "mcp")
            ) || ["provider_id", "model_id"].iter().any(|key| {
                !role
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(valid_opaque_identity)
            }) || !role
                .get("fixed_provider_transcript_digest")
                .and_then(Value::as_str)
                .is_some_and(is_digest)
            {
                return code("GKX_EVAL_PROVIDER_ROLE_IDENTITY_INVALID");
            }
            if embedding
                && !role
                    .get("dimensions")
                    .and_then(Value::as_u64)
                    .is_some_and(|value| (1..=4096).contains(&value))
            {
                return code("GKX_EVAL_EMBEDDING_ROLE_DIMENSIONS_INVALID");
            }
        }
        _ => return code("GKX_EVAL_PROVIDER_ROLE_STATE_INVALID"),
    }
    Ok(())
}

fn seal_environment(value: &Value) -> EvalResult<Value> {
    let environment = object(value, "GKX_EVAL_ENVIRONMENT_INVALID")?;
    exact_keys(
        environment,
        &[
            "contract_version",
            "scenario_id",
            "vault_fixture",
            "retrieval_contract_version",
            "evaluation_contract_version",
            "golden_contract_version",
            "metric_contract_version",
            "engine_version",
            "gkx_standard_commit",
            "gkx_projection_profile",
            "projection_schema_version",
            "chunker_version",
            "tokenizer_version",
            "lexical_backend",
            "normalized_golden_digest",
            "fixture_catalog_digest",
            "corpus_fixture_digest",
            "source_snapshot_digest",
            "runtime_policy_inputs_digest",
            "evaluation_audit_oracle_digest",
            "projection_id",
            "projection_digest",
            "embedding_role",
            "reranker_role",
            "ndcg_discount_table_digest",
            "metric_scale",
            "environment_digest",
        ],
        "GKX_EVAL_ENVIRONMENT_FIELDS_INVALID",
    )?;
    let exact = [
        ("contract_version", ENVIRONMENT_VERSION),
        ("retrieval_contract_version", "gkos-retrieval/1.0.0-draft.2"),
        ("evaluation_contract_version", EVALUATION_VERSION),
        ("golden_contract_version", GOLDEN_VERSION),
        ("metric_contract_version", METRIC_VERSION),
        ("engine_version", "2.1.2"),
        (
            "gkx_standard_commit",
            "a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6",
        ),
        ("gkx_projection_profile", "gkx-2.3-validating-projection"),
        ("chunker_version", "gkos-heading-chunker/1"),
        ("tokenizer_version", "gkos-ascii-whitespace/1"),
        ("ndcg_discount_table_digest", NDCG_TABLE_DIGEST),
    ];
    if exact
        .iter()
        .any(|(key, expected)| environment.get(*key).and_then(Value::as_str) != Some(*expected))
        || environment
            .get("projection_schema_version")
            .and_then(Value::as_u64)
            != Some(3)
        || environment.get("metric_scale").and_then(Value::as_u64) != Some(1_000_000)
        || !matches!(
            environment.get("lexical_backend").and_then(Value::as_str),
            Some("sqlite_fts5" | "sqlite_lexical_scan")
        )
        || !environment
            .get("vault_fixture")
            .and_then(Value::as_str)
            .is_some_and(|value| bounded_id(value, 128))
    {
        return code("GKX_EVAL_ENVIRONMENT_COORDINATE_INVALID");
    }
    for key in [
        "normalized_golden_digest",
        "fixture_catalog_digest",
        "corpus_fixture_digest",
        "source_snapshot_digest",
        "runtime_policy_inputs_digest",
        "evaluation_audit_oracle_digest",
        "projection_digest",
        "environment_digest",
    ] {
        if !environment
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(is_digest)
        {
            return code("GKX_EVAL_ENVIRONMENT_DIGEST_INVALID");
        }
    }
    let projection_digest = environment
        .get("projection_digest")
        .and_then(Value::as_str)
        .unwrap();
    if environment.get("projection_id").and_then(Value::as_str)
        != Some(format!("retrieval:{}", &projection_digest[7..31]).as_str())
    {
        return code("GKX_EVAL_ENVIRONMENT_PROJECTION_BINDING_INVALID");
    }
    seal_provider_role(
        field(
            environment,
            "embedding_role",
            "GKX_EVAL_ENVIRONMENT_INVALID",
        )?,
        true,
    )?;
    seal_provider_role(
        field(environment, "reranker_role", "GKX_EVAL_ENVIRONMENT_INVALID")?,
        false,
    )?;
    let embedding_scenario = environment
        .get("embedding_role")
        .and_then(Value::as_object)
        .and_then(|role| role.get("provider_scenario_id"))
        .and_then(Value::as_str)
        .unwrap();
    let reranker_scenario = environment
        .get("reranker_role")
        .and_then(Value::as_object)
        .and_then(|role| role.get("provider_scenario_id"))
        .and_then(Value::as_str)
        .unwrap();
    let backend =
        if environment.get("lexical_backend").and_then(Value::as_str) == Some("sqlite_fts5") {
            "sqlite-fts5"
        } else {
            "sqlite-lexical-scan"
        };
    let expected_scenario = format!(
        "{}--{}--vector-{}--reranker-{}",
        environment
            .get("vault_fixture")
            .and_then(Value::as_str)
            .unwrap(),
        backend,
        embedding_scenario,
        reranker_scenario
    );
    if expected_scenario.len() > 512
        || environment.get("scenario_id").and_then(Value::as_str) != Some(&expected_scenario)
    {
        return code("GKX_EVAL_ENVIRONMENT_SCENARIO_INVALID");
    }
    verify_digest(
        value,
        "environment_digest",
        "GKX_EVAL_ENVIRONMENT_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn seal_environment_set(value: &Value, golden_value: &Value) -> EvalResult<Value> {
    seal_normalized_golden(golden_value)?;
    let golden = object(golden_value, "GKX_EVAL_GOLDEN_INVALID")?;
    let golden_queries = array(
        field(golden, "queries", "GKX_EVAL_GOLDEN_INVALID")?,
        "GKX_EVAL_GOLDEN_INVALID",
    )?;
    let set = object(value, "GKX_EVAL_ENVIRONMENT_SET_INVALID")?;
    exact_keys(
        set,
        &[
            "contract_version",
            "normalized_golden_digest",
            "query_count",
            "members",
            "environment_set_digest",
        ],
        "GKX_EVAL_ENVIRONMENT_SET_FIELDS_INVALID",
    )?;
    let members = array(
        field(set, "members", "GKX_EVAL_ENVIRONMENT_SET_MEMBERS_INVALID")?,
        "GKX_EVAL_ENVIRONMENT_SET_MEMBERS_INVALID",
    )?;
    if set.get("contract_version").and_then(Value::as_str) != Some(ENVIRONMENT_SET_VERSION)
        || set.get("normalized_golden_digest") != golden.get("golden_digest")
        || set.get("query_count").and_then(Value::as_u64) != Some(golden_queries.len() as u64)
        || members.is_empty()
        || members.len() > 256
    {
        return code("GKX_EVAL_ENVIRONMENT_SET_COORDINATE_INVALID");
    }
    let query_by_id = golden_queries
        .iter()
        .map(|query| {
            let record = query.as_object().unwrap();
            (record.get("id").and_then(Value::as_str).unwrap(), record)
        })
        .collect::<HashMap<_, _>>();
    let mut seen_queries = HashSet::new();
    let mut prior_environment: Option<&str> = None;
    let mut seen_scenarios = HashSet::new();
    let mut seen_vaults = HashSet::new();
    for member_value in members {
        let member = object(member_value, "GKX_EVAL_ENVIRONMENT_SET_MEMBER_INVALID")?;
        exact_keys(
            member,
            &[
                "environment",
                "query_partition",
                "query_count",
                "member_digest",
            ],
            "GKX_EVAL_ENVIRONMENT_SET_MEMBER_FIELDS_INVALID",
        )?;
        let environment_value = field(
            member,
            "environment",
            "GKX_EVAL_ENVIRONMENT_SET_MEMBER_INVALID",
        )?;
        seal_environment(environment_value)?;
        let environment = object(environment_value, "GKX_EVAL_ENVIRONMENT_INVALID")?;
        let partition = array(
            field(
                member,
                "query_partition",
                "GKX_EVAL_ENVIRONMENT_SET_QUERY_PARTITION_INVALID",
            )?,
            "GKX_EVAL_ENVIRONMENT_SET_QUERY_PARTITION_INVALID",
        )?;
        if partition.is_empty()
            || partition.len() > 256
            || member.get("query_count").and_then(Value::as_u64) != Some(partition.len() as u64)
            || environment.get("normalized_golden_digest") != golden.get("golden_digest")
        {
            return code("GKX_EVAL_ENVIRONMENT_SET_MEMBER_COORDINATE_INVALID");
        }
        let vault = environment
            .get("vault_fixture")
            .and_then(Value::as_str)
            .unwrap();
        let expected_partition = golden_queries
            .iter()
            .filter(|query| query.get("vault_fixture").and_then(Value::as_str) == Some(vault))
            .map(|query| {
                json!({
                    "query_id": query.get("id").unwrap(),
                    "query_digest": query.get("query_digest").unwrap(),
                })
            })
            .collect::<Vec<_>>();
        if *partition != expected_partition {
            return code("GKX_EVAL_ENVIRONMENT_SET_QUERY_ORDER_INVALID");
        }
        for row in partition {
            let row = object(row, "GKX_EVAL_ENVIRONMENT_SET_QUERY_FIELDS_INVALID")?;
            exact_keys(
                row,
                &["query_id", "query_digest"],
                "GKX_EVAL_ENVIRONMENT_SET_QUERY_FIELDS_INVALID",
            )?;
            let id = row
                .get("query_id")
                .and_then(Value::as_str)
                .ok_or(EvalError("GKX_EVAL_ENVIRONMENT_SET_QUERY_BINDING_INVALID"))?;
            let query = query_by_id
                .get(id)
                .ok_or(EvalError("GKX_EVAL_ENVIRONMENT_SET_QUERY_BINDING_INVALID"))?;
            if row.get("query_digest") != query.get("query_digest") || !seen_queries.insert(id) {
                return code("GKX_EVAL_ENVIRONMENT_SET_QUERY_BINDING_INVALID");
            }
        }
        let digest = environment
            .get("environment_digest")
            .and_then(Value::as_str)
            .unwrap();
        let scenario = environment
            .get("scenario_id")
            .and_then(Value::as_str)
            .unwrap();
        if prior_environment.is_some_and(|prior| utf16_cmp(prior, digest) != Ordering::Less)
            || !seen_scenarios.insert(scenario)
            || !seen_vaults.insert(vault)
        {
            return code("GKX_EVAL_ENVIRONMENT_SET_PARTITION_INVALID");
        }
        prior_environment = Some(digest);
        verify_digest(
            member_value,
            "member_digest",
            "GKX_EVAL_ENVIRONMENT_SET_MEMBER_DIGEST_MISMATCH",
        )?;
    }
    if seen_queries.len() != golden_queries.len() {
        return code("GKX_EVAL_ENVIRONMENT_SET_PARTITION_INVALID");
    }
    verify_digest(
        value,
        "environment_set_digest",
        "GKX_EVAL_ENVIRONMENT_SET_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn query_metrics_set_digest(environment_set_digest: &str, rows: &[Value]) -> EvalResult<String> {
    let query_evaluations = rows
        .iter()
        .map(|row| {
            let row = object(row, "GKX_EVAL_METRICS_SET_QUERY_INVALID")?;
            let metrics = object(field(row, "query_metrics", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?, "GKX_EVAL_METRICS_SET_QUERY_INVALID")?;
            Ok(json!({
                "environment_digest": field(row, "environment_digest", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?,
                "golden_query_digest": field(row, "golden_query_digest", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?,
                "query_metrics_digest": field(metrics, "query_metrics_digest", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?,
            }))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    canonical_digest(&json!({
        "contract_version": QUERY_METRICS_SET_VERSION,
        "environment_set_digest": environment_set_digest,
        "query_count": rows.len(),
        "query_evaluations": query_evaluations,
    }))
    .map_err(|_| EvalError("GKX_EVAL_METRICS_SET_QUERY_SET_DIGEST_INVALID"))
}

fn scoped_query_metrics_set_digest(
    environment_digest: &str,
    rows: &[&Value],
) -> EvalResult<String> {
    let query_evaluations = rows
        .iter()
        .map(|row| {
            let row = object(row, "GKX_EVAL_METRICS_SET_QUERY_INVALID")?;
            let metrics = object(field(row, "query_metrics", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?, "GKX_EVAL_METRICS_SET_QUERY_INVALID")?;
            Ok(json!({
                "golden_query_digest": field(row, "golden_query_digest", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?,
                "query_metrics_digest": field(metrics, "query_metrics_digest", "GKX_EVAL_METRICS_SET_QUERY_INVALID")?,
            }))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    canonical_digest(&json!({
        "contract_version": QUERY_METRICS_SET_VERSION,
        "environment_digest": environment_digest,
        "query_count": rows.len(),
        "query_evaluations": query_evaluations,
    }))
    .map_err(|_| EvalError("GKX_EVAL_METRICS_SET_QUERY_SET_DIGEST_INVALID"))
}

fn seal_metrics_set(
    value: &Value,
    environment_set_value: &Value,
    golden_value: &Value,
) -> EvalResult<Value> {
    seal_environment_set(environment_set_value, golden_value)?;
    let set = object(environment_set_value, "GKX_EVAL_ENVIRONMENT_SET_INVALID")?;
    let golden = object(golden_value, "GKX_EVAL_GOLDEN_INVALID")?;
    let golden_queries = array(
        field(golden, "queries", "GKX_EVAL_GOLDEN_INVALID")?,
        "GKX_EVAL_GOLDEN_INVALID",
    )?;
    let metrics_set = object(value, "GKX_EVAL_METRICS_SET_INVALID")?;
    exact_keys(
        metrics_set,
        &[
            "contract_version",
            "environment_set_digest",
            "normalized_golden_digest",
            "query_count",
            "query_evaluations",
            "query_metrics_set_digest",
            "environment_aggregates",
            "aggregate_metrics",
            "metrics_set_digest",
        ],
        "GKX_EVAL_METRICS_SET_FIELDS_INVALID",
    )?;
    let rows = array(
        field(
            metrics_set,
            "query_evaluations",
            "GKX_EVAL_METRICS_SET_QUERY_EVALUATIONS_INVALID",
        )?,
        "GKX_EVAL_METRICS_SET_QUERY_EVALUATIONS_INVALID",
    )?;
    if metrics_set.get("contract_version").and_then(Value::as_str) != Some(METRICS_SET_VERSION)
        || metrics_set.get("environment_set_digest") != set.get("environment_set_digest")
        || metrics_set.get("normalized_golden_digest") != golden.get("golden_digest")
        || metrics_set.get("query_count").and_then(Value::as_u64)
            != Some(golden_queries.len() as u64)
        || rows.len() != golden_queries.len()
    {
        return code("GKX_EVAL_METRICS_SET_COORDINATE_INVALID");
    }
    let members = array(
        field(set, "members", "GKX_EVAL_ENVIRONMENT_SET_INVALID")?,
        "GKX_EVAL_ENVIRONMENT_SET_INVALID",
    )?;
    let environment_by_query = members
        .iter()
        .flat_map(|member| {
            let member = member.as_object().unwrap();
            let environment_digest = member
                .get("environment")
                .and_then(Value::as_object)
                .and_then(|environment| environment.get("environment_digest"))
                .and_then(Value::as_str)
                .unwrap();
            member
                .get("query_partition")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(move |row| {
                    (
                        row.get("query_id").and_then(Value::as_str).unwrap(),
                        environment_digest,
                    )
                })
        })
        .collect::<HashMap<_, _>>();
    for (index, row_value) in rows.iter().enumerate() {
        let row = object(row_value, "GKX_EVAL_METRICS_SET_QUERY_INVALID")?;
        exact_keys(
            row,
            &["environment_digest", "golden_query_digest", "query_metrics"],
            "GKX_EVAL_METRICS_SET_QUERY_FIELDS_INVALID",
        )?;
        let metrics = seal_query_metrics(field(
            row,
            "query_metrics",
            "GKX_EVAL_METRICS_SET_QUERY_INVALID",
        )?)?;
        let query = object(&golden_queries[index], "GKX_EVAL_QUERY_INVALID")?;
        let query_id = query.get("id").and_then(Value::as_str).unwrap();
        if row.get("environment_digest").and_then(Value::as_str)
            != environment_by_query.get(query_id).copied()
            || row.get("golden_query_digest") != query.get("query_digest")
            || metrics.get("query_id") != query.get("id")
            || metrics.get("expected_top_k") != query.get("expected_top_k")
        {
            return code("GKX_EVAL_METRICS_SET_QUERY_BINDING_INVALID");
        }
    }
    let environment_set_digest = set
        .get("environment_set_digest")
        .and_then(Value::as_str)
        .unwrap();
    if metrics_set
        .get("query_metrics_set_digest")
        .and_then(Value::as_str)
        != Some(query_metrics_set_digest(environment_set_digest, rows)?.as_str())
    {
        return code("GKX_EVAL_METRICS_SET_QUERY_SET_DIGEST_MISMATCH");
    }
    let environment_aggregates = array(
        field(
            metrics_set,
            "environment_aggregates",
            "GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATES_INVALID",
        )?,
        "GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATES_INVALID",
    )?;
    if environment_aggregates.len() != members.len() {
        return code("GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATES_INVALID");
    }
    for (index, aggregate_value) in environment_aggregates.iter().enumerate() {
        let entry = object(
            aggregate_value,
            "GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATE_INVALID",
        )?;
        exact_keys(
            entry,
            &[
                "environment_digest",
                "query_count",
                "query_metrics_set_digest",
                "aggregate_metrics",
                "environment_aggregate_digest",
            ],
            "GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATE_FIELDS_INVALID",
        )?;
        let environment_digest = members[index]
            .get("environment")
            .and_then(Value::as_object)
            .and_then(|environment| environment.get("environment_digest"))
            .and_then(Value::as_str)
            .unwrap();
        let member_rows = rows
            .iter()
            .filter(|row| {
                row.get("environment_digest").and_then(Value::as_str) == Some(environment_digest)
            })
            .collect::<Vec<_>>();
        if entry.get("environment_digest").and_then(Value::as_str) != Some(environment_digest)
            || entry.get("query_count").and_then(Value::as_u64) != Some(member_rows.len() as u64)
            || entry
                .get("query_metrics_set_digest")
                .and_then(Value::as_str)
                != Some(scoped_query_metrics_set_digest(environment_digest, &member_rows)?.as_str())
        {
            return code("GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATE_COORDINATE_INVALID");
        }
        let expected = aggregate_query_metrics(
            &member_rows
                .iter()
                .map(|row| row.get("query_metrics").unwrap().clone())
                .collect::<Vec<_>>(),
        )?;
        if entry.get("aggregate_metrics") != Some(&expected) {
            return code("GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATE_MISMATCH");
        }
        verify_digest(
            aggregate_value,
            "environment_aggregate_digest",
            "GKX_EVAL_METRICS_SET_ENVIRONMENT_AGGREGATE_DIGEST_MISMATCH",
        )?;
    }
    let expected = aggregate_query_metrics(
        &rows
            .iter()
            .map(|row| row.get("query_metrics").unwrap().clone())
            .collect::<Vec<_>>(),
    )?;
    if metrics_set.get("aggregate_metrics") != Some(&expected) {
        return code("GKX_EVAL_METRICS_SET_AGGREGATE_MISMATCH");
    }
    verify_digest(
        value,
        "metrics_set_digest",
        "GKX_EVAL_METRICS_SET_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn seal_base_configuration(value: &Value) -> EvalResult<Value> {
    let base = object(value, "GKX_EVAL_BASE_CONFIGURATION_INVALID")?;
    exact_keys(
        base,
        &[
            "contract_version",
            "effective_non_tunable_configuration_digest",
            "base_configuration_digest",
        ],
        "GKX_EVAL_BASE_CONFIGURATION_FIELDS_INVALID",
    )?;
    if base.get("contract_version").and_then(Value::as_str) != Some(BASE_CONFIGURATION_VERSION)
        || !base
            .get("effective_non_tunable_configuration_digest")
            .and_then(Value::as_str)
            .is_some_and(is_digest)
    {
        return code("GKX_EVAL_BASE_CONFIGURATION_COORDINATE_INVALID");
    }
    verify_digest(
        value,
        "base_configuration_digest",
        "GKX_EVAL_BASE_CONFIGURATION_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn tuning_grid_material() -> Value {
    json!({
        "contract_version": TUNING_GRID_VERSION,
        "rrf_k": [5, 10, 20, 30, 60, 100],
        "mmr": [
            {"enabled": false, "lambda_micros": null},
            {"enabled": true, "lambda_micros": 0},
            {"enabled": true, "lambda_micros": 300000},
            {"enabled": true, "lambda_micros": 500000},
            {"enabled": true, "lambda_micros": 700000},
            {"enabled": true, "lambda_micros": 1000000}
        ],
        "semantic_top_k": [5, 10, 20, 40, 80],
        "lexical_top_k": [5, 10, 20, 40, 80],
        "candidate_count": 900,
    })
}

fn seal_tuning_grid(value: &Value) -> EvalResult<Value> {
    let grid = object(value, "GKX_EVAL_TUNING_GRID_INVALID")?;
    exact_keys(
        grid,
        &[
            "contract_version",
            "rrf_k",
            "mmr",
            "semantic_top_k",
            "lexical_top_k",
            "candidate_count",
            "tuning_grid_digest",
        ],
        "GKX_EVAL_TUNING_GRID_FIELDS_INVALID",
    )?;
    let mut expected = object(&tuning_grid_material(), "GKX_EVAL_TUNING_GRID_INVALID")?.clone();
    let digest = canonical_digest(&Value::Object(expected.clone()))
        .map_err(|_| EvalError("GKX_EVAL_TUNING_GRID_INVALID"))?;
    expected.insert("tuning_grid_digest".to_owned(), Value::String(digest));
    if *value != Value::Object(expected) {
        return code("GKX_EVAL_TUNING_GRID_MISMATCH");
    }
    Ok(value.clone())
}

fn seal_axes_coordinate(value: &Value) -> EvalResult<Value> {
    let coordinate = object(value, "GKX_EVAL_TUNING_AXES_INVALID")?;
    exact_keys(
        coordinate,
        &[
            "contract_version",
            "rrf_k",
            "mmr",
            "mmr_lambda_micros",
            "semantic_top_k",
            "lexical_top_k",
            "tuning_axes_digest",
        ],
        "GKX_EVAL_TUNING_AXES_COORDINATE_FIELDS_INVALID",
    )?;
    if coordinate.get("contract_version").and_then(Value::as_str) != Some(TUNING_AXES_VERSION) {
        return code("GKX_EVAL_TUNING_AXES_COORDINATE_INVALID");
    }
    let mut raw = coordinate.clone();
    raw.remove("contract_version");
    raw.remove("tuning_axes_digest");
    seal_axes(&Value::Object(raw))?;
    verify_digest(
        value,
        "tuning_axes_digest",
        "GKX_EVAL_TUNING_AXES_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn candidate_config_from_coordinate(value: &Value) -> EvalResult<Value> {
    seal_axes_coordinate(value)?;
    let mut raw = object(value, "GKX_EVAL_TUNING_AXES_INVALID")?.clone();
    raw.remove("contract_version");
    raw.remove("tuning_axes_digest");
    Ok(candidate_config(&seal_axes(&Value::Object(raw))?))
}

#[allow(clippy::too_many_arguments)]
fn evaluation_coordinate_digest(
    environment_set_digest: &Value,
    golden_digest: &Value,
    base_digest: &Value,
    grid_digest: &Value,
    axes_digest: &Value,
    candidate_config_digest: &Value,
    metrics_set: &Map<String, Value>,
    budget: &Value,
    query_count: u64,
    maximum_top_k: u64,
) -> EvalResult<String> {
    let aggregate = object(
        field(
            metrics_set,
            "aggregate_metrics",
            "GKX_EVAL_METRICS_SET_INVALID",
        )?,
        "GKX_EVAL_METRICS_SET_INVALID",
    )?;
    canonical_digest(&json!({
        "contract_version": EVALUATION_COORDINATE_VERSION,
        "environment_set_digest": environment_set_digest,
        "normalized_golden_digest": golden_digest,
        "base_configuration_digest": base_digest,
        "tuning_grid_digest": grid_digest,
        "tuning_axes_digest": axes_digest,
        "candidate_config_digest": candidate_config_digest,
        "query_metrics_set_digest": field(metrics_set, "query_metrics_set_digest", "GKX_EVAL_METRICS_SET_INVALID")?,
        "aggregate_metrics_digest": field(aggregate, "aggregate_metrics_digest", "GKX_EVAL_AGGREGATE_INVALID")?,
        "metrics_set_digest": field(metrics_set, "metrics_set_digest", "GKX_EVAL_METRICS_SET_INVALID")?,
        "relative_ndcg_budget": budget,
        "metric_contract_version": METRIC_VERSION,
        "ndcg_discount_table_digest": NDCG_TABLE_DIGEST,
        "metric_scale": 1_000_000,
        "query_count": query_count,
        "maximum_expected_top_k": maximum_top_k,
    }))
    .map_err(|_| EvalError("GKX_EVAL_BASELINE_EVALUATION_DIGEST_INVALID"))
}

fn seal_budget(value: &Value) -> EvalResult<()> {
    let budget = object(value, "GKX_EVAL_RELATIVE_NDCG_BUDGET_INVALID")?;
    exact_keys(
        budget,
        &["numerator", "denominator"],
        "GKX_EVAL_RELATIVE_NDCG_BUDGET_FIELDS_INVALID",
    )?;
    let numerator = count(
        field(
            budget,
            "numerator",
            "GKX_EVAL_RELATIVE_NDCG_BUDGET_NUMERATOR_INVALID",
        )?,
        1_000_000,
        "GKX_EVAL_RELATIVE_NDCG_BUDGET_NUMERATOR_INVALID",
    )?;
    let denominator = count(
        field(
            budget,
            "denominator",
            "GKX_EVAL_RELATIVE_NDCG_BUDGET_DENOMINATOR_INVALID",
        )?,
        1_000_000,
        "GKX_EVAL_RELATIVE_NDCG_BUDGET_DENOMINATOR_INVALID",
    )?;
    if denominator == 0 || numerator > denominator {
        return code("GKX_EVAL_RELATIVE_NDCG_BUDGET_RELATION_INVALID");
    }
    Ok(())
}

fn seal_baseline(value: &Value) -> EvalResult<Value> {
    let baseline = object(value, "GKX_EVAL_BASELINE_INVALID")?;
    exact_keys(
        baseline,
        &[
            "contract_version",
            "environment_set",
            "normalized_golden",
            "base_configuration",
            "tuning_grid",
            "selected_axes",
            "candidate_config_digest",
            "metrics_set",
            "relative_ndcg_budget",
            "metric_contract_version",
            "ndcg_discount_table_digest",
            "metric_scale",
            "query_count",
            "maximum_expected_top_k",
            "normalized_golden_digest",
            "environment_set_digest",
            "base_configuration_digest",
            "tuning_grid_digest",
            "tuning_axes_digest",
            "query_metrics_set_digest",
            "aggregate_metrics_digest",
            "metrics_set_digest",
            "baseline_evaluation_digest",
            "baseline_digest",
        ],
        "GKX_EVAL_BASELINE_FIELDS_INVALID",
    )?;
    if baseline.get("contract_version").and_then(Value::as_str) != Some(BASELINE_VERSION)
        || baseline
            .get("metric_contract_version")
            .and_then(Value::as_str)
            != Some(METRIC_VERSION)
        || baseline
            .get("ndcg_discount_table_digest")
            .and_then(Value::as_str)
            != Some(NDCG_TABLE_DIGEST)
        || baseline.get("metric_scale").and_then(Value::as_u64) != Some(1_000_000)
    {
        return code("GKX_EVAL_BASELINE_COORDINATE_INVALID");
    }
    let golden_value = field(baseline, "normalized_golden", "GKX_EVAL_BASELINE_INVALID")?;
    seal_normalized_golden(golden_value)?;
    let golden = object(golden_value, "GKX_EVAL_GOLDEN_INVALID")?;
    let queries = array(
        field(golden, "queries", "GKX_EVAL_GOLDEN_INVALID")?,
        "GKX_EVAL_GOLDEN_INVALID",
    )?;
    let environment_set = field(baseline, "environment_set", "GKX_EVAL_BASELINE_INVALID")?;
    seal_environment_set(environment_set, golden_value)?;
    let base = field(baseline, "base_configuration", "GKX_EVAL_BASELINE_INVALID")?;
    seal_base_configuration(base)?;
    let grid = field(baseline, "tuning_grid", "GKX_EVAL_BASELINE_INVALID")?;
    seal_tuning_grid(grid)?;
    let axes = field(baseline, "selected_axes", "GKX_EVAL_BASELINE_INVALID")?;
    seal_axes_coordinate(axes)?;
    let metrics_value = field(baseline, "metrics_set", "GKX_EVAL_BASELINE_INVALID")?;
    seal_metrics_set(metrics_value, environment_set, golden_value)?;
    let metrics = object(metrics_value, "GKX_EVAL_METRICS_SET_INVALID")?;
    let budget = field(
        baseline,
        "relative_ndcg_budget",
        "GKX_EVAL_BASELINE_INVALID",
    )?;
    seal_budget(budget)?;
    let candidate_config_digest = canonical_digest(&json!({
        "base_configuration_digest": base.get("base_configuration_digest").unwrap(),
        "candidate_config": candidate_config_from_coordinate(axes)?,
    }))
    .map_err(|_| EvalError("GKX_EVAL_BASELINE_CANDIDATE_CONFIG_DIGEST_INVALID"))?;
    if baseline
        .get("candidate_config_digest")
        .and_then(Value::as_str)
        != Some(candidate_config_digest.as_str())
    {
        return code("GKX_EVAL_BASELINE_CANDIDATE_CONFIG_DIGEST_MISMATCH");
    }
    let query_count = queries.len() as u64;
    let maximum_top_k = queries
        .iter()
        .filter_map(|query| query.get("expected_top_k").and_then(Value::as_u64))
        .max()
        .ok_or(EvalError("GKX_EVAL_BASELINE_COORDINATE_INVALID"))?;
    let aggregate = object(
        field(metrics, "aggregate_metrics", "GKX_EVAL_BASELINE_INVALID")?,
        "GKX_EVAL_BASELINE_INVALID",
    )?;
    let repeats = [
        ("query_count", Value::from(query_count)),
        ("maximum_expected_top_k", Value::from(maximum_top_k)),
        (
            "normalized_golden_digest",
            golden.get("golden_digest").unwrap().clone(),
        ),
        (
            "environment_set_digest",
            environment_set
                .get("environment_set_digest")
                .unwrap()
                .clone(),
        ),
        (
            "base_configuration_digest",
            base.get("base_configuration_digest").unwrap().clone(),
        ),
        (
            "tuning_grid_digest",
            grid.get("tuning_grid_digest").unwrap().clone(),
        ),
        (
            "tuning_axes_digest",
            axes.get("tuning_axes_digest").unwrap().clone(),
        ),
        (
            "query_metrics_set_digest",
            metrics.get("query_metrics_set_digest").unwrap().clone(),
        ),
        (
            "aggregate_metrics_digest",
            aggregate.get("aggregate_metrics_digest").unwrap().clone(),
        ),
        (
            "metrics_set_digest",
            metrics.get("metrics_set_digest").unwrap().clone(),
        ),
    ];
    if repeats
        .iter()
        .any(|(key, expected)| baseline.get(*key) != Some(expected))
    {
        return code("GKX_EVAL_BASELINE_REPEATED_COORDINATE_MISMATCH");
    }
    let expected_evaluation = evaluation_coordinate_digest(
        baseline.get("environment_set_digest").unwrap(),
        baseline.get("normalized_golden_digest").unwrap(),
        baseline.get("base_configuration_digest").unwrap(),
        baseline.get("tuning_grid_digest").unwrap(),
        baseline.get("tuning_axes_digest").unwrap(),
        baseline.get("candidate_config_digest").unwrap(),
        metrics,
        budget,
        query_count,
        maximum_top_k,
    )?;
    if baseline
        .get("baseline_evaluation_digest")
        .and_then(Value::as_str)
        != Some(expected_evaluation.as_str())
    {
        return code("GKX_EVAL_BASELINE_EVALUATION_DIGEST_MISMATCH");
    }
    verify_digest(
        value,
        "baseline_digest",
        "GKX_EVAL_BASELINE_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn seal_observation_report(value: &Value) -> EvalResult<Value> {
    let report = object(value, "GKX_EVAL_OBSERVATION_INVALID")?;
    exact_keys(
        report,
        &[
            "contract_version",
            "evaluation_digest",
            "fixed_sample_plan_digest",
            "environment",
            "warmup_count",
            "sample_count",
            "query_latency_micros",
            "index_time_micros",
            "update_time_micros",
            "chunks_reprocessed",
            "chunks_reused",
            "observation_digest",
        ],
        "GKX_EVAL_OBSERVATION_FIELDS_INVALID",
    )?;
    if report.get("contract_version").and_then(Value::as_str) != Some(OBSERVATION_VERSION)
        || !report
            .get("evaluation_digest")
            .and_then(Value::as_str)
            .is_some_and(is_digest)
        || !report
            .get("fixed_sample_plan_digest")
            .and_then(Value::as_str)
            .is_some_and(is_digest)
    {
        return code("GKX_EVAL_OBSERVATION_COORDINATE_INVALID");
    }
    let environment = object(
        field(
            report,
            "environment",
            "GKX_EVAL_OBSERVATION_ENVIRONMENT_INVALID",
        )?,
        "GKX_EVAL_OBSERVATION_ENVIRONMENT_INVALID",
    )?;
    exact_keys(
        environment,
        &[
            "runtime",
            "runtime_version",
            "os",
            "arch",
            "sqlite_version",
            "lexical_backend",
            "fts5_available",
            "runner_class",
        ],
        "GKX_EVAL_OBSERVATION_ENVIRONMENT_FIELDS_INVALID",
    )?;
    let lexical_backend = environment.get("lexical_backend").and_then(Value::as_str);
    if environment.get("runtime").and_then(Value::as_str) != Some("node")
        || !environment
            .get("runtime_version")
            .and_then(Value::as_str)
            .is_some_and(valid_observation_version)
        || !matches!(
            environment.get("os").and_then(Value::as_str),
            Some("linux" | "windows" | "darwin")
        )
        || !matches!(
            environment.get("arch").and_then(Value::as_str),
            Some("x64" | "arm64")
        )
        || !environment
            .get("sqlite_version")
            .and_then(Value::as_str)
            .is_some_and(valid_observation_version)
        || !matches!(lexical_backend, Some("sqlite_fts5" | "sqlite_lexical_scan"))
        || !environment
            .get("fts5_available")
            .is_some_and(Value::is_boolean)
        || !matches!(
            environment.get("runner_class").and_then(Value::as_str),
            Some("github_hosted" | "local")
        )
        || (lexical_backend == Some("sqlite_fts5")
            && environment.get("fts5_available").and_then(Value::as_bool) != Some(true))
    {
        return code("GKX_EVAL_OBSERVATION_ENVIRONMENT_INVALID");
    }
    count(
        field(
            report,
            "warmup_count",
            "GKX_EVAL_OBSERVATION_WARMUP_COUNT_INVALID",
        )?,
        1_000_000,
        "GKX_EVAL_OBSERVATION_WARMUP_COUNT_INVALID",
    )?;
    if count(
        field(
            report,
            "sample_count",
            "GKX_EVAL_OBSERVATION_SAMPLE_COUNT_INVALID",
        )?,
        1_000_000,
        "GKX_EVAL_OBSERVATION_SAMPLE_COUNT_INVALID",
    )? == 0
    {
        return code("GKX_EVAL_OBSERVATION_SAMPLE_COUNT_INVALID");
    }
    let latency = object(
        field(
            report,
            "query_latency_micros",
            "GKX_EVAL_OBSERVATION_LATENCY_INVALID",
        )?,
        "GKX_EVAL_OBSERVATION_LATENCY_INVALID",
    )?;
    exact_keys(
        latency,
        &["p50", "p95", "p99"],
        "GKX_EVAL_OBSERVATION_LATENCY_FIELDS_INVALID",
    )?;
    let p50 = count(
        field(latency, "p50", "GKX_EVAL_OBSERVATION_P50_INVALID")?,
        u64::MAX,
        "GKX_EVAL_OBSERVATION_P50_INVALID",
    )?;
    let p95 = count(
        field(latency, "p95", "GKX_EVAL_OBSERVATION_P95_INVALID")?,
        u64::MAX,
        "GKX_EVAL_OBSERVATION_P95_INVALID",
    )?;
    let p99 = count(
        field(latency, "p99", "GKX_EVAL_OBSERVATION_P99_INVALID")?,
        u64::MAX,
        "GKX_EVAL_OBSERVATION_P99_INVALID",
    )?;
    if p50 > p95 || p95 > p99 {
        return code("GKX_EVAL_OBSERVATION_PERCENTILE_ORDER_INVALID");
    }
    for key in [
        "index_time_micros",
        "update_time_micros",
        "chunks_reprocessed",
        "chunks_reused",
    ] {
        count(
            field(report, key, "GKX_EVAL_OBSERVATION_COUNT_INVALID")?,
            u64::MAX,
            "GKX_EVAL_OBSERVATION_COUNT_INVALID",
        )?;
    }
    verify_digest(
        value,
        "observation_digest",
        "GKX_EVAL_OBSERVATION_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn zero_gate_failures(aggregate: &Map<String, Value>) -> EvalResult<Vec<&'static str>> {
    let mut failures = Vec::new();
    let policy = object(
        field(aggregate, "policy", "GKX_EVAL_AGGREGATE_INVALID")?,
        "GKX_EVAL_AGGREGATE_INVALID",
    )?;
    let citation = object(
        field(aggregate, "citation", "GKX_EVAL_AGGREGATE_INVALID")?,
        "GKX_EVAL_AGGREGATE_INVALID",
    )?;
    if policy.get("policy_leak_count").and_then(Value::as_u64) != Some(0) {
        failures.push("POLICY_LEAK");
    }
    if citation.get("mismatch").and_then(Value::as_u64) != Some(0) {
        failures.push("CITATION_MISMATCH");
    }
    if citation.get("stale").and_then(Value::as_u64) != Some(0) {
        failures.push("STALE_CITATION");
    }
    if citation.get("applicability").and_then(Value::as_str) == Some("required")
        && (citation.get("checked").and_then(Value::as_u64) == Some(0)
            || citation.get("correctness_micros").and_then(Value::as_u64) != Some(1_000_000))
    {
        failures.push("CITATION_COVERAGE");
    }
    for (key, reason) in [
        ("confidence_mismatch_count", "CONFIDENCE_MISMATCH"),
        ("temporal_mismatch_count", "TEMPORAL_MISMATCH"),
        ("stale_projection_query_count", "STALE_PROJECTION"),
        ("unverified_projection_query_count", "UNVERIFIED_PROJECTION"),
    ] {
        if aggregate.get(key).and_then(Value::as_u64) != Some(0) {
            failures.push(reason);
        }
    }
    Ok(failures)
}

fn metrics_set_zero_gate_failures(value: &Value) -> EvalResult<Vec<&'static str>> {
    let metrics_set = object(value, "GKX_EVAL_METRICS_SET_INVALID")?;
    let mut failures = BTreeSet::new();
    let aggregate = object(
        field(
            metrics_set,
            "aggregate_metrics",
            "GKX_EVAL_METRICS_SET_INVALID",
        )?,
        "GKX_EVAL_METRICS_SET_INVALID",
    )?;
    failures.extend(zero_gate_failures(aggregate)?);
    for entry in array(
        field(
            metrics_set,
            "environment_aggregates",
            "GKX_EVAL_METRICS_SET_INVALID",
        )?,
        "GKX_EVAL_METRICS_SET_INVALID",
    )? {
        let entry = object(entry, "GKX_EVAL_METRICS_SET_INVALID")?;
        failures.extend(zero_gate_failures(object(
            field(entry, "aggregate_metrics", "GKX_EVAL_METRICS_SET_INVALID")?,
            "GKX_EVAL_METRICS_SET_INVALID",
        )?)?);
    }
    for row in array(
        field(
            metrics_set,
            "query_evaluations",
            "GKX_EVAL_METRICS_SET_INVALID",
        )?,
        "GKX_EVAL_METRICS_SET_INVALID",
    )? {
        let metrics = object(
            field(
                object(row, "GKX_EVAL_METRICS_SET_INVALID")?,
                "query_metrics",
                "GKX_EVAL_METRICS_SET_INVALID",
            )?,
            "GKX_EVAL_METRICS_SET_INVALID",
        )?;
        let citation = object(
            field(metrics, "citation", "GKX_EVAL_QUERY_METRICS_INVALID")?,
            "GKX_EVAL_QUERY_METRICS_INVALID",
        )?;
        let returned = metrics
            .get("returned_unique_source_count")
            .and_then(Value::as_u64)
            .ok_or(EvalError("GKX_EVAL_QUERY_METRICS_INVALID"))?;
        let valid = if returned == 0 {
            citation.get("applicability").and_then(Value::as_str) == Some("not_applicable")
                && citation.get("checked").and_then(Value::as_u64) == Some(0)
                && citation
                    .get("correctness_micros")
                    .is_some_and(Value::is_null)
        } else {
            citation.get("applicability").and_then(Value::as_str) == Some("required")
                && citation
                    .get("checked")
                    .and_then(Value::as_u64)
                    .is_some_and(|count| count > 0)
                && citation.get("correctness_micros").and_then(Value::as_u64) == Some(1_000_000)
        };
        if !valid {
            failures.insert("CITATION_COVERAGE");
        }
    }
    Ok(failures.into_iter().collect())
}

fn compare_baseline(input: &Value) -> EvalResult<Value> {
    let input = object(input, "GKX_EVAL_COMPARISON_INPUT_INVALID")?;
    exact_keys(
        input,
        &[
            "current_environment_set",
            "current_base_configuration",
            "current_tuning_grid",
            "current_tuning_axes",
            "current_golden",
            "current_metrics_set",
            "current_relative_ndcg_budget",
            "baseline",
        ],
        "GKX_EVAL_COMPARISON_INPUT_FIELDS_INVALID",
    )?;
    let baseline_value = field(input, "baseline", "GKX_EVAL_COMPARISON_INPUT_INVALID")?;
    seal_baseline(baseline_value)?;
    let baseline = object(baseline_value, "GKX_EVAL_BASELINE_INVALID")?;
    let golden_value = field(input, "current_golden", "GKX_EVAL_COMPARISON_INPUT_INVALID")?;
    seal_normalized_golden(golden_value)?;
    let golden = object(golden_value, "GKX_EVAL_GOLDEN_INVALID")?;
    let environments = field(
        input,
        "current_environment_set",
        "GKX_EVAL_COMPARISON_INPUT_INVALID",
    )?;
    seal_environment_set(environments, golden_value)?;
    let base = field(
        input,
        "current_base_configuration",
        "GKX_EVAL_COMPARISON_INPUT_INVALID",
    )?;
    seal_base_configuration(base)?;
    let grid = field(
        input,
        "current_tuning_grid",
        "GKX_EVAL_COMPARISON_INPUT_INVALID",
    )?;
    seal_tuning_grid(grid)?;
    let axes = field(
        input,
        "current_tuning_axes",
        "GKX_EVAL_COMPARISON_INPUT_INVALID",
    )?;
    seal_axes_coordinate(axes)?;
    let metrics_value = field(
        input,
        "current_metrics_set",
        "GKX_EVAL_COMPARISON_INPUT_INVALID",
    )?;
    seal_metrics_set(metrics_value, environments, golden_value)?;
    let metrics = object(metrics_value, "GKX_EVAL_METRICS_SET_INVALID")?;
    let budget = field(
        input,
        "current_relative_ndcg_budget",
        "GKX_EVAL_COMPARISON_INPUT_INVALID",
    )?;
    seal_budget(budget)?;
    let current_aggregate = object(
        field(metrics, "aggregate_metrics", "GKX_EVAL_METRICS_SET_INVALID")?,
        "GKX_EVAL_AGGREGATE_INVALID",
    )?;
    let baseline_metrics = object(
        field(baseline, "metrics_set", "GKX_EVAL_BASELINE_INVALID")?,
        "GKX_EVAL_METRICS_SET_INVALID",
    )?;
    let baseline_aggregate = object(
        field(
            baseline_metrics,
            "aggregate_metrics",
            "GKX_EVAL_METRICS_SET_INVALID",
        )?,
        "GKX_EVAL_AGGREGATE_INVALID",
    )?;
    let query_count = array(
        field(golden, "queries", "GKX_EVAL_GOLDEN_INVALID")?,
        "GKX_EVAL_GOLDEN_INVALID",
    )?
    .len() as u64;
    let maximum_top_k = array(
        field(golden, "queries", "GKX_EVAL_GOLDEN_INVALID")?,
        "GKX_EVAL_GOLDEN_INVALID",
    )?
    .iter()
    .filter_map(|query| query.get("expected_top_k").and_then(Value::as_u64))
    .max()
    .ok_or(EvalError("GKX_EVAL_COMPARISON_INPUT_INVALID"))?;
    let baseline_failures = metrics_set_zero_gate_failures(field(
        baseline,
        "metrics_set",
        "GKX_EVAL_BASELINE_INVALID",
    )?)?;
    let default_budget = |value: &Value| {
        value.get("numerator").and_then(Value::as_u64) == Some(2)
            && value.get("denominator").and_then(Value::as_u64) == Some(100)
    };
    let coordinate_changed = environments.get("environment_set_digest")
        != baseline.get("environment_set_digest")
        || base.get("base_configuration_digest") != baseline.get("base_configuration_digest")
        || grid.get("tuning_grid_digest") != baseline.get("tuning_grid_digest")
        || axes.get("tuning_axes_digest") != baseline.get("tuning_axes_digest")
        || golden.get("golden_digest") != baseline.get("normalized_golden_digest")
        || current_aggregate.get("query_count") != baseline.get("query_count")
        || Some(&Value::from(maximum_top_k)) != baseline.get("maximum_expected_top_k")
        || !default_budget(budget)
        || !default_budget(field(
            baseline,
            "relative_ndcg_budget",
            "GKX_EVAL_BASELINE_INVALID",
        )?);
    let human = coordinate_changed || !baseline_failures.is_empty();
    let mut reasons = if human {
        let mut reasons = Vec::new();
        if !baseline_failures.is_empty() {
            reasons.push("BASELINE_ZERO_GATE_INVALID");
        }
        if current_aggregate.get("query_count") != baseline.get("query_count") {
            reasons.push("QUERY_COUNT_CHANGED");
        }
        if coordinate_changed {
            reasons.push("COMPARABILITY_COORDINATE_CHANGED");
        }
        reasons
    } else {
        let mut reasons = metrics_set_zero_gate_failures(metrics_value)?;
        let current_ndcg = current_aggregate
            .get("ndcg_at_k_micros")
            .and_then(Value::as_u64)
            .ok_or(EvalError("GKX_EVAL_COMPARISON_INPUT_INVALID"))?;
        let baseline_ndcg = baseline_aggregate
            .get("ndcg_at_k_micros")
            .and_then(Value::as_u64)
            .ok_or(EvalError("GKX_EVAL_COMPARISON_INPUT_INVALID"))?;
        if compare_ndcg(baseline_ndcg, current_ndcg) == "regression" {
            reasons.push("NDCG_RELATIVE_REGRESSION");
        }
        reasons
    };
    reasons.sort_by(|left, right| utf16_cmp(left, right));
    let candidate_config_digest = canonical_digest(&json!({
        "base_configuration_digest": base.get("base_configuration_digest").unwrap(),
        "candidate_config": candidate_config_from_coordinate(axes)?,
    }))
    .map_err(|_| EvalError("GKX_EVAL_COMPARISON_INPUT_INVALID"))?;
    let current_evaluation_digest = evaluation_coordinate_digest(
        environments.get("environment_set_digest").unwrap(),
        golden.get("golden_digest").unwrap(),
        base.get("base_configuration_digest").unwrap(),
        grid.get("tuning_grid_digest").unwrap(),
        axes.get("tuning_axes_digest").unwrap(),
        &Value::String(candidate_config_digest),
        metrics,
        budget,
        query_count,
        maximum_top_k,
    )?;
    let status = if human {
        "needs_human"
    } else if reasons.is_empty() {
        "pass"
    } else {
        "regression"
    };
    let material = json!({
        "contract_version": COMPARISON_VERSION,
        "status": status,
        "reasons": reasons,
        "baseline_ndcg_at_k_micros": baseline_aggregate.get("ndcg_at_k_micros").unwrap(),
        "current_ndcg_at_k_micros": current_aggregate.get("ndcg_at_k_micros").unwrap(),
        "baseline_evaluation_digest": baseline.get("baseline_evaluation_digest").unwrap(),
        "current_evaluation_digest": current_evaluation_digest,
    });
    let mut output = object(&material, "GKX_EVAL_COMPARISON_INVALID")?.clone();
    output.insert(
        "comparison_digest".to_owned(),
        Value::String(
            canonical_digest(&material)
                .map_err(|_| EvalError("GKX_EVAL_COMPARISON_DIGEST_INVALID"))?,
        ),
    );
    Ok(Value::Object(output))
}

const SCENARIO_COUNTER_FIELDS: [&str; 11] = [
    "authority_input_snapshot_count",
    "source_read_count",
    "retrieval_sql_stage_count",
    "vector_provider_call_count",
    "vector_provider_item_count",
    "rerank_provider_call_count",
    "rerank_provider_item_count",
    "ranking_call_count",
    "confidence_call_count",
    "citation_verification_count",
    "metric_computation_count",
];

fn seal_scenario_outcome(value: &Value) -> EvalResult<Value> {
    let outcome = object(value, "GKX_EVAL_SCENARIO_OUTCOME_INVALID")?;
    exact_keys(
        outcome,
        &[
            "contract_version",
            "scenario_id",
            "kind",
            "public_result_digest",
            "coverage",
            "confidence",
            "reason_code",
            "message",
            "ordered_hit_projections",
            "citation_applicability",
            "host_classification",
            "exit_code",
            "work_counters",
            "effects",
            "outcome_digest",
        ],
        "GKX_EVAL_SCENARIO_OUTCOME_FIELDS_INVALID",
    )?;
    if outcome.get("contract_version") != Some(&Value::String(SCENARIO_OUTCOME_VERSION.to_owned()))
        || !outcome
            .get("scenario_id")
            .and_then(Value::as_str)
            .is_some_and(|id| bounded_id(id, 128))
        || !matches!(
            outcome.get("kind").and_then(Value::as_str),
            Some("result" | "insufficient" | "authorized_view_conflict" | "operational_exclusion")
        )
    {
        return code("GKX_EVAL_SCENARIO_OUTCOME_COORDINATE_INVALID");
    }
    if let Some(digest) = outcome
        .get("public_result_digest")
        .filter(|item| !item.is_null())
    {
        if !digest.as_str().is_some_and(is_digest) {
            return code("GKX_EVAL_SCENARIO_RESULT_DIGEST_INVALID");
        }
    }
    let projections = array(
        field(
            outcome,
            "ordered_hit_projections",
            "GKX_EVAL_SCENARIO_VALUE_INVALID",
        )?,
        "GKX_EVAL_SCENARIO_VALUE_INVALID",
    )?;
    if projections.len() > 100 {
        return code("GKX_EVAL_SCENARIO_VALUE_INVALID");
    }
    for projection in projections {
        let projection = object(projection, "GKX_EVAL_EXPECTED_TEMPORAL_HIT_INVALID")?;
        exact_keys(
            projection,
            &[
                "source_id",
                "temporal_state",
                "valid_from",
                "valid_to",
                "supersedes",
                "superseded_by",
            ],
            "GKX_EVAL_EXPECTED_TEMPORAL_HIT_FIELDS_INVALID",
        )?;
        if !projection
            .get("source_id")
            .and_then(Value::as_str)
            .is_some_and(is_valid_authored_uid)
            || !matches!(
                projection.get("temporal_state").and_then(Value::as_str),
                Some("current" | "historical" | "unknown")
            )
        {
            return code("GKX_EVAL_EXPECTED_TEMPORAL_HIT_COORDINATE_INVALID");
        }
    }
    let counters = object(
        field(
            outcome,
            "work_counters",
            "GKX_EVAL_SCENARIO_COUNTERS_INVALID",
        )?,
        "GKX_EVAL_SCENARIO_COUNTERS_INVALID",
    )?;
    exact_keys(
        counters,
        &SCENARIO_COUNTER_FIELDS,
        "GKX_EVAL_SCENARIO_COUNTER_FIELDS_INVALID",
    )?;
    for key in SCENARIO_COUNTER_FIELDS {
        count(
            field(counters, key, "GKX_EVAL_SCENARIO_COUNTER_INVALID")?,
            9_007_199_254_740_991,
            "GKX_EVAL_SCENARIO_COUNTER_INVALID",
        )?;
    }
    let vector_calls = counters
        .get("vector_provider_call_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let vector_items = counters
        .get("vector_provider_item_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let rerank_calls = counters
        .get("rerank_provider_call_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let rerank_items = counters
        .get("rerank_provider_item_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if (vector_calls == 0 && vector_items != 0) || (rerank_calls == 0 && rerank_items != 0) {
        return code("GKX_EVAL_SCENARIO_PROVIDER_COUNTER_RELATION_INVALID");
    }
    let effects = object(
        field(outcome, "effects", "GKX_EVAL_SCENARIO_EFFECTS_INVALID")?,
        "GKX_EVAL_SCENARIO_EFFECTS_INVALID",
    )?;
    exact_keys(
        effects,
        &[
            "public_result_emitted",
            "output_artifact_written",
            "state_mutated",
        ],
        "GKX_EVAL_SCENARIO_EFFECT_FIELDS_INVALID",
    )?;
    if effects.values().any(|value| !value.is_boolean()) {
        return code("GKX_EVAL_SCENARIO_EFFECT_VALUE_INVALID");
    }
    let snapshot = counters
        .get("authority_input_snapshot_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let downstream_zero = SCENARIO_COUNTER_FIELDS[1..]
        .iter()
        .all(|key| counters.get(*key).and_then(Value::as_u64) == Some(0));
    let all_zero = snapshot == 0 && downstream_zero;
    let emitted = effects
        .get("public_result_emitted")
        .and_then(Value::as_bool);
    let output_written = effects
        .get("output_artifact_written")
        .and_then(Value::as_bool);
    let mutated = effects.get("state_mutated").and_then(Value::as_bool);
    let kind = outcome
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let relation_valid = match kind {
        "result" => {
            let ordinary = matches!(
                outcome.get("coverage").and_then(Value::as_str),
                Some("not_requested" | "sufficient")
            ) && matches!(
                outcome.get("confidence").and_then(Value::as_str),
                Some("high" | "medium" | "low")
            ) && !projections.is_empty()
                && outcome
                    .get("citation_applicability")
                    .and_then(Value::as_str)
                    == Some("required")
                && counters
                    .get("citation_verification_count")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    > 0
                && counters
                    .get("metric_computation_count")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    > 0;
            let empty = outcome.get("coverage").and_then(Value::as_str) == Some("not_evaluated")
                && outcome.get("confidence").and_then(Value::as_str) == Some("insufficient")
                && projections.is_empty()
                && outcome
                    .get("citation_applicability")
                    .and_then(Value::as_str)
                    == Some("not_applicable")
                && counters
                    .get("citation_verification_count")
                    .and_then(Value::as_u64)
                    == Some(0);
            outcome
                .get("public_result_digest")
                .and_then(Value::as_str)
                .is_some_and(is_digest)
                && snapshot == 1
                && outcome.get("reason_code").is_some_and(Value::is_null)
                && outcome.get("message").is_some_and(Value::is_null)
                && outcome
                    .get("host_classification")
                    .is_some_and(Value::is_null)
                && outcome.get("exit_code").and_then(Value::as_u64) == Some(0)
                && emitted == Some(true)
                && output_written == Some(false)
                && mutated == Some(false)
                && (ordinary || empty)
                && !(outcome.get("coverage").and_then(Value::as_str) == Some("sufficient")
                    && projections.iter().any(|projection| {
                        projection.get("valid_from").is_some_and(Value::is_null)
                            || projection.get("temporal_state").and_then(Value::as_str)
                                == Some("unknown")
                    }))
        }
        "insufficient" => {
            outcome
                .get("public_result_digest")
                .and_then(Value::as_str)
                .is_some_and(is_digest)
                && outcome.get("coverage").and_then(Value::as_str) == Some("insufficient")
                && outcome.get("confidence").and_then(Value::as_str) == Some("insufficient")
                && outcome.get("reason_code").and_then(Value::as_str)
                    == Some("TEMPORAL_COVERAGE_INSUFFICIENT")
                && outcome.get("message").is_some_and(Value::is_null)
                && projections.is_empty()
                && outcome
                    .get("citation_applicability")
                    .and_then(Value::as_str)
                    == Some("not_applicable")
                && outcome
                    .get("host_classification")
                    .is_some_and(Value::is_null)
                && outcome.get("exit_code").and_then(Value::as_u64) == Some(0)
                && snapshot == 1
                && downstream_zero
                && emitted == Some(true)
                && output_written == Some(false)
                && mutated == Some(false)
        }
        "authorized_view_conflict" => {
            outcome
                .get("public_result_digest")
                .is_some_and(Value::is_null)
                && outcome.get("coverage").is_some_and(Value::is_null)
                && outcome.get("confidence").is_some_and(Value::is_null)
                && outcome.get("reason_code").and_then(Value::as_str)
                    == Some("RETRIEVAL_AUTHORIZED_VIEW_CONFLICT")
                && outcome.get("message").and_then(Value::as_str)
                    == Some("Authorized retrieval view conflict.")
                && projections.is_empty()
                && outcome
                    .get("citation_applicability")
                    .and_then(Value::as_str)
                    == Some("not_applicable")
                && outcome
                    .get("host_classification")
                    .is_some_and(Value::is_null)
                && outcome.get("exit_code").and_then(Value::as_u64) == Some(0)
                && snapshot == 1
                && downstream_zero
                && emitted == Some(false)
                && output_written == Some(false)
                && mutated == Some(false)
        }
        "operational_exclusion" => {
            outcome
                .get("public_result_digest")
                .is_some_and(Value::is_null)
                && outcome.get("coverage").is_some_and(Value::is_null)
                && outcome.get("confidence").is_some_and(Value::is_null)
                && outcome.get("reason_code").and_then(Value::as_str)
                    == Some("GKX_RETRIEVAL_EVALUATION_OPERATIONAL_FAILURE")
                && outcome.get("message").and_then(Value::as_str)
                    == Some("Retrieval evaluation failed safely.")
                && projections.is_empty()
                && outcome
                    .get("citation_applicability")
                    .and_then(Value::as_str)
                    == Some("not_applicable")
                && matches!(
                    outcome.get("host_classification").and_then(Value::as_str),
                    Some("fixture_authority_failure" | "retrieval_authority_failure")
                )
                && outcome.get("exit_code").and_then(Value::as_u64) == Some(3)
                && all_zero
                && emitted == Some(false)
                && output_written == Some(false)
                && mutated == Some(false)
        }
        _ => false,
    };
    if !relation_valid {
        return code(match kind {
            "result" => "GKX_EVAL_SCENARIO_RESULT_RELATION_INVALID",
            "insufficient" => "GKX_EVAL_SCENARIO_INSUFFICIENT_RELATION_INVALID",
            "authorized_view_conflict" => "GKX_EVAL_SCENARIO_CONFLICT_RELATION_INVALID",
            _ => "GKX_EVAL_SCENARIO_OPERATIONAL_RELATION_INVALID",
        });
    }
    verify_digest(
        value,
        "outcome_digest",
        "GKX_EVAL_SCENARIO_OUTCOME_DIGEST_MISMATCH",
    )?;
    Ok(value.clone())
}

fn compare_scenario_outcome(expected: &Value, observed: &Value) -> EvalResult<Value> {
    seal_scenario_outcome(expected)?;
    seal_scenario_outcome(observed)?;
    if expected.get("scenario_id") != observed.get("scenario_id") {
        return code("GKX_EVAL_SCENARIO_COMPARISON_COORDINATE_INVALID");
    }
    let pass = expected.get("outcome_digest") == observed.get("outcome_digest");
    let reasons = if pass {
        Vec::new()
    } else {
        let mut reasons = Vec::new();
        if expected.get("kind") != observed.get("kind") {
            reasons.push("SCENARIO_KIND_MISMATCH");
        }
        reasons.push("SCENARIO_OUTCOME_MISMATCH");
        reasons
    };
    let material = json!({
        "contract_version": SCENARIO_COMPARISON_VERSION,
        "status": if pass { "pass" } else { "regression" },
        "reasons": reasons,
        "expected_outcome_digest": expected.get("outcome_digest").ok_or(EvalError("GKX_EVAL_SCENARIO_OUTCOME_INVALID"))?,
        "observed_outcome_digest": observed.get("outcome_digest").ok_or(EvalError("GKX_EVAL_SCENARIO_OUTCOME_INVALID"))?,
    });
    let mut output = object(&material, "GKX_EVAL_SCENARIO_COMPARISON_INVALID")?.clone();
    output.insert(
        "scenario_comparison_digest".to_owned(),
        Value::String(
            canonical_digest(&material)
                .map_err(|_| EvalError("GKX_EVAL_SCENARIO_COMPARISON_DIGEST_INVALID"))?,
        ),
    );
    Ok(Value::Object(output))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Axes {
    rrf_k: u64,
    mmr: bool,
    mmr_lambda_micros: Option<u64>,
    semantic_top_k: u64,
    lexical_top_k: u64,
}

fn seal_axes(value: &Value) -> EvalResult<Axes> {
    let axes = object(value, "GKX_EVAL_TUNING_AXES_INVALID")?;
    exact_keys(
        axes,
        &[
            "rrf_k",
            "mmr",
            "mmr_lambda_micros",
            "semantic_top_k",
            "lexical_top_k",
        ],
        "GKX_EVAL_TUNING_AXES_FIELDS_INVALID",
    )?;
    let rrf_k = axes
        .get("rrf_k")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let mmr = axes
        .get("mmr")
        .and_then(Value::as_bool)
        .ok_or(EvalError("GKX_EVAL_TUNING_AXES_INVALID"))?;
    let semantic_top_k = axes
        .get("semantic_top_k")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let lexical_top_k = axes
        .get("lexical_top_k")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let lambda = axes.get("mmr_lambda_micros").and_then(Value::as_u64);
    if ![5, 10, 20, 30, 60, 100].contains(&rrf_k)
        || ![5, 10, 20, 40, 80].contains(&semantic_top_k)
        || ![5, 10, 20, 40, 80].contains(&lexical_top_k)
        || if mmr {
            !matches!(lambda, Some(0 | 300_000 | 500_000 | 700_000 | 1_000_000))
        } else {
            axes.get("mmr_lambda_micros") != Some(&Value::Null)
        }
    {
        return code("GKX_EVAL_TUNING_AXES_INVALID");
    }
    Ok(Axes {
        rrf_k,
        mmr,
        mmr_lambda_micros: lambda,
        semantic_top_k,
        lexical_top_k,
    })
}

fn candidate_config(axes: &Axes) -> Value {
    let mut retrieval = Map::new();
    retrieval.insert("rrf_k".to_owned(), json!(axes.rrf_k));
    retrieval.insert("mmr".to_owned(), json!(axes.mmr));
    if axes.mmr {
        retrieval.insert(
            "mmr_lambda".to_owned(),
            json!(axes.mmr_lambda_micros.unwrap_or_default() as f64 / 1_000_000_f64),
        );
    }
    retrieval.insert("semantic_top_k".to_owned(), json!(axes.semantic_top_k));
    retrieval.insert("lexical_top_k".to_owned(), json!(axes.lexical_top_k));
    json!({"config_version": 1, "retrieval": retrieval})
}

fn changed_axis_count(left: &Axes, baseline: &Axes) -> u64 {
    u64::from(left.rrf_k != baseline.rrf_k)
        + u64::from(left.mmr != baseline.mmr)
        + u64::from(left.mmr_lambda_micros != baseline.mmr_lambda_micros)
        + u64::from(left.semantic_top_k != baseline.semantic_top_k)
        + u64::from(left.lexical_top_k != baseline.lexical_top_k)
}

fn tune_priority_compare(left: &Value, right: &Value, baseline: &Axes) -> EvalResult<Ordering> {
    let left_record = object(left, "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID")?;
    let right_record = object(right, "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID")?;
    let left_axes = seal_axes(field(
        left_record,
        "axes",
        "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID",
    )?)?;
    let right_axes = seal_axes(field(
        right_record,
        "axes",
        "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID",
    )?)?;
    for metric in ["ndcg_at_k_micros", "recall_at_k_micros", "mrr_micros"] {
        let left_metric = count(
            field(
                left_record,
                metric,
                "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID",
            )?,
            1_000_000,
            "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID",
        )?;
        let right_metric = count(
            field(
                right_record,
                metric,
                "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID",
            )?,
            1_000_000,
            "GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID",
        )?;
        let order = right_metric.cmp(&left_metric);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    let comparisons = [
        changed_axis_count(&left_axes, baseline).cmp(&changed_axis_count(&right_axes, baseline)),
        (left_axes.semantic_top_k + left_axes.lexical_top_k)
            .cmp(&(right_axes.semantic_top_k + right_axes.lexical_top_k)),
        left_axes
            .rrf_k
            .abs_diff(60)
            .cmp(&right_axes.rrf_k.abs_diff(60)),
        u8::from(left_axes.mmr).cmp(&u8::from(right_axes.mmr)),
        left_axes
            .mmr_lambda_micros
            .unwrap_or(700_000)
            .abs_diff(700_000)
            .cmp(
                &right_axes
                    .mmr_lambda_micros
                    .unwrap_or(700_000)
                    .abs_diff(700_000),
            ),
    ];
    if let Some(order) = comparisons
        .into_iter()
        .find(|order| *order != Ordering::Equal)
    {
        return Ok(order);
    }
    let left_config = canonical_json(&candidate_config(&left_axes))
        .map_err(|_| EvalError("GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID"))?;
    let right_config = canonical_json(&candidate_config(&right_axes))
        .map_err(|_| EvalError("GKX_EVAL_TUNE_PRIORITY_CANDIDATE_INVALID"))?;
    Ok(utf16_cmp(&left_config, &right_config))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    const PACK: &str = "../../contracts/gkos-retrieval-evaluation-1.0.0-draft.1";

    fn fixture(name: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(PACK)
            .join(name);
        serde_json::from_slice(&fs::read(path).expect("read frozen fixture"))
            .expect("parse frozen JSON")
    }

    fn reseal(value: &mut Value, key: &str) {
        let digest = digest_without(value, key).expect("canonical digest");
        value
            .as_object_mut()
            .expect("object")
            .insert(key.to_owned(), Value::String(digest));
    }

    fn rows<'a>(fixture: &'a Value, key: &str) -> &'a Vec<Value> {
        fixture
            .get(key)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{key} must be an array"))
    }

    fn assert_unique_case_ids(rows: &[Value], expected: &[&str]) {
        let actual = rows
            .iter()
            .map(|row| row.get("case_id").and_then(Value::as_str).unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual.len(), rows.len(), "case IDs must be unique");
        assert_eq!(actual, expected.iter().copied().collect());
    }

    fn decoded_scalar(row: &Value) -> Option<String> {
        match row
            .get("encoding")
            .and_then(Value::as_str)
            .unwrap_or("literal")
        {
            "literal" => Some(row.get("value").and_then(Value::as_str).unwrap().to_owned()),
            "repeat_code_point" => {
                let prefix = row.get("prefix").and_then(Value::as_str).unwrap_or("");
                let suffix = row.get("suffix").and_then(Value::as_str).unwrap_or("");
                let character =
                    char::from_u32(row.get("code_point").and_then(Value::as_u64).unwrap() as u32)
                        .unwrap();
                let count = row.get("count").and_then(Value::as_u64).unwrap() as usize;
                Some(format!(
                    "{prefix}{}{suffix}",
                    character.to_string().repeat(count)
                ))
            }
            "utf16_code_units" => {
                let units = row
                    .get("code_units")
                    .and_then(Value::as_array)
                    .unwrap()
                    .iter()
                    .map(|unit| unit.as_u64().unwrap() as u16)
                    .collect::<Vec<_>>();
                String::from_utf16(&units).ok()
            }
            encoding => panic!("unknown fixture scalar encoding {encoding}"),
        }
    }

    #[test]
    fn frozen_pack_pin_hashes_bytes_and_hygiene_are_exact() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PACK);
        let pin: Value = serde_json::from_slice(
            &fs::read(directory.join("FULL-PIN.json")).expect("read Full pin"),
        )
        .expect("parse Full pin");
        assert_eq!(
            pin.get("reference_commit").and_then(Value::as_str),
            Some("cac029a5b570135b26f3585bc86f4c9beb00c36d")
        );
        assert_eq!(
            pin.get("reference_package_version").and_then(Value::as_str),
            Some("2.1.2")
        );
        assert_eq!(
            pin.get("publication_qualified").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(pin.get("pack_file_count").and_then(Value::as_u64), Some(37));
        assert_eq!(
            pin.get("pack_byte_count").and_then(Value::as_u64),
            Some(4_948_463)
        );
        let files = pin.get("files").and_then(Value::as_object).unwrap();
        assert_eq!(files.len(), 37);
        let mut total = 0_u64;
        for (name, expected_digest) in files {
            let bytes = fs::read(directory.join(name)).unwrap_or_else(|_| panic!("read {name}"));
            total += bytes.len() as u64;
            assert_eq!(sha256(&bytes), expected_digest.as_str().unwrap(), "{name}");
            assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]), "{name}: BOM");
            assert!(!bytes.contains(&b'\r'), "{name}: CR");
            assert!(std::str::from_utf8(&bytes).is_ok(), "{name}: UTF-8");
            assert!(bytes.ends_with(b"\n"), "{name}: terminal LF");
            assert!(!bytes.ends_with(b"\n\n"), "{name}: multiple terminal LF");
        }
        assert_eq!(total, 4_948_463);
        assert_eq!(fs::read_dir(directory).unwrap().count(), 38);

        let implementation = include_str!("evaluation.rs");
        let pure = implementation.split("#[cfg(test)]").next().unwrap();
        assert!(!pure.contains("pub fn "));
        assert!(!pure.contains("pub(crate) fn "));
        assert!(!pure.contains("use std::fs"));
        assert!(!pure.contains("rusqlite"));
        assert!(!pure.contains("toml::"));
    }

    #[test]
    fn frozen_metric_rows_are_independently_recomputed() {
        let fixture = fixture("metric-computation-fixture.json");
        verify_digest(&fixture, "fixture_digest", "fixture digest").unwrap();
        let cases = fixture.get("cases").and_then(Value::as_array).unwrap();
        assert_eq!(cases.len(), 58);
        let mut ids = BTreeSet::new();
        let mut metric_count = 0;
        let mut error_count = 0;
        for case in cases {
            verify_digest(case, "case_digest", "case digest").unwrap();
            let id = case.get("case_id").and_then(Value::as_str).unwrap();
            assert!(ids.insert(id));
            match case.get("expected_status").and_then(Value::as_str).unwrap() {
                "metrics" => {
                    let actual = compute_query_metrics(case.get("input").unwrap())
                        .unwrap_or_else(|error| panic!("{id}: unexpected {error}"));
                    assert_eq!(actual, *case.get("expected_metrics").unwrap(), "{id}");
                    metric_count += 1;
                }
                "error" => {
                    let error = compute_query_metrics(case.get("input").unwrap())
                        .expect_err("negative case must reject");
                    assert_eq!(
                        Some(error.0),
                        case.get("expected_code").and_then(Value::as_str),
                        "{id}"
                    );
                    error_count += 1;
                }
                status => panic!("unknown metric status {status}"),
            }
        }
        assert_eq!((metric_count, error_count), (53, 5));
    }

    #[test]
    fn frozen_scenario_union_and_comparison_are_executable() {
        let fixture = fixture("scenario-conformance-fixture.json");
        let outcomes = fixture.get("outcomes").and_then(Value::as_array).unwrap();
        assert_eq!(outcomes.len(), 5);
        let mut by_id = HashMap::new();
        let mut kinds = BTreeSet::new();
        let mut branches = BTreeSet::new();
        for outcome in outcomes {
            seal_scenario_outcome(outcome).unwrap();
            let id = outcome.get("scenario_id").and_then(Value::as_str).unwrap();
            by_id.insert(id, outcome);
            let kind = outcome.get("kind").and_then(Value::as_str).unwrap();
            kinds.insert(kind);
            if kind == "result" {
                branches.insert(outcome.get("coverage").and_then(Value::as_str).unwrap());
            }
        }
        assert_eq!(
            kinds,
            [
                "authorized_view_conflict",
                "insufficient",
                "operational_exclusion",
                "result"
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(
            branches,
            ["not_evaluated", "not_requested"].into_iter().collect()
        );

        let same =
            compare_scenario_outcome(by_id["ordinary-result"], by_id["ordinary-result"]).unwrap();
        assert_eq!(same.get("status").and_then(Value::as_str), Some("pass"));
        let mut conflict = by_id["authorized-conflict"].clone();
        conflict.as_object_mut().unwrap().insert(
            "scenario_id".to_owned(),
            Value::String("ordinary-result".to_owned()),
        );
        reseal(&mut conflict, "outcome_digest");
        let changed = compare_scenario_outcome(by_id["ordinary-result"], &conflict).unwrap();
        assert_eq!(
            changed.get("status").and_then(Value::as_str),
            Some("regression")
        );
        assert_eq!(
            changed.get("reasons"),
            Some(&json!([
                "SCENARIO_KIND_MISMATCH",
                "SCENARIO_OUTCOME_MISMATCH"
            ]))
        );

        let negative_rows = rows(&fixture, "semantic_negative_matrix");
        assert_unique_case_ids(
            negative_rows,
            &[
                "conflict-downstream-work",
                "operational-after-snapshot",
                "rerank-items-without-call",
                "scenario-id-overlong",
                "sufficient-unknown-hit",
                "vector-items-without-call",
            ],
        );
        for row in negative_rows {
            let base_id = row.get("base_scenario_id").and_then(Value::as_str).unwrap();
            let mut candidate = by_id[base_id].clone();
            match row.get("mutation").and_then(Value::as_str).unwrap() {
                "vector_provider_item_count_one" => {
                    candidate["work_counters"]["vector_provider_item_count"] = json!(1);
                }
                "rerank_provider_item_count_one" => {
                    candidate["work_counters"]["rerank_provider_item_count"] = json!(1);
                }
                "coverage_sufficient_keep_unknown_hit" => {
                    candidate["coverage"] = json!("sufficient");
                }
                "retrieval_sql_stage_count_one" => {
                    candidate["work_counters"]["retrieval_sql_stage_count"] = json!(1);
                }
                "authority_input_snapshot_count_one" => {
                    candidate["work_counters"]["authority_input_snapshot_count"] = json!(1);
                }
                "scenario_id_129_ascii" => {
                    candidate["scenario_id"] = Value::String("a".repeat(129));
                }
                mutation => panic!("unknown scenario mutation {mutation}"),
            }
            reseal(&mut candidate, "outcome_digest");
            assert_eq!(
                Some(seal_scenario_outcome(&candidate).unwrap_err().0),
                row.get("expected_code").and_then(Value::as_str),
                "{}",
                row["case_id"]
            );
        }

        let comparison_rows = rows(&fixture, "comparison_matrix");
        assert_unique_case_ids(comparison_rows, &["kind-transition", "same-outcome"]);
        for row in comparison_rows {
            let expected = by_id[row
                .get("expected_scenario_id")
                .and_then(Value::as_str)
                .unwrap()];
            let observed = if let Some(id) = row.get("observed_scenario_id").and_then(Value::as_str)
            {
                by_id[id].clone()
            } else {
                assert_eq!(
                    row.get("observed_mutation").and_then(Value::as_str),
                    Some("authorized_conflict_same_scenario_id")
                );
                let mut value = by_id["authorized-conflict"].clone();
                value["scenario_id"] = expected.get("scenario_id").unwrap().clone();
                reseal(&mut value, "outcome_digest");
                value
            };
            let comparison = compare_scenario_outcome(expected, &observed).unwrap();
            assert_eq!(
                comparison.get("status"),
                row.get("expected_status"),
                "{}",
                row["case_id"]
            );
            assert_eq!(
                comparison.get("reasons"),
                row.get("expected_reasons"),
                "{}",
                row["case_id"]
            );
        }

        let precedence = rows(&fixture, "precedence_matrix");
        assert_unique_case_ids(precedence, &["known-conflict-over-unknown-coverage"]);
        let row = &precedence[0];
        assert_eq!(
            row.get("known_conflict").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            row.get("unknown_coverage").and_then(Value::as_bool),
            Some(true)
        );
        let mut competing = by_id["insufficient"].clone();
        competing["scenario_id"] = by_id["authorized-conflict"]["scenario_id"].clone();
        reseal(&mut competing, "outcome_digest");
        let precedence_comparison =
            compare_scenario_outcome(by_id["authorized-conflict"], &competing).unwrap();
        assert_eq!(
            precedence_comparison.get("status"),
            row.get("expected_comparison_status")
        );
        assert_eq!(
            precedence_comparison.get("reasons"),
            row.get("expected_comparison_reasons")
        );
    }

    #[test]
    fn frozen_tune_priority_rows_drive_every_tie_break() {
        let fixture = fixture("tune-priority-fixture.json");
        verify_digest(&fixture, "fixture_digest", "fixture digest").unwrap();
        let cases = fixture.get("cases").and_then(Value::as_array).unwrap();
        assert_eq!(cases.len(), 11);
        let mut ids = BTreeSet::new();
        for case in cases {
            verify_digest(case, "case_digest", "case digest").unwrap();
            let case_id = case.get("case_id").and_then(Value::as_str).unwrap();
            assert!(ids.insert(case_id));
            let baseline = seal_axes(case.get("baseline_axes").unwrap()).unwrap();
            let baseline_ndcg = case
                .get("baseline_ndcg_at_k_micros")
                .and_then(Value::as_u64)
                .unwrap();
            let candidates = case.get("candidates").and_then(Value::as_array).unwrap();
            let mut conforming = candidates
                .iter()
                .filter(|candidate| {
                    candidate
                        .get("zero_gate_failures")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                        && candidate
                            .get("ndcg_at_k_micros")
                            .and_then(Value::as_u64)
                            .is_some_and(|value| value >= baseline_ndcg)
                })
                .collect::<Vec<_>>();
            conforming
                .sort_by(|left, right| tune_priority_compare(left, right, &baseline).unwrap());
            let ordered = conforming
                .iter()
                .map(|candidate| {
                    candidate
                        .get("candidate_id")
                        .and_then(Value::as_str)
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let mut sorted = ordered.clone();
            sorted.sort_by(|left, right| utf16_cmp(left, right));
            assert_eq!(
                Value::Array(sorted.into_iter().map(|id| json!(id)).collect()),
                *case.get("expected_conforming_candidate_ids").unwrap(),
                "{case_id}"
            );
            assert_eq!(
                Value::Array(ordered.iter().map(|id| json!(id)).collect()),
                *case.get("expected_ordered_candidate_ids").unwrap(),
                "{case_id}"
            );
            assert_eq!(
                ordered.first().copied().map_or(Value::Null, Value::from),
                *case.get("expected_selected_candidate_id").unwrap(),
                "{case_id}"
            );
        }
    }

    #[test]
    fn frozen_tune_grid_and_private_host_matrix_boundaries_are_exhaustive() {
        let conformance = fixture("conformance-fixture.json");
        let tune_rows = rows(&conformance, "tune_matrix");
        assert_unique_case_ids(
            tune_rows,
            &["max-top-k-100", "max-top-k-80", "shipped-24-query-grid"],
        );
        for row in tune_rows {
            let query_count = row.get("query_count").and_then(Value::as_u64).unwrap();
            let maximum_top_k = row
                .get("maximum_expected_top_k")
                .and_then(Value::as_u64)
                .unwrap();
            let available_top_k = [5_u64, 10, 20, 40, 80]
                .into_iter()
                .filter(|top_k| *top_k >= maximum_top_k)
                .count() as u64;
            let evaluated = 6 * 6 * available_top_k * available_top_k;
            assert_eq!(
                Some(evaluated),
                row.get("expected_evaluated_candidate_count")
                    .and_then(Value::as_u64),
                "{}",
                row["case_id"]
            );
            assert_eq!(
                Some(900 - evaluated),
                row.get("expected_excluded_candidate_count")
                    .and_then(Value::as_u64)
            );
            assert_eq!(
                Some(evaluated * query_count),
                row.get("expected_query_evaluation_count")
                    .and_then(Value::as_u64)
            );
            if let Some(axes) = row.get("expected_selected_axes") {
                seal_axes_coordinate(axes).unwrap();
                let baseline = conformance
                    .get("valid_envelopes")
                    .and_then(Value::as_object)
                    .and_then(|valid| valid.get("baseline"))
                    .and_then(Value::as_object)
                    .unwrap();
                let config_digest = canonical_digest(&json!({
                    "base_configuration_digest": baseline.get("base_configuration_digest").unwrap(),
                    "candidate_config": candidate_config_from_coordinate(axes).unwrap(),
                }))
                .unwrap();
                assert_eq!(
                    Some(config_digest.as_str()),
                    row.get("expected_selected_candidate_config_digest")
                        .and_then(Value::as_str)
                );
                assert_eq!(
                    row.get("expected_selected_candidate_evaluation_digest"),
                    baseline.get("baseline_evaluation_digest")
                );
            }
        }

        let provider_rows = rows(&conformance, "provider_semantic_matrix");
        assert_unique_case_ids(
            provider_rows,
            &[
                "all-excluded-tune-index-replay",
                "disabled-embedding-retains-template",
                "disabled-reranker-retains-oracle",
                "embedding-failure-with-response",
                "embedding-failure-wrong-stage",
                "embedding-query-failure-degrades",
                "embedding-template-permutation",
                "eval-query-count-256",
                "eval-query-count-257",
                "eval-query-count-30",
                "eval-query-count-31",
                "fts-only-disabled-provider-roles",
                "hybrid-without-reranker",
                "reranker-failure-degrades",
                "reranker-failure-wrong-error",
                "reranker-items-0",
                "reranker-items-100",
                "reranker-items-101",
                "reranker-items-160",
                "reranker-items-161",
                "reranker-template-permutation",
            ],
        );
        let mut provider_kinds = BTreeSet::new();
        for row in provider_rows {
            let kind = row.get("kind").and_then(Value::as_str).unwrap();
            provider_kinds.insert(kind);
            match kind {
                "reranker_item_boundary" => assert_eq!(
                    row.get("item_count").and_then(Value::as_u64).unwrap() <= 160,
                    row.get("expected_valid").and_then(Value::as_bool).unwrap()
                ),
                "query_count_boundary" => {
                    let count = row.get("query_count").and_then(Value::as_u64).unwrap();
                    assert_eq!(
                        count <= 256,
                        row.get("expected_valid").and_then(Value::as_bool).unwrap()
                    );
                    if count <= 256 {
                        assert_eq!(
                            count <= 30,
                            row.get("expected_tune_applicable")
                                .and_then(Value::as_bool)
                                .unwrap()
                        );
                    }
                }
                "template_permutation" => assert!(matches!(
                    row.get("template_role").and_then(Value::as_str),
                    Some("embedding" | "reranker")
                )),
                "all_excluded_tune" => {
                    assert_eq!(
                        row.get("expected_query_evaluation_count")
                            .and_then(Value::as_u64),
                        Some(0)
                    );
                    assert_eq!(
                        row.get("expected_vector_provider_call_count")
                            .and_then(Value::as_u64),
                        Some(1)
                    );
                }
                "provider_role_conditional" => assert!(matches!(
                    row.get("embedding_role").and_then(Value::as_str),
                    Some("active" | "disabled")
                )),
                "provider_conditional_negative" => assert!(row
                    .get("expected_code")
                    .and_then(Value::as_str)
                    .is_some_and(|code| code.starts_with("GKX_EVAL_FIXED_"))),
                kind => panic!("unknown provider fixture kind {kind}"),
            }
        }
        assert_eq!(
            provider_kinds,
            [
                "all_excluded_tune",
                "provider_conditional_negative",
                "provider_role_conditional",
                "query_count_boundary",
                "reranker_item_boundary",
                "template_permutation",
            ]
            .into_iter()
            .collect()
        );

        // These matrices intentionally remain Full-host authority. Lite binds
        // every row and stable code without parsing catalogs, GKX, providers,
        // index receipts, or reviewed-bundle source bytes.
        let endpoint_rows = rows(&conformance, "environment_endpoint_partition_matrix");
        assert_unique_case_ids(
            endpoint_rows,
            &[
                "bundle-endpoint-class-substitution",
                "bundle-endpoint-extra-uid",
                "bundle-endpoint-source-only-omission",
                "bundle-source-endpoint-only",
            ],
        );
        assert!(endpoint_rows.iter().all(|row| row
            .get("expected_code")
            .and_then(Value::as_str)
            .is_some_and(|code| code.starts_with("GKX_EVAL_"))));
        let reviewed_rows = rows(&conformance, "reviewed_bundle_negative_matrix");
        assert_unique_case_ids(
            reviewed_rows,
            &[
                "reviewed-absent-endpoint-extra",
                "reviewed-absent-index-input-order",
                "reviewed-absent-index-request-substitution",
                "reviewed-absent-index-response-omission",
                "reviewed-absent-source-class-substitution",
                "reviewed-baseline-axes-substitution",
                "reviewed-baseline-grid-substitution",
                "reviewed-companion-digest-splice",
                "reviewed-future-result-splice",
                "reviewed-hidden-result-splice",
                "reviewed-origin-counter-splice",
                "reviewed-origin-duplicate",
                "reviewed-origin-omission",
                "reviewed-origin-order",
                "reviewed-origin-provider-splice",
                "reviewed-origin-public-result-splice",
                "reviewed-origin-request-splice",
                "reviewed-origin-schedule-splice",
                "reviewed-pair-public-view-splice",
            ],
        );
        assert_eq!(
            reviewed_rows
                .iter()
                .map(|row| row.get("stage").and_then(Value::as_str).unwrap())
                .collect::<BTreeSet<_>>(),
            ["executable", "host_receipt", "shallow"]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn frozen_normalized_environment_metrics_and_baseline_bundle_is_fully_sealed() {
        let fixture = fixture("conformance-fixture.json");
        let valid = fixture
            .get("valid_envelopes")
            .and_then(Value::as_object)
            .unwrap();
        seal_query_metrics(valid.get("query_metrics").unwrap()).unwrap();
        seal_aggregate_metrics(valid.get("aggregate_metrics").unwrap()).unwrap();
        let baseline = valid.get("baseline").unwrap();
        seal_baseline(baseline).unwrap();
        let baseline = baseline.as_object().unwrap();
        seal_environment_set(
            valid.get("environment_set").unwrap(),
            baseline.get("normalized_golden").unwrap(),
        )
        .unwrap();
        seal_metrics_set(
            valid.get("metrics_set").unwrap(),
            valid.get("environment_set").unwrap(),
            baseline.get("normalized_golden").unwrap(),
        )
        .unwrap();

        let rows = valid
            .get("metrics_set")
            .and_then(Value::as_object)
            .and_then(|set| set.get("query_evaluations"))
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|row| row.get("query_metrics").unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(
            aggregate_query_metrics(&rows).unwrap(),
            *valid
                .get("metrics_set")
                .and_then(Value::as_object)
                .and_then(|set| set.get("aggregate_metrics"))
                .unwrap()
        );
        assert_eq!(rows.len(), 24);
    }

    #[test]
    fn frozen_metric_semantic_negatives_and_baseline_comparison_are_executable() {
        let conformance = fixture("conformance-fixture.json");
        let valid = conformance
            .get("valid_envelopes")
            .and_then(Value::as_object)
            .unwrap();
        seal_observation_report(valid.get("observation_report").unwrap()).unwrap();

        let negative_rows = rows(&conformance, "metric_semantic_negative_matrix");
        assert_unique_case_ids(
            negative_rows,
            &[
                "aggregate-failures-over-query-count",
                "forged-citation-rate",
                "query-id-overlong",
                "returned-zero-with-citation",
                "top-k-zero",
                "unknown-field",
            ],
        );
        for row in negative_rows {
            let mutation = row.get("mutation").and_then(Value::as_str).unwrap();
            let mut candidate = match row.get("target").and_then(Value::as_str).unwrap() {
                "query_metrics" => valid.get("query_metrics").unwrap().clone(),
                "aggregate_metrics" => valid.get("aggregate_metrics").unwrap().clone(),
                target => panic!("unknown metric target {target}"),
            };
            match mutation {
                "query_id_129_ascii" => {
                    candidate["query_id"] = Value::String("a".repeat(129));
                    reseal(&mut candidate, "query_metrics_digest");
                }
                "expected_top_k_zero" => {
                    candidate["expected_top_k"] = json!(0);
                    reseal(&mut candidate, "query_metrics_digest");
                }
                "returned_unique_source_count_zero" => {
                    candidate["returned_unique_source_count"] = json!(0);
                    candidate["relevant_returned_source_count"] = json!(0);
                    candidate["relevant_source_ranks"] = json!([]);
                    candidate["first_relevant_rank"] = Value::Null;
                    candidate["recall_at_k_micros"] = json!(0);
                    candidate["mrr_micros"] = json!(0);
                    candidate["ndcg_at_k_micros"] = json!(0);
                    candidate["policy"]["policy_identity_field_count"] = json!(2);
                    reseal(&mut candidate, "query_metrics_digest");
                }
                "citation_correctness_zero" => {
                    candidate["citation"]["correctness_micros"] = json!(0);
                    reseal(&mut candidate, "query_metrics_digest");
                }
                "temporal_mismatch_count_over_query_count" => {
                    let query_count = candidate["query_count"].as_u64().unwrap();
                    candidate["temporal_mismatch_count"] = json!(query_count + 1);
                    reseal(&mut candidate, "aggregate_metrics_digest");
                }
                "add_unknown_field" => {
                    candidate["unknown_field"] = Value::Bool(true);
                }
                mutation => panic!("unknown metric mutation {mutation}"),
            }
            let error = if row.get("target").and_then(Value::as_str) == Some("query_metrics") {
                seal_query_metrics(&candidate).unwrap_err()
            } else {
                seal_aggregate_metrics(&candidate).unwrap_err()
            };
            assert_eq!(
                Some(error.0),
                row.get("expected_code").and_then(Value::as_str),
                "{}",
                row.get("case_id").and_then(Value::as_str).unwrap()
            );
        }

        let mut minimum_policy_fields = valid.get("query_metrics").unwrap().clone();
        let returned = minimum_policy_fields["returned_unique_source_count"]
            .as_u64()
            .unwrap();
        let minimum = 2 + 6 * returned;
        assert_eq!(
            minimum_policy_fields["policy"]["policy_identity_field_count"],
            json!(minimum)
        );
        reseal(&mut minimum_policy_fields, "query_metrics_digest");
        seal_query_metrics(&minimum_policy_fields).unwrap();

        minimum_policy_fields["policy"]["policy_identity_field_count"] = json!(minimum - 1);
        minimum_policy_fields["policy"]["policy_leak_count"] = json!(0);
        minimum_policy_fields["policy"]["policy_leak_rate_micros"] = json!(0);
        reseal(&mut minimum_policy_fields, "query_metrics_digest");
        assert_eq!(
            seal_query_metrics(&minimum_policy_fields).unwrap_err().0,
            "GKX_EVAL_QUERY_POLICY_FIELD_COUNT_INVALID"
        );

        let mut aggregate_left = valid.get("query_metrics").unwrap().clone();
        let mut aggregate_right = valid.get("query_metrics").unwrap().clone();
        aggregate_right["query_id"] = json!("aggregate-policy-boundary-second");
        aggregate_left["policy"]["policy_identity_field_count"] =
            json!(JS_MAX_SAFE_INTEGER - minimum);
        aggregate_right["policy"]["policy_identity_field_count"] = json!(minimum);
        for row in [&mut aggregate_left, &mut aggregate_right] {
            row["policy"]["policy_leak_count"] = json!(0);
            row["policy"]["policy_leak_rate_micros"] = json!(0);
            reseal(row, "query_metrics_digest");
            seal_query_metrics(row).unwrap();
        }
        let aggregate =
            aggregate_query_metrics(&[aggregate_left.clone(), aggregate_right.clone()]).unwrap();
        assert_eq!(
            aggregate["policy"]["policy_identity_field_count"],
            json!(JS_MAX_SAFE_INTEGER)
        );

        aggregate_left["policy"]["policy_identity_field_count"] =
            json!(JS_MAX_SAFE_INTEGER - minimum + 1);
        reseal(&mut aggregate_left, "query_metrics_digest");
        assert_eq!(
            aggregate_query_metrics(&[aggregate_left, aggregate_right])
                .unwrap_err()
                .0,
            "GKX_EVAL_AGGREGATE_POLICY_COUNT_OVERFLOW"
        );

        let baseline = valid.get("baseline").unwrap();
        let comparison_input = json!({
            "current_environment_set": baseline.get("environment_set").unwrap(),
            "current_base_configuration": baseline.get("base_configuration").unwrap(),
            "current_tuning_grid": baseline.get("tuning_grid").unwrap(),
            "current_tuning_axes": baseline.get("selected_axes").unwrap(),
            "current_golden": baseline.get("normalized_golden").unwrap(),
            "current_metrics_set": baseline.get("metrics_set").unwrap(),
            "current_relative_ndcg_budget": {"numerator": 2, "denominator": 100},
            "baseline": baseline,
        });
        let comparison = compare_baseline(&comparison_input).unwrap();
        assert_eq!(
            comparison.get("status").and_then(Value::as_str),
            Some("pass")
        );
        assert_eq!(comparison.get("reasons"), Some(&json!([])));
        verify_digest(&comparison, "comparison_digest", "comparison digest").unwrap();
        let mut equivalent_budget = comparison_input;
        equivalent_budget["current_relative_ndcg_budget"] =
            json!({"numerator": 1, "denominator": 50});
        let changed = compare_baseline(&equivalent_budget).unwrap();
        assert_eq!(
            changed.get("status").and_then(Value::as_str),
            Some("needs_human")
        );

        let comparison_rows = rows(&conformance, "comparison_matrix");
        assert_unique_case_ids(
            comparison_rows,
            &[
                "environment-change",
                "exact-two-percent-boundary",
                "over-budget",
                "zero-baseline",
                "zero-hit-current",
            ],
        );
        for row in comparison_rows {
            let query_count = row.get("query_count").and_then(Value::as_u64).unwrap();
            let baseline_perfect = row
                .get("baseline_perfect_query_count")
                .and_then(Value::as_u64)
                .unwrap();
            let current_perfect = row
                .get("current_perfect_query_count")
                .and_then(Value::as_u64)
                .unwrap();
            let baseline_ndcg = round_ratio(
                u128::from(baseline_perfect),
                u128::from(query_count),
                METRIC_SCALE,
            )
            .unwrap();
            let current_ndcg = round_ratio(
                u128::from(current_perfect),
                u128::from(query_count),
                METRIC_SCALE,
            )
            .unwrap();
            assert_eq!(
                Some(current_ndcg),
                row.get("expected_current_ndcg_at_k_micros")
                    .and_then(Value::as_u64)
            );
            let status = if row.get("case_id").and_then(Value::as_str) == Some("environment-change")
            {
                "needs_human"
            } else {
                compare_ndcg(baseline_ndcg, current_ndcg)
            };
            assert_eq!(
                Some(status),
                row.get("expected_status").and_then(Value::as_str)
            );
            if row.get("case_id").and_then(Value::as_str) == Some("zero-hit-current") {
                assert_eq!(row.get("expected_reason"), Some(&Value::Null));
            }
        }
    }

    #[test]
    fn frozen_portable_and_unicode_semantic_supplements_are_executable() {
        let conformance = fixture("conformance-fixture.json");
        let portable = rows(&conformance, "portable_scalar_matrix");
        assert_unique_case_ids(
            portable,
            &[
                "invalid-version-uid",
                "path-absolute",
                "path-ads",
                "path-double-slash",
                "path-parent",
                "path-reserved",
                "path-trailing-dot",
                "path-trailing-slash",
                "path-trailing-space",
                "path-utf8-1024-bytes",
                "path-utf8-1025-bytes",
                "uppercase-valid-uid",
            ],
        );
        for row in portable {
            let value = decoded_scalar(row).unwrap();
            let valid = match row.get("kind").and_then(Value::as_str).unwrap() {
                "uid" => is_valid_authored_uid(&value),
                "source_path" => valid_source_path(&value),
                kind => panic!("unknown scalar kind {kind}"),
            };
            assert_eq!(
                Some(valid),
                row.get("runtime_valid").and_then(Value::as_bool),
                "{}",
                row.get("case_id").and_then(Value::as_str).unwrap()
            );
            if row.get("case_id").and_then(Value::as_str) == Some("path-utf8-1024-bytes") {
                assert_eq!(value.len(), 1024);
            }
            if row.get("case_id").and_then(Value::as_str) == Some("path-utf8-1025-bytes") {
                assert_eq!(value.len(), 1025);
            }
        }

        let identities = rows(&conformance, "opaque_identity_matrix");
        assert_unique_case_ids(
            identities,
            &[
                "all-ecmascript-trim",
                "astral-512-utf16",
                "astral-514-utf16",
                "c0-control",
                "del-control",
                "internal-spaces",
                "lone-surrogate",
                "url-looking-unicode",
            ],
        );
        for row in identities {
            let expected = row.get("semantic_valid").and_then(Value::as_bool).unwrap();
            match decoded_scalar(row) {
                Some(value) => {
                    assert_eq!(
                        valid_opaque_identity(&value),
                        expected,
                        "{}",
                        row["case_id"]
                    );
                    if row.get("case_id").and_then(Value::as_str) == Some("astral-512-utf16") {
                        assert_eq!(value.encode_utf16().count(), 512);
                    }
                    if row.get("case_id").and_then(Value::as_str) == Some("astral-514-utf16") {
                        assert_eq!(value.encode_utf16().count(), 514);
                    }
                }
                None => {
                    assert_eq!(
                        row.get("case_id").and_then(Value::as_str),
                        Some("lone-surrogate")
                    );
                    assert!(!expected);
                    assert!(serde_json::from_str::<Value>(r#""\ud800""#).is_err());
                }
            }
        }

        let versions = rows(&conformance, "observation_version_matrix");
        assert_unique_case_ids(
            versions,
            &[
                "ascii-32",
                "ascii-33",
                "del",
                "non-ascii",
                "single-ascii",
                "space",
                "vendor-label",
            ],
        );
        for row in versions {
            let value = decoded_scalar(row).unwrap();
            assert_eq!(
                valid_observation_version(&value),
                row.get("expected_valid").and_then(Value::as_bool).unwrap(),
                "{}",
                row["case_id"]
            );
        }
    }

    #[test]
    fn frozen_conformance_surfaces_and_host_only_rows_cannot_drift() {
        let conformance = fixture("conformance-fixture.json");
        assert_eq!(
            conformance.get("contract_version").and_then(Value::as_str),
            Some("gkos-retrieval-evaluation-conformance/1.0.0-draft.1")
        );
        assert_eq!(
            conformance
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            [
                "comparison_matrix",
                "contract_version",
                "environment_endpoint_partition_matrix",
                "fixture_files",
                "golden",
                "metric_semantic_negative_matrix",
                "ndcg",
                "observation_version_matrix",
                "opaque_identity_matrix",
                "oracle_partition_matrix",
                "portable_scalar_matrix",
                "provider_semantic_matrix",
                "reviewed_bundle_negative_matrix",
                "tune_matrix",
                "valid_envelopes",
            ]
            .into_iter()
            .collect()
        );
        let golden = conformance
            .get("golden")
            .and_then(Value::as_object)
            .unwrap();
        assert_eq!(
            golden.get("toml_file").and_then(Value::as_str),
            Some("golden-fixture.toml")
        );
        seal_normalized_golden(golden.get("expected_normalized").unwrap()).unwrap();
        assert_eq!(
            golden.get("expected_normalized"),
            conformance
                .get("valid_envelopes")
                .and_then(Value::as_object)
                .and_then(|valid| valid.get("baseline"))
                .and_then(Value::as_object)
                .and_then(|baseline| baseline.get("normalized_golden"))
        );
        let parser_rows = golden
            .get("parser_negative_matrix")
            .and_then(Value::as_array)
            .unwrap();
        assert_unique_case_ids(
            parser_rows,
            &[
                "bare-cr",
                "bom",
                "duplicate-key",
                "insufficient-ordinary",
                "nonempty-lineage",
                "surrogate-escape",
                "trailing-comma",
                "unknown-key",
            ],
        );
        for row in parser_rows {
            let expected = match row.get("mutation").and_then(Value::as_str).unwrap() {
                "prepend_bom" => "GKX_EVAL_GOLDEN_TOML_BOM_INVALID",
                "replace_first_lf_with_bare_cr" => "GKX_EVAL_GOLDEN_TOML_NEWLINE_INVALID",
                "replace_first_query_id_key_with_unknown" => {
                    "GKX_EVAL_GOLDEN_TOML_QUERY_KEY_UNKNOWN"
                }
                "duplicate_first_query_id" => "GKX_EVAL_GOLDEN_TOML_QUERY_KEY_DUPLICATE",
                "add_first_array_trailing_comma" => {
                    "GKX_EVAL_GOLDEN_TOML_ARRAY_TRAILING_COMMA_INVALID"
                }
                "set_first_expected_lineage_nonempty" => "GKX_EVAL_LINEAGE_ID_UNAVAILABLE",
                "set_first_confidence_insufficient" => "GKX_EVAL_CONFIDENCE_INVALID",
                "set_first_text_lone_surrogate_escape" => {
                    "GKX_EVAL_GOLDEN_TOML_UNICODE_ESCAPE_INVALID"
                }
                mutation => panic!("unknown Full-only parser mutation {mutation}"),
            };
            assert_eq!(
                row.get("expected_code").and_then(Value::as_str),
                Some(expected)
            );
        }

        let companion_keys = [
            ("fixed_provider", "provider_fixture_digest"),
            ("fixture_catalog", "catalog_digest"),
            ("source_corpus", "source_corpus_digest"),
            ("metric_computation", "fixture_digest"),
            ("tune_priority", "fixture_digest"),
            ("reviewed_bundle", "reviewed_bundle_digest"),
        ];
        let companions = conformance
            .get("fixture_files")
            .and_then(Value::as_object)
            .unwrap();
        assert_eq!(companions.len(), companion_keys.len());
        for (key, digest_key) in companion_keys {
            let coordinate = companions.get(key).and_then(Value::as_object).unwrap();
            assert_eq!(
                coordinate
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                ["digest", "file"].into_iter().collect()
            );
            let companion = fixture(coordinate.get("file").and_then(Value::as_str).unwrap());
            assert_eq!(coordinate.get("digest"), companion.get(digest_key), "{key}");
        }

        let oracle_rows = rows(&conformance, "oracle_partition_matrix");
        assert_unique_case_ids(
            oracle_rows,
            &[
                "authorized-endpoint",
                "cross-role-explicit-classification",
                "endpoint-authorization-overlap",
                "endpoint-only-in-source-class",
                "forbidden-endpoint-occurrences",
                "malformed-endpoint-scalar",
                "null-lineage-and-absent-parent-zero",
                "source-only-in-endpoint-class",
                "unknown-endpoint",
            ],
        );
        for row in oracle_rows {
            match row.get("expected_status").and_then(Value::as_str).unwrap() {
                "metrics" => {
                    let fields = row
                        .get("expected_policy_identity_field_count")
                        .and_then(Value::as_u64)
                        .unwrap();
                    if let Some(leaks) = row
                        .get("expected_policy_leak_count")
                        .and_then(Value::as_u64)
                    {
                        assert_eq!(
                            row.get("expected_policy_leak_rate_micros")
                                .and_then(Value::as_u64),
                            Some(
                                round_ratio(u128::from(leaks), u128::from(fields), METRIC_SCALE,)
                                    .unwrap()
                            )
                        );
                    }
                    if row.get("case_id").and_then(Value::as_str)
                        == Some("null-lineage-and-absent-parent-zero")
                    {
                        assert_eq!(
                            row.get("expected_null_lineage_occurrences")
                                .and_then(Value::as_u64),
                            Some(0)
                        );
                        assert_eq!(
                            row.get("expected_absent_parent_occurrences")
                                .and_then(Value::as_u64),
                            Some(0)
                        );
                    }
                }
                "error" => assert!(matches!(
                    row.get("expected_code").and_then(Value::as_str),
                    Some(
                        "GKX_EVAL_ORACLE_CATALOG_PARTITION_INCOMPLETE"
                            | "GKX_EVAL_ORACLE_PARTITION_INCOMPLETE"
                            | "GKX_EVAL_ORACLE_AUTHORIZATION_OVERLAP"
                            | "GKX_EVAL_ORACLE_IDENTITY_INVALID"
                    )
                )),
                status => panic!("unknown oracle status {status}"),
            }
        }
        let metric_fixture = fixture("metric-computation-fixture.json");
        let metric_cases = metric_fixture
            .get("cases")
            .and_then(Value::as_array)
            .unwrap();
        for id in [
            "endpoint-no-source-fallback",
            "forbidden-endpoint-leak",
            "malformed-audited-endpoint",
            "null-lineage-absence",
        ] {
            assert!(metric_cases
                .iter()
                .any(|case| case.get("case_id").and_then(Value::as_str) == Some(id)));
        }
    }

    #[test]
    fn normalized_query_uses_production_lexical_and_time_semantics() {
        let conformance = fixture("conformance-fixture.json");
        let base = conformance
            .get("golden")
            .and_then(Value::as_object)
            .and_then(|golden| golden.get("expected_normalized"))
            .and_then(Value::as_object)
            .and_then(|golden| golden.get("queries"))
            .and_then(Value::as_array)
            .and_then(|queries| queries.first())
            .unwrap();
        let mutate_text = |text: String| {
            let mut query = base.clone();
            query["text"] = Value::String(text);
            reseal(&mut query, "query_digest");
            query
        };
        for (text, expected) in [
            ("...".to_owned(), "GKX_EVAL_QUERY_LEXICAL_INVALID"),
            ("\"unclosed".to_owned(), "GKX_EVAL_QUERY_LEXICAL_INVALID"),
            (
                "\"closed\"tail".to_owned(),
                "GKX_EVAL_QUERY_LEXICAL_INVALID",
            ),
            (
                std::iter::repeat_n("term", 65)
                    .collect::<Vec<_>>()
                    .join(" "),
                "GKX_EVAL_QUERY_LEXICAL_CLAUSE_COUNT_INVALID",
            ),
            (
                format!("\"{}\"", "a".repeat(257)),
                "GKX_EVAL_QUERY_LEXICAL_CLAUSE_SIZE_INVALID",
            ),
        ] {
            assert_eq!(
                seal_normalized_query(&mutate_text(text)).unwrap_err().0,
                expected
            );
        }
        seal_normalized_query(&mutate_text(
            std::iter::repeat_n("term", 64)
                .collect::<Vec<_>>()
                .join(" "),
        ))
        .unwrap();
        seal_normalized_query(&mutate_text(format!("\"{}\"", "a".repeat(256)))).unwrap();
        let trimmed = mutate_text("\u{feff}term\u{3000}".to_owned());
        assert_eq!(
            effective_query_text(trimmed.get("text").and_then(Value::as_str).unwrap()),
            "term"
        );
        seal_normalized_query(&trimmed).unwrap();
        let mut invalid_time = base.clone();
        invalid_time["as_of"] = json!("2026-08-22T00:00:00Z");
        reseal(&mut invalid_time, "query_digest");
        assert_eq!(
            seal_normalized_query(&invalid_time).unwrap_err().0,
            "GKX_EVAL_AS_OF_INVALID"
        );
    }

    #[test]
    fn ndcg_table_and_u128_boundary_are_exact() {
        let table = fixture("ndcg-discount-table.json");
        verify_digest(&table, "table_digest", "table digest").unwrap();
        assert_eq!(
            table.get("ndcg_discount_scale").and_then(Value::as_u64),
            Some(DISCOUNT_SCALE as u64)
        );
        assert_eq!(
            table.get("ndcg_discount_scaled"),
            Some(&Value::Array(
                NDCG_DISCOUNTS.into_iter().map(Value::from).collect()
            ))
        );
        assert_eq!(ndcg_micros(&[1], 1, 1).unwrap(), 1_000_000);
        assert_eq!(ndcg_micros(&[3], 1, 3).unwrap(), 500_000);
        assert_eq!(compare_ndcg(1_000_000, 980_000), "pass");
        assert_eq!(compare_ndcg(1_000_000, 979_999), "regression");
        assert_eq!(compare_ndcg(0, 0), "pass");
    }
}
