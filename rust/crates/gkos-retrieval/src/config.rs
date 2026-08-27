use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::digest::canonical_digest;
use crate::path_security::{absolute_lexical_path, contained_path, equivalent_paths};
use crate::providers::{RerankProviderConfig, VectorProviderConfig};
use crate::{RetrievalError, RetrievalResult};

pub const GKOS_HOST_CONFIG_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: u64 = 1_048_576;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

type TomlSubsetDocument = BTreeMap<String, BTreeMap<String, Value>>;

fn invalid_config(code: impl Into<String>) -> RetrievalError {
    RetrievalError::InvalidConfig(code.into())
}

fn is_bare_toml_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn strip_toml_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote = None;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let mut slashes = 0_usize;
        let mut prior = index;
        while prior > 0 && bytes[prior - 1] == b'\\' {
            slashes += 1;
            prior -= 1;
        }
        match (quote, byte) {
            (None, b'\'') | (None, b'"') => quote = Some(byte),
            (Some(b'\''), b'\'') => quote = None,
            (Some(b'"'), b'"') if slashes % 2 == 0 => quote = None,
            _ => {}
        }
        if byte == b'#' && quote.is_none() {
            return &line[..index];
        }
    }
    line
}

fn direct_string_control_invalid(character: char) -> bool {
    matches!(character as u32, 0x00..=0x08 | 0x0a..=0x1f | 0x7f)
}

fn parse_basic_toml_string(raw: &str) -> RetrievalResult<String> {
    if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
        return Err(invalid_config("GKOS_CONFIG_BASIC_STRING_INVALID"));
    }
    let closing = raw.len() - 1;
    let mut index = 1_usize;
    let mut output = String::new();
    while index < closing {
        let character = raw[index..]
            .chars()
            .next()
            .ok_or_else(|| invalid_config("GKOS_CONFIG_BASIC_STRING_INVALID"))?;
        if character == '"' {
            return Err(invalid_config("GKOS_CONFIG_BASIC_STRING_INVALID"));
        }
        if character != '\\' {
            if direct_string_control_invalid(character) {
                return Err(invalid_config("GKOS_CONFIG_STRING_CONTROL_INVALID"));
            }
            output.push(character);
            index += character.len_utf8();
            continue;
        }

        index += 1;
        if index >= closing {
            return Err(invalid_config("GKOS_CONFIG_BASIC_STRING_ESCAPE_INVALID"));
        }
        let escape = raw.as_bytes()[index];
        index += 1;
        match escape {
            b'b' => output.push('\u{8}'),
            b't' => output.push('\t'),
            b'n' => output.push('\n'),
            b'f' => output.push('\u{c}'),
            b'r' => output.push('\r'),
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'u' | b'U' => {
                let width = if escape == b'u' { 4 } else { 8 };
                let end = index
                    .checked_add(width)
                    .ok_or_else(|| invalid_config("GKOS_CONFIG_UNICODE_ESCAPE_INVALID"))?;
                if end > closing {
                    return Err(invalid_config("GKOS_CONFIG_UNICODE_ESCAPE_INVALID"));
                }
                let digits = &raw.as_bytes()[index..end];
                if !digits.iter().all(u8::is_ascii_hexdigit) {
                    return Err(invalid_config("GKOS_CONFIG_UNICODE_ESCAPE_INVALID"));
                }
                let digits = std::str::from_utf8(digits)
                    .map_err(|_| invalid_config("GKOS_CONFIG_UNICODE_ESCAPE_INVALID"))?;
                let codepoint = u32::from_str_radix(digits, 16)
                    .map_err(|_| invalid_config("GKOS_CONFIG_UNICODE_ESCAPE_INVALID"))?;
                let character = char::from_u32(codepoint)
                    .ok_or_else(|| invalid_config("GKOS_CONFIG_UNICODE_ESCAPE_INVALID"))?;
                output.push(character);
                index = end;
            }
            _ => {
                return Err(invalid_config("GKOS_CONFIG_BASIC_STRING_ESCAPE_INVALID"));
            }
        }
    }
    Ok(output)
}

fn parse_literal_toml_string(raw: &str) -> RetrievalResult<String> {
    if raw.len() < 2 || !raw.starts_with('\'') || !raw.ends_with('\'') {
        return Err(invalid_config("GKOS_CONFIG_LITERAL_STRING_INVALID"));
    }
    let inner = &raw[1..raw.len() - 1];
    if inner.contains('\'') {
        return Err(invalid_config("GKOS_CONFIG_LITERAL_STRING_INVALID"));
    }
    if inner.chars().any(direct_string_control_invalid) {
        return Err(invalid_config("GKOS_CONFIG_STRING_CONTROL_INVALID"));
    }
    Ok(inner.to_owned())
}

fn is_plain_integer(value: &str) -> bool {
    let unsigned = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    !unsigned.is_empty()
        && unsigned.bytes().all(|byte| byte.is_ascii_digit())
        && (unsigned == "0" || !unsigned.starts_with('0'))
}

fn is_plain_decimal(value: &str) -> bool {
    let unsigned = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    let mut pieces = unsigned.split('.');
    let integer = pieces.next().unwrap_or_default();
    let fraction = pieces.next();
    if pieces.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer != "0" && integer.starts_with('0'))
    {
        return false;
    }
    fraction
        .is_none_or(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
}

fn skip_toml_whitespace(value: &str, mut offset: usize) -> usize {
    while let Some(character) = value[offset..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        offset += character.len_utf8();
    }
    offset
}

fn find_array_string_end(value: &str, offset: usize) -> Option<usize> {
    let quote = *value.as_bytes().get(offset)?;
    let mut index = offset + 1;
    while index < value.len() {
        let byte = value.as_bytes()[index];
        if quote == b'\'' && byte == b'\'' {
            return Some(index);
        }
        if quote == b'"' && byte == b'"' {
            let mut slashes = 0_usize;
            let mut prior = index;
            while prior > offset + 1 && value.as_bytes()[prior - 1] == b'\\' {
                slashes += 1;
                prior -= 1;
            }
            if slashes % 2 == 0 {
                return Some(index);
            }
        }
        index += value[index..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

fn parse_toml_subset_value(raw: &str) -> RetrievalResult<Value> {
    let value = raw.trim();
    if value.starts_with('"') {
        return Ok(Value::String(parse_basic_toml_string(value)?));
    }
    if value.starts_with('\'') {
        return Ok(Value::String(parse_literal_toml_string(value)?));
    }
    if value == "true" || value == "false" {
        return Ok(Value::Bool(value == "true"));
    }
    if is_plain_decimal(value) {
        let numeric = value
            .parse::<f64>()
            .map_err(|_| invalid_config("GKOS_CONFIG_NUMBER_INVALID"))?;
        if !numeric.is_finite() || (numeric.fract() == 0.0 && numeric.abs() > MAX_SAFE_INTEGER) {
            return Err(invalid_config("GKOS_CONFIG_NUMBER_INVALID"));
        }
        let number = if is_plain_integer(value) {
            if numeric == 0.0 {
                serde_json::Number::from(0)
            } else if numeric.is_sign_negative() {
                serde_json::Number::from(
                    value
                        .parse::<i64>()
                        .map_err(|_| invalid_config("GKOS_CONFIG_NUMBER_INVALID"))?,
                )
            } else {
                serde_json::Number::from(
                    value
                        .trim_start_matches('+')
                        .parse::<u64>()
                        .map_err(|_| invalid_config("GKOS_CONFIG_NUMBER_INVALID"))?,
                )
            }
        } else {
            serde_json::Number::from_f64(numeric)
                .ok_or_else(|| invalid_config("GKOS_CONFIG_NUMBER_INVALID"))?
        };
        return Ok(Value::Number(number));
    }
    if value.starts_with('[') && value.ends_with(']') {
        let inner = value[1..value.len() - 1].trim();
        if inner.is_empty() {
            return Ok(Value::Array(Vec::new()));
        }
        let mut output = Vec::new();
        let mut offset = 0_usize;
        while offset < inner.len() {
            offset = skip_toml_whitespace(inner, offset);
            let quote = *inner
                .as_bytes()
                .get(offset)
                .ok_or_else(|| invalid_config("GKOS_CONFIG_ARRAY_INVALID"))?;
            if !matches!(quote, b'"' | b'\'') {
                return Err(invalid_config("GKOS_CONFIG_ARRAY_INVALID"));
            }
            let end = find_array_string_end(inner, offset)
                .ok_or_else(|| invalid_config("GKOS_CONFIG_ARRAY_INVALID"))?;
            let item = &inner[offset..=end];
            output.push(Value::String(if quote == b'"' {
                parse_basic_toml_string(item)?
            } else {
                parse_literal_toml_string(item)?
            }));
            offset = skip_toml_whitespace(inner, end + 1);
            if offset == inner.len() {
                break;
            }
            if inner.as_bytes()[offset] != b',' {
                return Err(invalid_config("GKOS_CONFIG_ARRAY_INVALID"));
            }
            offset += 1;
            if inner[offset..].trim().is_empty() {
                return Err(invalid_config(
                    "GKOS_CONFIG_ARRAY_TRAILING_COMMA_UNSUPPORTED",
                ));
            }
        }
        return Ok(Value::Array(output));
    }
    Err(invalid_config("GKOS_CONFIG_VALUE_UNSUPPORTED"))
}

fn integer_config_coordinate(coordinate: &str) -> bool {
    matches!(
        coordinate,
        "config_version"
            | "service.port"
            | "agents.activity_retention_days"
            | "retrieval.max_tokens"
            | "retrieval.overlap_tokens"
            | "retrieval.parent_expansion_max_child_tokens"
            | "retrieval.rrf_k"
            | "retrieval.lexical_top_k"
            | "retrieval.semantic_top_k"
            | "vectors.dimensions"
            | "vectors.timeout_ms"
            | "reranker.timeout_ms"
            | "watcher.debounce_ms"
    )
}

fn known_config_key(section: &str, key: &str) -> bool {
    match section {
        "" => matches!(key, "config_version"),
        "service" => matches!(key, "transport" | "host" | "port" | "legacy_rest"),
        "agents" => matches!(
            key,
            "multi_agent" | "require_authentication" | "activity_retention_days"
        ),
        "retrieval" => matches!(
            key,
            "mode"
                | "max_tokens"
                | "overlap_tokens"
                | "parent_expansion"
                | "parent_expansion_max_child_tokens"
                | "mmr"
                | "mmr_lambda"
                | "rrf_k"
                | "lexical_top_k"
                | "semantic_top_k"
                | "path_include"
                | "path_exclude"
        ),
        "vectors" => matches!(
            key,
            "enabled"
                | "provider"
                | "provider_id"
                | "model_id"
                | "dimensions"
                | "timeout_ms"
                | "endpoint"
                | "token_env"
                | "model_path"
                | "server"
                | "tool"
        ),
        "reranker" => matches!(
            key,
            "enabled"
                | "provider"
                | "provider_id"
                | "model_id"
                | "timeout_ms"
                | "endpoint"
                | "token_env"
                | "model_path"
                | "server"
                | "tool"
        ),
        "graph" => matches!(
            key,
            "sqlite_projection" | "graphiti_projection" | "similarity_projection"
        ),
        "watcher" => matches!(key, "enabled" | "debounce_ms" | "startup_reconciliation"),
        _ => false,
    }
}

fn boolean_config_coordinate(coordinate: &str) -> bool {
    matches!(
        coordinate,
        "service.legacy_rest"
            | "agents.multi_agent"
            | "agents.require_authentication"
            | "retrieval.parent_expansion"
            | "retrieval.mmr"
            | "vectors.enabled"
            | "reranker.enabled"
            | "graph.sqlite_projection"
            | "graph.graphiti_projection"
            | "graph.similarity_projection"
            | "watcher.enabled"
            | "watcher.startup_reconciliation"
    )
}

fn number_config_coordinate(coordinate: &str) -> bool {
    integer_config_coordinate(coordinate) || coordinate == "retrieval.mmr_lambda"
}

fn array_config_coordinate(coordinate: &str) -> bool {
    matches!(
        coordinate,
        "retrieval.path_include" | "retrieval.path_exclude"
    )
}

fn parse_gkos_toml_subset(text: &str) -> RetrievalResult<Value> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut document = TomlSubsetDocument::new();
    document.insert(String::new(), BTreeMap::new());
    let mut declared_sections = BTreeSet::new();
    let mut section = String::new();

    for (line_index, source) in text.split('\n').enumerate() {
        let source = source.strip_suffix('\r').unwrap_or(source);
        let line = strip_toml_comment(source).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let heading = &line[1..line.len() - 1];
            if is_bare_toml_name(heading) {
                if !declared_sections.insert(heading.to_owned()) {
                    return Err(invalid_config(format!(
                        "GKOS_CONFIG_DUPLICATE_SECTION:{heading}"
                    )));
                }
                section = heading.to_owned();
                document.entry(section.clone()).or_default();
                continue;
            }
        }

        let Some((key, raw)) = line.split_once('=') else {
            return Err(invalid_config(format!(
                "GKOS_CONFIG_SYNTAX_ERROR:{}",
                line_index + 1
            )));
        };
        let key = key.trim_end();
        let raw = raw.trim_start();
        if !is_bare_toml_name(key) || raw.is_empty() {
            return Err(invalid_config(format!(
                "GKOS_CONFIG_SYNTAX_ERROR:{}",
                line_index + 1
            )));
        }
        let values = document.entry(section.clone()).or_default();
        if values.contains_key(key) {
            return Err(invalid_config(format!(
                "GKOS_CONFIG_DUPLICATE_KEY:{section}.{key}"
            )));
        }
        let value = parse_toml_subset_value(raw)?;
        let coordinate = if section.is_empty() {
            key.to_owned()
        } else {
            format!("{section}.{key}")
        };
        if integer_config_coordinate(&coordinate)
            && value.is_number()
            && !is_plain_integer(raw.trim())
        {
            return Err(invalid_config(format!(
                "GKOS_CONFIG_INTEGER_SYNTAX_INVALID:{coordinate}"
            )));
        }
        values.insert(key.to_owned(), value);
    }

    if document
        .get("")
        .and_then(|root| root.get("config_version"))
        .and_then(Value::as_u64)
        != Some(u64::from(GKOS_HOST_CONFIG_VERSION))
    {
        return Err(invalid_config("GKOS_CONFIG_VERSION_UNSUPPORTED"));
    }

    for (section, values) in &document {
        if !matches!(
            section.as_str(),
            "" | "service" | "agents" | "retrieval" | "vectors" | "reranker" | "graph" | "watcher"
        ) {
            return Err(invalid_config(format!(
                "GKOS_CONFIG_SECTION_UNKNOWN:{section}"
            )));
        }
        for (key, value) in values {
            if !known_config_key(section, key) {
                return Err(invalid_config(format!(
                    "GKOS_CONFIG_KEY_UNKNOWN:{section}.{key}"
                )));
            }
            let coordinate = if section.is_empty() {
                key.clone()
            } else {
                format!("{section}.{key}")
            };
            if coordinate == "config_version" {
                continue;
            }
            let type_matches = if boolean_config_coordinate(&coordinate) {
                value.is_boolean()
            } else if number_config_coordinate(&coordinate) {
                value.is_number()
            } else if array_config_coordinate(&coordinate) {
                value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string))
            } else {
                value.is_string()
            };
            if !type_matches {
                return Err(invalid_config(format!(
                    "GKOS_CONFIG_TYPE_INVALID:{coordinate}"
                )));
            }
        }
    }

    serde_json::to_value(document)
        .map_err(|error| invalid_config(format!("GKOS_CONFIG_INVALID:{error}")))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustedConfigProvenance {
    Explicit,
    TrustedCwd,
    WorkspaceRoot,
    User,
}

#[derive(Clone, Debug, Default)]
pub struct TrustedConfigDiscoveryOptions {
    pub explicit_config: Option<PathBuf>,
    pub trust_cwd: bool,
    pub cwd: Option<PathBuf>,
    pub workspace_root: Option<PathBuf>,
    pub vault_root: Option<PathBuf>,
    pub user_config_path: Option<PathBuf>,
}

/// A host configuration can only be created by the reviewed filesystem
/// discovery path. Callers cannot self-assert trusted provenance in provider
/// JSON or a vault-local note.
pub struct TrustedGkosConfig {
    path: PathBuf,
    provenance: TrustedConfigProvenance,
    document: HostConfigDocument,
    configuration_digest: String,
}

impl TrustedGkosConfig {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn provenance(&self) -> TrustedConfigProvenance {
        self.provenance
    }

    pub fn configuration_digest(&self) -> &str {
        &self.configuration_digest
    }

    pub(crate) fn vector_config(&self) -> Option<&VectorProviderConfig> {
        match self.document.vectors.as_ref()? {
            VectorProviderSection::Disabled(_) => None,
            VectorProviderSection::Enabled(config) => Some(config),
        }
    }

    pub(crate) fn rerank_config(&self) -> Option<&RerankProviderConfig> {
        match self.document.reranker.as_ref()? {
            RerankProviderSection::Disabled(_) => None,
            RerankProviderSection::Enabled(config) => Some(config),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostConfigDocument {
    config_version: u32,
    service: Option<ServiceSection>,
    agents: Option<AgentsSection>,
    retrieval: Option<RetrievalSection>,
    vectors: Option<VectorProviderSection>,
    reranker: Option<RerankProviderSection>,
    graph: Option<GraphSection>,
    watcher: Option<WatcherSection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ServiceSection {
    transport: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    legacy_rest: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct AgentsSection {
    multi_agent: Option<bool>,
    require_authentication: Option<bool>,
    activity_retention_days: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct RetrievalSection {
    mode: Option<String>,
    max_tokens: Option<u32>,
    overlap_tokens: Option<u32>,
    parent_expansion: Option<bool>,
    parent_expansion_max_child_tokens: Option<u32>,
    mmr: Option<bool>,
    mmr_lambda: Option<f64>,
    rrf_k: Option<u64>,
    lexical_top_k: Option<u32>,
    semantic_top_k: Option<u32>,
    path_include: Option<Vec<String>>,
    path_exclude: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisabledProvider {
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum VectorProviderSection {
    Disabled(DisabledProvider),
    Enabled(VectorProviderConfig),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum RerankProviderSection {
    Disabled(DisabledProvider),
    Enabled(RerankProviderConfig),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct GraphSection {
    sqlite_projection: Option<bool>,
    graphiti_projection: Option<bool>,
    similarity_projection: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WatcherSection {
    enabled: Option<bool>,
    debounce_ms: Option<u64>,
    startup_reconciliation: Option<bool>,
}

impl HostConfigDocument {
    fn parse(text: &str) -> RetrievalResult<Self> {
        // Reject syntax outside the Full-owned gkos-toml-subset/1 before the
        // general TOML parser can normalize it into an apparently valid host
        // configuration.
        parse_gkos_toml_subset(text)?;
        let document: Self = toml::from_str(text).map_err(|error| {
            RetrievalError::InvalidConfig(format!("GKOS_CONFIG_INVALID:{error}"))
        })?;
        document.validate()?;
        Ok(document)
    }

    fn validate(&self) -> RetrievalResult<()> {
        if self.config_version != GKOS_HOST_CONFIG_VERSION {
            return Err(RetrievalError::InvalidConfig(
                "GKOS_CONFIG_VERSION_UNSUPPORTED".to_owned(),
            ));
        }
        if self
            .retrieval
            .as_ref()
            .and_then(|section| section.parent_expansion_max_child_tokens)
            .is_some_and(|value| !(1..=4096).contains(&value))
        {
            return Err(RetrievalError::InvalidConfig(
                "GKOS_CONFIG_VALUE_INVALID:retrieval.parent_expansion_max_child_tokens".to_owned(),
            ));
        }
        if self
            .retrieval
            .as_ref()
            .and_then(|section| section.mmr_lambda)
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(RetrievalError::InvalidConfig(
                "GKOS_CONFIG_VALUE_INVALID:retrieval.mmr_lambda".to_owned(),
            ));
        }
        validate_vector_section(self.vectors.as_ref())?;
        validate_rerank_section(self.reranker.as_ref())?;
        Ok(())
    }

    fn is_vault_safe(&self) -> bool {
        if self.service.is_some()
            || self.agents.is_some()
            || self.vectors.is_some()
            || self.reranker.is_some()
            || self.graph.is_some()
            || self.watcher.is_some()
        {
            return false;
        }
        self.retrieval.as_ref().is_none_or(|section| {
            section.mode.is_none()
                && section.max_tokens.is_none()
                && section.overlap_tokens.is_none()
        })
    }
}

fn validate_vector_section(section: Option<&VectorProviderSection>) -> RetrievalResult<()> {
    match section {
        None => Ok(()),
        Some(VectorProviderSection::Disabled(disabled)) if !disabled.enabled => Ok(()),
        Some(VectorProviderSection::Disabled(_)) => Err(RetrievalError::InvalidConfig(
            "GKOS_CONFIG_PROVIDER_DISABLED_INVALID:vectors".to_owned(),
        )),
        Some(VectorProviderSection::Enabled(config)) => config.validate(),
    }
}

fn validate_rerank_section(section: Option<&RerankProviderSection>) -> RetrievalResult<()> {
    match section {
        None => Ok(()),
        Some(RerankProviderSection::Disabled(disabled)) if !disabled.enabled => Ok(()),
        Some(RerankProviderSection::Disabled(_)) => Err(RetrievalError::InvalidConfig(
            "GKOS_CONFIG_PROVIDER_DISABLED_INVALID:reranker".to_owned(),
        )),
        Some(RerankProviderSection::Enabled(config)) => config.validate(),
    }
}

pub fn discover_trusted_gkos_config(
    options: &TrustedConfigDiscoveryOptions,
) -> RetrievalResult<Option<TrustedGkosConfig>> {
    let cwd = absolute_path(options.cwd.as_deref().unwrap_or(Path::new(".")))?;
    let vault_root = options
        .vault_root
        .as_deref()
        .map(absolute_path)
        .transpose()?;
    let workspace_was_explicit = options.workspace_root.is_some();
    let workspace = match options.workspace_root.as_deref() {
        Some(path) => Some(absolute_path(path)?),
        None => find_workspace_root(&cwd),
    };
    let explicit = options
        .explicit_config
        .as_deref()
        .map(absolute_path)
        .transpose()?;
    if explicit.as_ref().is_some_and(|path| !entry_exists(path)) {
        return Err(RetrievalError::InvalidConfig(
            "EXPLICIT_GKOS_CONFIG_NOT_FOUND".to_owned(),
        ));
    }
    let user = options
        .user_config_path
        .clone()
        .unwrap_or_else(default_user_config_path);
    let workspace_config = workspace.map(|root| root.join("gkos.toml"));
    // A Git repository inside a vault is still vault-controlled input. Merely
    // discovering its `.git` marker must not promote its provider, credential,
    // service, or executable routing keys to trusted host configuration. An
    // operator can still authorize that exact workspace through the explicit
    // `workspace_root` option (or the higher-precedence explicit/CWD choices).
    let workspace_config = if !workspace_was_explicit
        && workspace_config.as_ref().is_some_and(|candidate| {
            vault_root
                .as_ref()
                .is_some_and(|vault| contained_path(candidate, vault))
        }) {
        None
    } else {
        workspace_config
    };
    let candidates = [
        (explicit, TrustedConfigProvenance::Explicit),
        (
            options.trust_cwd.then(|| cwd.join("gkos.toml")),
            TrustedConfigProvenance::TrustedCwd,
        ),
        (workspace_config, TrustedConfigProvenance::WorkspaceRoot),
        (Some(user), TrustedConfigProvenance::User),
    ];
    let mut selected = None;
    for (candidate, provenance) in candidates {
        let Some(candidate) = candidate else { continue };
        if entry_exists(&candidate) {
            selected = Some(read_trusted_config(&candidate, provenance)?);
            break;
        }
    }
    if let Some(vault_root) = vault_root.as_deref() {
        reject_unsafe_vault_config(vault_root, selected.as_ref().map(TrustedGkosConfig::path))?;
    }
    Ok(selected)
}

pub fn reject_unsafe_vault_config(
    vault_root: &Path,
    selected_trusted_path: Option<&Path>,
) -> RetrievalResult<()> {
    let candidate = absolute_path(vault_root)?.join("gkos.toml");
    if !entry_exists(&candidate)
        || selected_trusted_path.is_some_and(|selected| equivalent_paths(selected, &candidate))
    {
        return Ok(());
    }
    let (path, text) = read_config_file(&candidate)?;
    let document = HostConfigDocument::parse(&text)?;
    if !document.is_vault_safe() {
        return Err(RetrievalError::InvalidConfig(format!(
            "UNTRUSTED_VAULT_CONFIG_SHADOWING_REJECTED:{}",
            path.display()
        )));
    }
    Ok(())
}

fn read_trusted_config(
    path: &Path,
    provenance: TrustedConfigProvenance,
) -> RetrievalResult<TrustedGkosConfig> {
    let (path, text) = read_config_file(path)?;
    let document = HostConfigDocument::parse(&text)?;
    let configuration_digest = full_parity_configuration_digest(&text)?;
    Ok(TrustedGkosConfig {
        path,
        provenance,
        document,
        configuration_digest,
    })
}

fn full_parity_configuration_digest(text: &str) -> RetrievalResult<String> {
    canonical_digest(&parse_gkos_toml_subset(text)?)
}

fn read_config_file(path: &Path) -> RetrievalResult<(PathBuf, String)> {
    let path = absolute_path(path)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_CONFIG_BYTES
        || config_link_count(&path, &metadata)? > 1
    {
        return Err(RetrievalError::InvalidConfig(
            "GKOS_CONFIG_ALIAS_OR_SIZE_REJECTED".to_owned(),
        ));
    }
    let canonical = fs::canonicalize(&path)?;
    if !equivalent_paths(&canonical, &path) {
        return Err(RetrievalError::InvalidConfig(
            "GKOS_CONFIG_ALIAS_REJECTED".to_owned(),
        ));
    }
    let bytes = fs::read(&canonical)?;
    if bytes.len() > MAX_CONFIG_BYTES as usize {
        return Err(RetrievalError::InvalidConfig(
            "GKOS_CONFIG_SIZE_REJECTED".to_owned(),
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| RetrievalError::InvalidConfig("GKOS_CONFIG_UTF8_INVALID".to_owned()))?;
    Ok((path, text))
}

fn absolute_path(path: &Path) -> RetrievalResult<PathBuf> {
    Ok(absolute_lexical_path(path)?)
}

fn entry_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if entry_exists(&current.join(".git")) {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn default_user_config_path() -> PathBuf {
    if let Some(value) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(value).join("gkos").join("gkos.toml");
    }
    if cfg!(windows) {
        if let Some(value) = env::var_os("APPDATA") {
            return PathBuf::from(value).join("GKOS").join("gkos.toml");
        }
    }
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("gkos")
        .join("gkos.toml")
}

#[cfg(unix)]
fn config_link_count(_path: &Path, metadata: &fs::Metadata) -> RetrievalResult<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink())
}

#[cfg(windows)]
fn config_link_count(path: &Path, _metadata: &fs::Metadata) -> RetrievalResult<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a valid handle throughout this call and the output
    // pointer refers to initialized writable storage of the required type.
    let success =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
    if success == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(u64::from(information.nNumberOfLinks))
}

#[cfg(not(any(unix, windows)))]
fn config_link_count(_path: &Path, _metadata: &fs::Metadata) -> RetrievalResult<u64> {
    Ok(1)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::Deserialize;
    use tempfile::tempdir;

    use super::*;

    const PROVIDERS: &str = r#"config_version = 1

[vectors]
enabled = true
provider = "openai_compatible"
provider_id = "operator-any"
model_id = "model-any"
dimensions = 3
endpoint = "https://arbitrary.operator.invalid/v1/embeddings"
token_env = "GKOS_TEST_TOKEN"
timeout_ms = 1000

[reranker]
enabled = true
provider = "mcp"
provider_id = "operator-rerank"
model_id = "rerank-any"
server = "operator-mcp"
tool = "custom.rerank"
"#;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TomlLexicalFixture {
        contract_version: String,
        profile: String,
        accepted: Vec<TomlAcceptedCase>,
        rejected: Vec<TomlRejectedCase>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TomlAcceptedCase {
        id: String,
        text: String,
        canonical_document: String,
        configuration_digest: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TomlRejectedCase {
        id: String,
        text: String,
        error_code: String,
    }

    #[test]
    fn strict_toml_subset_matches_full_accept_reject_and_digest_fixture() {
        let fixture: TomlLexicalFixture = serde_json::from_slice(include_bytes!(concat!(
            "../../../contracts/gkos-retrieval-1.0.0-draft.1/",
            "gkos-toml-lexical-fixture.json"
        )))
        .unwrap();
        assert_eq!(
            fixture.contract_version,
            crate::contract::RETRIEVAL_CONTRACT
        );
        assert_eq!(fixture.profile, "gkos-toml-subset/1");

        for case in fixture.accepted {
            let document = parse_gkos_toml_subset(&case.text).unwrap();
            assert_eq!(
                crate::digest::canonical_json(&document).unwrap(),
                case.canonical_document,
                "{}",
                case.id
            );
            assert_eq!(
                canonical_digest(&document).unwrap(),
                case.configuration_digest,
                "{}",
                case.id
            );
            HostConfigDocument::parse(&case.text)
                .unwrap_or_else(|error| panic!("{}: {error}", case.id));
        }

        for case in fixture.rejected {
            let error = parse_gkos_toml_subset(&case.text)
                .expect_err(&format!("{} must be rejected", case.id));
            assert!(
                error.to_string().contains(&case.error_code),
                "{}: expected {}, received {error}",
                case.id,
                case.error_code
            );
            let typed_error = HostConfigDocument::parse(&case.text)
                .expect_err(&format!("{} must fail before typed parsing", case.id));
            assert!(
                typed_error.to_string().contains(&case.error_code),
                "{} typed: expected {}, received {typed_error}",
                case.id,
                case.error_code
            );
        }
    }

    #[test]
    fn explicit_safe_file_creates_nonforgeable_trusted_provider_selection() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("operator.toml");
        fs::write(&path, PROVIDERS).unwrap();
        let selected = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            explicit_config: Some(path.clone()),
            user_config_path: Some(directory.path().join("missing-user.toml")),
            ..TrustedConfigDiscoveryOptions::default()
        })
        .unwrap()
        .unwrap();
        assert!(equivalent_paths(selected.path(), &path));
        assert_eq!(selected.provenance(), TrustedConfigProvenance::Explicit);
        assert_eq!(
            selected.configuration_digest(),
            "sha256:88e27f8c715968356ef4480e8b943617cc796f63792af2e25eff7f2d4d54df84"
        );
        assert!(matches!(
            selected.vector_config(),
            Some(VectorProviderConfig::OpenaiCompatible { token_env, .. })
                if token_env.as_deref() == Some("GKOS_TEST_TOKEN")
        ));
        assert!(matches!(
            selected.rerank_config(),
            Some(RerankProviderConfig::Mcp { tool, .. }) if tool == "custom.rerank"
        ));
    }

    #[test]
    fn schema_invalid_or_untrusted_provider_shadowing_fails_closed() {
        let directory = tempdir().unwrap();
        let invalid = directory.path().join("invalid.toml");
        fs::write(
            &invalid,
            PROVIDERS.replace(
                "provider = \"openai_compatible\"",
                "kind = \"openai_compatible\"",
            ),
        )
        .unwrap();
        assert!(
            discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
                explicit_config: Some(invalid),
                user_config_path: Some(directory.path().join("missing-user.toml")),
                ..TrustedConfigDiscoveryOptions::default()
            })
            .is_err()
        );

        let vault = directory.path().join("vault");
        fs::create_dir(&vault).unwrap();
        fs::write(vault.join("gkos.toml"), PROVIDERS).unwrap();
        assert!(reject_unsafe_vault_config(&vault, None).is_err());
        fs::write(
            vault.join("gkos.toml"),
            "config_version = 1\n[retrieval]\nmmr = true\npath_include = [\"notes/**\"]\n",
        )
        .unwrap();
        reject_unsafe_vault_config(&vault, None).unwrap();
    }

    #[test]
    fn auto_discovered_git_config_inside_vault_never_becomes_trusted_host_routing() {
        let directory = tempdir().unwrap();
        let vault = directory.path().join("vault");
        fs::create_dir(&vault).unwrap();
        fs::create_dir(vault.join(".git")).unwrap();
        fs::write(vault.join("gkos.toml"), PROVIDERS).unwrap();

        let equal_root = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            cwd: Some(vault.clone()),
            vault_root: Some(vault.clone()),
            user_config_path: Some(directory.path().join("missing-user.toml")),
            ..TrustedConfigDiscoveryOptions::default()
        });
        assert!(matches!(
            equal_root,
            Err(RetrievalError::InvalidConfig(message))
                if message.starts_with("UNTRUSTED_VAULT_CONFIG_SHADOWING_REJECTED:")
        ));

        fs::remove_file(vault.join("gkos.toml")).unwrap();
        let nested = vault.join("nested-repository");
        fs::create_dir(&nested).unwrap();
        fs::create_dir(nested.join(".git")).unwrap();
        fs::write(nested.join("gkos.toml"), PROVIDERS).unwrap();
        let user = directory.path().join("user.toml");
        fs::write(&user, "config_version = 1\n").unwrap();

        let selected = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            cwd: Some(nested.clone()),
            vault_root: Some(vault.clone()),
            user_config_path: Some(user.clone()),
            ..TrustedConfigDiscoveryOptions::default()
        })
        .unwrap()
        .unwrap();
        assert_eq!(selected.provenance(), TrustedConfigProvenance::User);
        assert!(equivalent_paths(selected.path(), &user));

        let explicitly_trusted = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            cwd: Some(nested.clone()),
            workspace_root: Some(nested),
            vault_root: Some(vault),
            user_config_path: Some(directory.path().join("missing-user.toml")),
            ..TrustedConfigDiscoveryOptions::default()
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            explicitly_trusted.provenance(),
            TrustedConfigProvenance::WorkspaceRoot
        );
    }

    #[test]
    fn auto_discovered_workspace_outside_nested_vault_remains_trusted() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("operator-workspace");
        let vault = workspace.join("vault");
        let cwd = vault.join("notes");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir(workspace.join(".git")).unwrap();
        fs::write(workspace.join("gkos.toml"), PROVIDERS).unwrap();

        let selected = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            cwd: Some(cwd),
            vault_root: Some(vault),
            user_config_path: Some(directory.path().join("missing-user.toml")),
            ..TrustedConfigDiscoveryOptions::default()
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            selected.provenance(),
            TrustedConfigProvenance::WorkspaceRoot
        );
        assert!(equivalent_paths(
            selected.path(),
            &workspace.join("gkos.toml")
        ));
    }

    #[test]
    fn configuration_digest_preserves_omitted_and_disabled_sections_exactly() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("disabled.toml");
        fs::write(&path, "config_version = 1\n[vectors]\nenabled = false\n").unwrap();
        let selected = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            explicit_config: Some(path),
            user_config_path: Some(directory.path().join("missing-user.toml")),
            ..TrustedConfigDiscoveryOptions::default()
        })
        .unwrap()
        .unwrap();
        // Full hashes the presence-preserving parsed document
        // {"":{"config_version":1},"vectors":{"enabled":false}}.
        assert_eq!(
            selected.configuration_digest(),
            "sha256:d647a366e7cfe7ee8f17c6a1b41a837c1f1db154649f7eb2f494ddb8bf4d8130"
        );
        assert!(selected.vector_config().is_none());
        assert!(selected.rerank_config().is_none());
    }

    #[test]
    fn ordinary_dot_and_parent_components_are_normalized_before_alias_checks() {
        let directory = tempdir().unwrap();
        fs::create_dir(directory.path().join("configs")).unwrap();
        let path = directory.path().join("operator.toml");
        fs::write(&path, "config_version = 1\n").unwrap();
        let spelling = directory
            .path()
            .join("configs")
            .join("..")
            .join("operator.toml");
        let selected = discover_trusted_gkos_config(&TrustedConfigDiscoveryOptions {
            explicit_config: Some(spelling),
            user_config_path: Some(directory.path().join("missing-user.toml")),
            ..TrustedConfigDiscoveryOptions::default()
        })
        .unwrap()
        .unwrap();
        assert!(equivalent_paths(selected.path(), &path));
    }
}
