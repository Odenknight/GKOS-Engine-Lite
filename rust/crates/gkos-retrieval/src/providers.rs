use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_timer::Delay;
use futures_util::future::{select, Either};
use serde::{Deserialize, Serialize};

use crate::contract::{
    is_sha256_digest, RetrievalProviderStageKind, RetrievalProviderStageStatus, RetrievalStageState,
};
use crate::digest::canonical_digest;
use crate::{RetrievalError, RetrievalResult};

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = RetrievalResult<T>> + Send + 'a>>;
pub const DEFAULT_PROVIDER_TIMEOUT_MS: u32 = 15_000;
const MAX_VECTOR_DIMENSIONS: u32 = 1_000_000;
const MAX_PROVIDER_IDENTITY_BYTES: usize = 512;
const MAX_PROVIDER_LOCATION_BYTES: usize = 4_096;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorProviderConfig {
    OpenaiCompatible {
        enabled: bool,
        provider_id: String,
        model_id: String,
        dimensions: u32,
        endpoint: String,
        token_env: Option<String>,
        timeout_ms: Option<u32>,
    },
    LocalOnnx {
        enabled: bool,
        provider_id: String,
        model_id: String,
        dimensions: u32,
        model_path: String,
        timeout_ms: Option<u32>,
    },
    Mcp {
        enabled: bool,
        provider_id: String,
        model_id: String,
        dimensions: u32,
        server: String,
        tool: String,
        timeout_ms: Option<u32>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VectorProviderIdentity {
    pub kind: RetrievalProviderStageKind,
    pub provider_id: String,
    pub model_id: String,
    pub dimensions: u32,
    pub timeout_ms: u32,
    pub configuration_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum RerankProviderConfig {
    OpenaiCompatible {
        enabled: bool,
        provider_id: String,
        model_id: String,
        endpoint: String,
        token_env: Option<String>,
        timeout_ms: Option<u32>,
    },
    LocalOnnx {
        enabled: bool,
        provider_id: String,
        model_id: String,
        model_path: String,
        timeout_ms: Option<u32>,
    },
    Mcp {
        enabled: bool,
        provider_id: String,
        model_id: String,
        server: String,
        tool: String,
        timeout_ms: Option<u32>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RerankProviderIdentity {
    pub kind: RetrievalProviderStageKind,
    pub provider_id: String,
    pub model_id: String,
    pub timeout_ms: u32,
    pub configuration_digest: String,
}

pub trait ProviderStageIdentity {
    fn kind(&self) -> RetrievalProviderStageKind;
    fn provider_id(&self) -> &str;
    fn model_id(&self) -> &str;
}

impl ProviderStageIdentity for VectorProviderIdentity {
    fn kind(&self) -> RetrievalProviderStageKind {
        self.kind
    }
    fn provider_id(&self) -> &str {
        &self.provider_id
    }
    fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl ProviderStageIdentity for RerankProviderIdentity {
    fn kind(&self) -> RetrievalProviderStageKind {
        self.kind
    }
    fn provider_id(&self) -> &str {
        &self.provider_id
    }
    fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl VectorProviderConfig {
    pub fn validate(&self) -> RetrievalResult<()> {
        let (enabled, provider_id, model_id, dimensions, timeout) = match self {
            Self::OpenaiCompatible {
                enabled,
                provider_id,
                model_id,
                dimensions,
                endpoint,
                token_env,
                timeout_ms,
            } => {
                validate_http_endpoint(endpoint)?;
                if token_env
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(RetrievalError::InvalidConfig(
                        "token_env must be absent or nonempty".to_owned(),
                    ));
                }
                (*enabled, provider_id, model_id, *dimensions, *timeout_ms)
            }
            Self::LocalOnnx {
                enabled,
                provider_id,
                model_id,
                dimensions,
                model_path,
                timeout_ms,
            } => {
                require_bounded_nonempty(model_path, "model_path", MAX_PROVIDER_LOCATION_BYTES)?;
                (*enabled, provider_id, model_id, *dimensions, *timeout_ms)
            }
            Self::Mcp {
                enabled,
                provider_id,
                model_id,
                dimensions,
                server,
                tool,
                timeout_ms,
            } => {
                require_bounded_nonempty(server, "server", MAX_PROVIDER_IDENTITY_BYTES)?;
                require_bounded_nonempty(tool, "tool", MAX_PROVIDER_IDENTITY_BYTES)?;
                (*enabled, provider_id, model_id, *dimensions, *timeout_ms)
            }
        };
        if !enabled {
            return Err(RetrievalError::InvalidConfig(
                "an enabled provider section must set enabled=true".to_owned(),
            ));
        }
        require_bounded_nonempty(provider_id, "provider_id", MAX_PROVIDER_IDENTITY_BYTES)?;
        require_bounded_nonempty(model_id, "model_id", MAX_PROVIDER_IDENTITY_BYTES)?;
        if !(1..=MAX_VECTOR_DIMENSIONS).contains(&dimensions) {
            return Err(RetrievalError::InvalidConfig(
                "provider dimensions must be within [1, 1000000]".to_owned(),
            ));
        }
        if timeout.is_some_and(|value| value == 0 || value > 300_000) {
            return Err(RetrievalError::InvalidConfig(
                "provider timeout_ms must be within [1, 300000]".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn identity(&self) -> RetrievalResult<VectorProviderIdentity> {
        self.validate()?;
        let (kind, provider_id, model_id, dimensions, timeout_ms) = match self {
            Self::OpenaiCompatible {
                provider_id,
                model_id,
                dimensions,
                timeout_ms,
                ..
            } => (
                RetrievalProviderStageKind::OpenaiCompatible,
                provider_id,
                model_id,
                *dimensions,
                timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS),
            ),
            Self::LocalOnnx {
                provider_id,
                model_id,
                dimensions,
                timeout_ms,
                ..
            } => (
                RetrievalProviderStageKind::LocalOnnx,
                provider_id,
                model_id,
                *dimensions,
                timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS),
            ),
            Self::Mcp {
                provider_id,
                model_id,
                dimensions,
                timeout_ms,
                ..
            } => (
                RetrievalProviderStageKind::Mcp,
                provider_id,
                model_id,
                *dimensions,
                timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS),
            ),
        };
        Ok(VectorProviderIdentity {
            kind,
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
            dimensions,
            timeout_ms,
            configuration_digest: canonical_digest(self)?,
        })
    }
}

impl RerankProviderConfig {
    pub fn validate(&self) -> RetrievalResult<()> {
        let (enabled, provider_id, model_id, timeout) = match self {
            Self::OpenaiCompatible {
                enabled,
                provider_id,
                model_id,
                endpoint,
                token_env,
                timeout_ms,
            } => {
                validate_http_endpoint(endpoint)?;
                if token_env
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(RetrievalError::InvalidConfig(
                        "token_env must be absent or nonempty".to_owned(),
                    ));
                }
                (*enabled, provider_id, model_id, *timeout_ms)
            }
            Self::LocalOnnx {
                enabled,
                provider_id,
                model_id,
                model_path,
                timeout_ms,
            } => {
                require_bounded_nonempty(model_path, "model_path", MAX_PROVIDER_LOCATION_BYTES)?;
                (*enabled, provider_id, model_id, *timeout_ms)
            }
            Self::Mcp {
                enabled,
                provider_id,
                model_id,
                server,
                tool,
                timeout_ms,
            } => {
                require_bounded_nonempty(server, "server", MAX_PROVIDER_IDENTITY_BYTES)?;
                require_bounded_nonempty(tool, "tool", MAX_PROVIDER_IDENTITY_BYTES)?;
                (*enabled, provider_id, model_id, *timeout_ms)
            }
        };
        if !enabled {
            return Err(RetrievalError::InvalidConfig(
                "an enabled provider section must set enabled=true".to_owned(),
            ));
        }
        require_bounded_nonempty(provider_id, "provider_id", MAX_PROVIDER_IDENTITY_BYTES)?;
        require_bounded_nonempty(model_id, "model_id", MAX_PROVIDER_IDENTITY_BYTES)?;
        if timeout.is_some_and(|value| value == 0 || value > 300_000) {
            return Err(RetrievalError::InvalidConfig(
                "provider timeout_ms must be within [1, 300000]".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn identity(&self) -> RetrievalResult<RerankProviderIdentity> {
        self.validate()?;
        let (kind, provider_id, model_id, timeout_ms) = match self {
            Self::OpenaiCompatible {
                provider_id,
                model_id,
                timeout_ms,
                ..
            } => (
                RetrievalProviderStageKind::OpenaiCompatible,
                provider_id,
                model_id,
                timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS),
            ),
            Self::LocalOnnx {
                provider_id,
                model_id,
                timeout_ms,
                ..
            } => (
                RetrievalProviderStageKind::LocalOnnx,
                provider_id,
                model_id,
                timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS),
            ),
            Self::Mcp {
                provider_id,
                model_id,
                timeout_ms,
                ..
            } => (
                RetrievalProviderStageKind::Mcp,
                provider_id,
                model_id,
                timeout_ms.unwrap_or(DEFAULT_PROVIDER_TIMEOUT_MS),
            ),
        };
        Ok(RerankProviderIdentity {
            kind,
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
            timeout_ms,
            configuration_digest: canonical_digest(self)?,
        })
    }
}

pub fn validate_provider_timeout(timeout_ms: u32) -> RetrievalResult<()> {
    if (1..=300_000).contains(&timeout_ms) {
        Ok(())
    } else {
        Err(RetrievalError::InvalidConfig(
            "provider timeout_ms must be within [1, 300000]".to_owned(),
        ))
    }
}

fn validate_runtime_provider_identity_fields(
    kind: RetrievalProviderStageKind,
    provider_id: &str,
    model_id: &str,
    timeout_ms: u32,
    configuration_digest: &str,
) -> RetrievalResult<()> {
    if !matches!(
        kind,
        RetrievalProviderStageKind::OpenaiCompatible
            | RetrievalProviderStageKind::LocalOnnx
            | RetrievalProviderStageKind::Mcp
    ) {
        return Err(RetrievalError::InvalidConfig(
            "runtime provider identity must use a provider-capable family".to_owned(),
        ));
    }
    require_bounded_nonempty(provider_id, "provider_id", MAX_PROVIDER_IDENTITY_BYTES)?;
    require_bounded_nonempty(model_id, "model_id", MAX_PROVIDER_IDENTITY_BYTES)?;
    validate_provider_timeout(timeout_ms)?;
    if !is_sha256_digest(configuration_digest) {
        return Err(RetrievalError::InvalidConfig(
            "provider configuration_digest must be a lowercase sha256 digest".to_owned(),
        ));
    }
    Ok(())
}

/// Validate the complete provider-neutral vector identity before any cache,
/// source-text, or provider work. This checks contract shape only and does not
/// privilege or allowlist any host, vendor, route, model, or provider ID.
pub(crate) fn validate_vector_provider_identity(
    identity: &VectorProviderIdentity,
) -> RetrievalResult<()> {
    validate_runtime_provider_identity_fields(
        identity.kind,
        &identity.provider_id,
        &identity.model_id,
        identity.timeout_ms,
        &identity.configuration_digest,
    )?;
    if !(1..=MAX_VECTOR_DIMENSIONS).contains(&identity.dimensions) {
        return Err(RetrievalError::InvalidConfig(
            "provider dimensions must be within [1, 1000000]".to_owned(),
        ));
    }
    Ok(())
}

/// Validate the complete dimensionless reranker identity before any query or
/// candidate text crosses the configured provider boundary.
pub(crate) fn validate_rerank_provider_identity(
    identity: &RerankProviderIdentity,
) -> RetrievalResult<()> {
    validate_runtime_provider_identity_fields(
        identity.kind,
        &identity.provider_id,
        &identity.model_id,
        identity.timeout_ms,
        &identity.configuration_digest,
    )
}

/// Race one provider operation against its trusted-config deadline. Dropping
/// the losing provider future is Lite's cancellation boundary; adapters must
/// bind their transport/model work to that future's lifetime.
pub async fn bounded_provider_call<T>(
    operation: ProviderFuture<'_, T>,
    timeout_ms: u32,
    timeout_code: &'static str,
) -> RetrievalResult<T> {
    validate_provider_timeout(timeout_ms)?;
    let timeout = Delay::new(Duration::from_millis(u64::from(timeout_ms)));
    match select(operation, timeout).await {
        Either::Left((result, _timeout)) => result,
        Either::Right((_elapsed, _operation)) => {
            Err(RetrievalError::ProviderResponse(timeout_code.to_owned()))
        }
    }
}

fn require_bounded_nonempty(value: &str, field: &str, max_bytes: usize) -> RetrievalResult<()> {
    if value.trim().is_empty() {
        Err(RetrievalError::InvalidConfig(format!(
            "provider {field} must not be empty"
        )))
    } else if value.len() > max_bytes {
        Err(RetrievalError::InvalidConfig(format!(
            "provider {field} must not exceed {max_bytes} UTF-8 bytes"
        )))
    } else {
        Ok(())
    }
}

/// Validate the frozen provider-neutral OpenAI-compatible endpoint shape
/// without introducing a host, vendor, route, or model allowlist.
fn validate_http_endpoint(endpoint: &str) -> RetrievalResult<()> {
    require_bounded_nonempty(endpoint, "endpoint", MAX_PROVIDER_LOCATION_BYTES)?;
    if endpoint
        .chars()
        .any(|character| character.is_control() || character.is_whitespace() || character == '\\')
    {
        return Err(RetrievalError::InvalidConfig(
            "provider endpoint must be an absolute HTTP or HTTPS URI".to_owned(),
        ));
    }
    let remainder = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .ok_or_else(|| {
            RetrievalError::InvalidConfig(
                "provider endpoint must be an absolute HTTP or HTTPS URI".to_owned(),
            )
        })?;
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    // Credentials belong in the owner-selected secret resolver. Accepting
    // URI userinfo here would let a committed config bypass `token_env`.
    if authority.contains('@') {
        return Err(RetrievalError::InvalidConfig(
            "provider endpoint must not contain URI userinfo".to_owned(),
        ));
    }
    let host_and_port = authority;
    let valid_host = if let Some(bracketed) = host_and_port.strip_prefix('[') {
        bracketed.find(']').is_some_and(|end| {
            end > 0
                && valid_optional_port(&bracketed[end + 1..])
                && !bracketed[..end].contains(['[', ']'])
        })
    } else if host_and_port.matches(':').count() > 1 {
        false
    } else if let Some((host, port)) = host_and_port.rsplit_once(':') {
        !host.is_empty() && valid_port(port)
    } else {
        !host_and_port.is_empty()
    };
    if authority.is_empty()
        || host_and_port.is_empty()
        || host_and_port.contains(['/', '?', '#'])
        || !valid_host
    {
        return Err(RetrievalError::InvalidConfig(
            "provider endpoint must be an absolute HTTP or HTTPS URI with a host".to_owned(),
        ));
    }
    Ok(())
}

fn valid_optional_port(suffix: &str) -> bool {
    suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

pub trait VectorProvider: Send + Sync {
    fn identity(&self) -> &VectorProviderIdentity;
    fn embed<'a>(
        &'a self,
        request_id: &'a str,
        texts: &'a [String],
    ) -> ProviderFuture<'a, Vec<Vec<f32>>>;
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RerankInput {
    pub chunk_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RerankScore {
    pub chunk_id: String,
    pub score: f64,
}

pub trait RerankProvider: Send + Sync {
    fn identity(&self) -> &RerankProviderIdentity;
    fn rerank<'a>(
        &'a self,
        request_id: &'a str,
        query: &'a str,
        inputs: &'a [RerankInput],
    ) -> ProviderFuture<'a, Vec<RerankScore>>;
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingAdapterResponse {
    pub request_id: String,
    pub model_id: String,
    pub dimensions: u32,
    pub vectors: Vec<Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedEmbedding {
    pub index: u32,
    pub embedding: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiEmbeddingAdapterResponse {
    pub request_id: Option<String>,
    pub model_id: String,
    pub dimensions: Option<u32>,
    pub data: Option<Vec<IndexedEmbedding>>,
    pub vectors: Option<Vec<Vec<f64>>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedRerankScore {
    pub index: u32,
    pub score: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RerankAdapterResponse {
    pub request_id: String,
    pub model_id: String,
    pub scores: Vec<IndexedRerankScore>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiRerankAdapterResponse {
    pub request_id: Option<String>,
    pub model_id: String,
    pub scores: Vec<IndexedRerankScore>,
}

pub fn validate_embedding_response(
    response: EmbeddingAdapterResponse,
    expected_request_id: &str,
    identity: &VectorProviderIdentity,
    expected_count: usize,
) -> RetrievalResult<Vec<Vec<f32>>> {
    validate_correlation(
        Some(response.request_id.as_str()),
        expected_request_id,
        false,
        "embedding",
    )?;
    validate_embedding_vectors(
        response.model_id,
        response.dimensions,
        response.vectors,
        identity,
        expected_count,
    )
}

pub fn validate_openai_embedding_response(
    response: OpenAiEmbeddingAdapterResponse,
    expected_request_id: &str,
    identity: &VectorProviderIdentity,
    expected_count: usize,
) -> RetrievalResult<Vec<Vec<f32>>> {
    validate_correlation(
        response.request_id.as_deref(),
        expected_request_id,
        true,
        "embedding",
    )?;
    let vectors = match (response.data, response.vectors) {
        (Some(items), None) => {
            if items.len() != expected_count {
                return Err(RetrievalError::ProviderResponse(
                    "embedding response item count mismatch".to_owned(),
                ));
            }
            let mut indexed = vec![None; expected_count];
            for item in items {
                let index = item.index as usize;
                if index >= expected_count || indexed[index].replace(item.embedding).is_some() {
                    return Err(RetrievalError::ProviderResponse(
                        "embedding response index is invalid".to_owned(),
                    ));
                }
            }
            indexed
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    RetrievalError::ProviderResponse(
                        "embedding response index is missing".to_owned(),
                    )
                })?
        }
        (None, Some(vectors)) => vectors,
        _ => {
            return Err(RetrievalError::ProviderResponse(
                "embedding response must contain exactly one vector envelope".to_owned(),
            ));
        }
    };
    let dimensions = response
        .dimensions
        .or_else(|| {
            vectors
                .first()
                .and_then(|vector| u32::try_from(vector.len()).ok())
        })
        .ok_or_else(|| {
            RetrievalError::ProviderResponse("embedding response dimensions are missing".to_owned())
        })?;
    validate_embedding_vectors(
        response.model_id,
        dimensions,
        vectors,
        identity,
        expected_count,
    )
}

fn validate_embedding_vectors(
    model_id: String,
    dimensions: u32,
    vectors: Vec<Vec<f64>>,
    identity: &VectorProviderIdentity,
    expected_count: usize,
) -> RetrievalResult<Vec<Vec<f32>>> {
    if model_id != identity.model_id {
        return Err(RetrievalError::ProviderResponse(
            "embedding response model mismatch".to_owned(),
        ));
    }
    if dimensions != identity.dimensions || vectors.len() != expected_count {
        return Err(RetrievalError::ProviderResponse(
            "embedding response shape mismatch".to_owned(),
        ));
    }
    vectors
        .into_iter()
        .map(|vector| {
            if vector.len() != identity.dimensions as usize {
                return Err(RetrievalError::ProviderResponse(
                    "embedding response dimensions mismatch".to_owned(),
                ));
            }
            vector
                .into_iter()
                .map(|value| {
                    let value = value as f32;
                    if value.is_finite() {
                        Ok(value)
                    } else {
                        Err(RetrievalError::ProviderResponse(
                            "embedding response contains a non-finite value".to_owned(),
                        ))
                    }
                })
                .collect()
        })
        .collect()
}

pub fn validate_rerank_response(
    response: RerankAdapterResponse,
    expected_request_id: &str,
    identity: &RerankProviderIdentity,
    inputs: &[RerankInput],
) -> RetrievalResult<Vec<RerankScore>> {
    validate_correlation(
        Some(response.request_id.as_str()),
        expected_request_id,
        false,
        "rerank",
    )?;
    validate_indexed_rerank_scores(response.model_id, response.scores, identity, inputs)
}

pub fn validate_openai_rerank_response(
    response: OpenAiRerankAdapterResponse,
    expected_request_id: &str,
    identity: &RerankProviderIdentity,
    inputs: &[RerankInput],
) -> RetrievalResult<Vec<RerankScore>> {
    validate_correlation(
        response.request_id.as_deref(),
        expected_request_id,
        true,
        "rerank",
    )?;
    validate_indexed_rerank_scores(response.model_id, response.scores, identity, inputs)
}

fn validate_indexed_rerank_scores(
    model_id: String,
    scores: Vec<IndexedRerankScore>,
    identity: &RerankProviderIdentity,
    inputs: &[RerankInput],
) -> RetrievalResult<Vec<RerankScore>> {
    if model_id != identity.model_id || scores.len() != inputs.len() {
        return Err(RetrievalError::ProviderResponse(
            "rerank response model or item count mismatch".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut output = Vec::with_capacity(inputs.len());
    for item in scores {
        let index = item.index as usize;
        if index >= inputs.len() || !seen.insert(index) || !item.score.is_finite() {
            return Err(RetrievalError::ProviderResponse(
                "rerank response index or score is invalid".to_owned(),
            ));
        }
        output.push(RerankScore {
            chunk_id: inputs[index].chunk_id.clone(),
            score: item.score,
        });
    }
    Ok(output)
}

fn validate_correlation(
    actual: Option<&str>,
    expected: &str,
    allow_missing: bool,
    stage: &str,
) -> RetrievalResult<()> {
    if actual.is_none() && allow_missing {
        return Ok(());
    }
    if actual != Some(expected) {
        return Err(RetrievalError::ProviderResponse(format!(
            "{stage} response correlation mismatch"
        )));
    }
    Ok(())
}

#[derive(Clone)]
pub struct ProviderSecret(String);

impl ProviderSecret {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ProviderSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProviderSecret([REDACTED])")
    }
}

pub trait ProviderSecretResolver: Send + Sync {
    fn resolve(&self, variable: &str) -> RetrievalResult<Option<ProviderSecret>>;
}

pub struct EnvironmentSecretResolver;

impl ProviderSecretResolver for EnvironmentSecretResolver {
    fn resolve(&self, variable: &str) -> RetrievalResult<Option<ProviderSecret>> {
        match std::env::var(variable) {
            Ok(value) if !value.trim().is_empty() => Ok(Some(ProviderSecret(value))),
            Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(RetrievalError::InvalidConfig(
                "configured provider secret is not valid Unicode".to_owned(),
            )),
        }
    }
}

#[derive(Clone)]
pub struct OpenAiEmbeddingRequest {
    pub request_id: String,
    pub endpoint: String,
    pub bearer_token: Option<ProviderSecret>,
    pub model_id: String,
    pub dimensions: u32,
    pub texts: Vec<String>,
}

#[derive(Clone)]
pub struct OpenAiRerankRequest {
    pub request_id: String,
    pub endpoint: String,
    pub bearer_token: Option<ProviderSecret>,
    pub model_id: String,
    pub query: String,
    pub inputs: Vec<RerankInput>,
}

#[derive(Clone)]
pub struct LocalEmbeddingRequest {
    pub request_id: String,
    pub model_path: String,
    pub model_id: String,
    pub dimensions: u32,
    pub texts: Vec<String>,
}

#[derive(Clone)]
pub struct LocalRerankRequest {
    pub request_id: String,
    pub model_path: String,
    pub model_id: String,
    pub query: String,
    pub inputs: Vec<RerankInput>,
}

#[derive(Clone)]
pub struct McpEmbeddingRequest {
    pub request_id: String,
    pub server: String,
    pub tool: String,
    pub model_id: String,
    pub dimensions: u32,
    pub texts: Vec<String>,
}

#[derive(Clone)]
pub struct McpRerankRequest {
    pub request_id: String,
    pub server: String,
    pub tool: String,
    pub model_id: String,
    pub query: String,
    pub inputs: Vec<RerankInput>,
}

impl std::fmt::Debug for OpenAiEmbeddingRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiEmbeddingRequest")
            .field("request_id", &self.request_id)
            .field("model_id", &self.model_id)
            .field("dimensions", &self.dimensions)
            .field("item_count", &self.texts.len())
            .field("credential_present", &self.bearer_token.is_some())
            .finish()
    }
}

impl std::fmt::Debug for OpenAiRerankRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiRerankRequest")
            .field("request_id", &self.request_id)
            .field("model_id", &self.model_id)
            .field("item_count", &self.inputs.len())
            .field("credential_present", &self.bearer_token.is_some())
            .finish()
    }
}

impl std::fmt::Debug for LocalEmbeddingRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalEmbeddingRequest")
            .field("request_id", &self.request_id)
            .field("model_id", &self.model_id)
            .field("dimensions", &self.dimensions)
            .field("item_count", &self.texts.len())
            .finish()
    }
}

impl std::fmt::Debug for LocalRerankRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalRerankRequest")
            .field("request_id", &self.request_id)
            .field("model_id", &self.model_id)
            .field("item_count", &self.inputs.len())
            .finish()
    }
}

impl std::fmt::Debug for McpEmbeddingRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpEmbeddingRequest")
            .field("request_id", &self.request_id)
            .field("model_id", &self.model_id)
            .field("dimensions", &self.dimensions)
            .field("item_count", &self.texts.len())
            .finish()
    }
}

impl std::fmt::Debug for McpRerankRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpRerankRequest")
            .field("request_id", &self.request_id)
            .field("model_id", &self.model_id)
            .field("item_count", &self.inputs.len())
            .finish()
    }
}

pub trait OpenAiCompatibleTransport: Send + Sync {
    fn embed(
        &self,
        request: OpenAiEmbeddingRequest,
    ) -> ProviderFuture<'_, OpenAiEmbeddingAdapterResponse>;
    fn rerank(
        &self,
        request: OpenAiRerankRequest,
    ) -> ProviderFuture<'_, OpenAiRerankAdapterResponse>;
}

pub trait LocalOnnxExecutor: Send + Sync {
    fn embed(&self, request: LocalEmbeddingRequest)
        -> ProviderFuture<'_, EmbeddingAdapterResponse>;
    fn rerank(&self, request: LocalRerankRequest) -> ProviderFuture<'_, RerankAdapterResponse>;
}

pub trait McpProviderCaller: Send + Sync {
    fn embed(&self, request: McpEmbeddingRequest) -> ProviderFuture<'_, EmbeddingAdapterResponse>;
    fn rerank(&self, request: McpRerankRequest) -> ProviderFuture<'_, RerankAdapterResponse>;
}

#[derive(Clone)]
pub struct ProviderAdapterDependencies {
    pub openai_compatible: Option<Arc<dyn OpenAiCompatibleTransport>>,
    pub local_onnx: Option<Arc<dyn LocalOnnxExecutor>>,
    pub mcp: Option<Arc<dyn McpProviderCaller>>,
    pub secret_resolver: Arc<dyn ProviderSecretResolver>,
}

impl Default for ProviderAdapterDependencies {
    fn default() -> Self {
        Self {
            openai_compatible: None,
            local_onnx: None,
            mcp: None,
            secret_resolver: Arc::new(EnvironmentSecretResolver),
        }
    }
}

pub struct SelectedProviders {
    pub vector: Option<Box<dyn VectorProvider>>,
    pub reranker: Option<Box<dyn RerankProvider>>,
}

enum ConfiguredVectorProvider {
    OpenAiCompatible {
        identity: VectorProviderIdentity,
        endpoint: String,
        token: Option<ProviderSecret>,
        transport: Arc<dyn OpenAiCompatibleTransport>,
    },
    LocalOnnx {
        identity: VectorProviderIdentity,
        model_path: String,
        executor: Arc<dyn LocalOnnxExecutor>,
    },
    Mcp {
        identity: VectorProviderIdentity,
        server: String,
        tool: String,
        caller: Arc<dyn McpProviderCaller>,
    },
}

enum ConfiguredRerankProvider {
    OpenAiCompatible {
        identity: RerankProviderIdentity,
        endpoint: String,
        token: Option<ProviderSecret>,
        transport: Arc<dyn OpenAiCompatibleTransport>,
    },
    LocalOnnx {
        identity: RerankProviderIdentity,
        model_path: String,
        executor: Arc<dyn LocalOnnxExecutor>,
    },
    Mcp {
        identity: RerankProviderIdentity,
        server: String,
        tool: String,
        caller: Arc<dyn McpProviderCaller>,
    },
}

pub fn select_configured_providers(
    config: &crate::config::TrustedGkosConfig,
    dependencies: &ProviderAdapterDependencies,
) -> RetrievalResult<SelectedProviders> {
    let vector = config
        .vector_config()
        .map(|provider| create_vector_provider(provider, dependencies))
        .transpose()?;
    let reranker = config
        .rerank_config()
        .map(|provider| create_rerank_provider(provider, dependencies))
        .transpose()?;
    Ok(SelectedProviders { vector, reranker })
}

fn create_vector_provider(
    config: &VectorProviderConfig,
    dependencies: &ProviderAdapterDependencies,
) -> RetrievalResult<Box<dyn VectorProvider>> {
    let identity = config.identity()?;
    let provider = match config {
        VectorProviderConfig::OpenaiCompatible {
            endpoint,
            token_env,
            ..
        } => ConfiguredVectorProvider::OpenAiCompatible {
            identity,
            endpoint: endpoint.clone(),
            token: resolve_configured_secret(
                token_env.as_deref(),
                "vectors",
                dependencies.secret_resolver.as_ref(),
            )?,
            transport: dependencies.openai_compatible.clone().ok_or_else(|| {
                RetrievalError::InvalidConfig(
                    "OPENAI_COMPATIBLE_TRANSPORT_UNAVAILABLE:vectors".to_owned(),
                )
            })?,
        },
        VectorProviderConfig::LocalOnnx { model_path, .. } => ConfiguredVectorProvider::LocalOnnx {
            identity,
            model_path: model_path.clone(),
            executor: dependencies.local_onnx.clone().ok_or_else(|| {
                RetrievalError::InvalidConfig("LOCAL_ONNX_EXECUTOR_UNAVAILABLE:vectors".to_owned())
            })?,
        },
        VectorProviderConfig::Mcp { server, tool, .. } => ConfiguredVectorProvider::Mcp {
            identity,
            server: server.clone(),
            tool: tool.clone(),
            caller: dependencies.mcp.clone().ok_or_else(|| {
                RetrievalError::InvalidConfig("MCP_PROVIDER_CALLER_UNAVAILABLE:vectors".to_owned())
            })?,
        },
    };
    Ok(Box::new(provider))
}

fn create_rerank_provider(
    config: &RerankProviderConfig,
    dependencies: &ProviderAdapterDependencies,
) -> RetrievalResult<Box<dyn RerankProvider>> {
    let identity = config.identity()?;
    let provider = match config {
        RerankProviderConfig::OpenaiCompatible {
            endpoint,
            token_env,
            ..
        } => ConfiguredRerankProvider::OpenAiCompatible {
            identity,
            endpoint: endpoint.clone(),
            token: resolve_configured_secret(
                token_env.as_deref(),
                "reranker",
                dependencies.secret_resolver.as_ref(),
            )?,
            transport: dependencies.openai_compatible.clone().ok_or_else(|| {
                RetrievalError::InvalidConfig(
                    "OPENAI_COMPATIBLE_TRANSPORT_UNAVAILABLE:reranker".to_owned(),
                )
            })?,
        },
        RerankProviderConfig::LocalOnnx { model_path, .. } => ConfiguredRerankProvider::LocalOnnx {
            identity,
            model_path: model_path.clone(),
            executor: dependencies.local_onnx.clone().ok_or_else(|| {
                RetrievalError::InvalidConfig("LOCAL_ONNX_EXECUTOR_UNAVAILABLE:reranker".to_owned())
            })?,
        },
        RerankProviderConfig::Mcp { server, tool, .. } => ConfiguredRerankProvider::Mcp {
            identity,
            server: server.clone(),
            tool: tool.clone(),
            caller: dependencies.mcp.clone().ok_or_else(|| {
                RetrievalError::InvalidConfig("MCP_PROVIDER_CALLER_UNAVAILABLE:reranker".to_owned())
            })?,
        },
    };
    Ok(Box::new(provider))
}

fn resolve_configured_secret(
    variable: Option<&str>,
    section: &str,
    resolver: &dyn ProviderSecretResolver,
) -> RetrievalResult<Option<ProviderSecret>> {
    let Some(variable) = variable else {
        return Ok(None);
    };
    resolver.resolve(variable)?.map(Some).ok_or_else(|| {
        RetrievalError::InvalidConfig(format!(
            "GKOS_CONFIG_SECRET_UNAVAILABLE:{section}.token_env"
        ))
    })
}

fn validate_request_id(request_id: &str) -> RetrievalResult<()> {
    if request_id.trim().is_empty() || request_id.len() > 256 {
        Err(RetrievalError::InvalidConfig(
            "provider request_id must contain from 1 through 256 UTF-8 bytes".to_owned(),
        ))
    } else {
        Ok(())
    }
}

impl VectorProvider for ConfiguredVectorProvider {
    fn identity(&self) -> &VectorProviderIdentity {
        match self {
            Self::OpenAiCompatible { identity, .. }
            | Self::LocalOnnx { identity, .. }
            | Self::Mcp { identity, .. } => identity,
        }
    }

    fn embed<'a>(
        &'a self,
        request_id: &'a str,
        texts: &'a [String],
    ) -> ProviderFuture<'a, Vec<Vec<f32>>> {
        if let Err(error) = validate_request_id(request_id) {
            return Box::pin(async move { Err(error) });
        }
        let request_id = request_id.to_owned();
        let texts = texts.to_vec();
        match self {
            Self::OpenAiCompatible {
                identity,
                endpoint,
                token,
                transport,
            } => {
                let identity = identity.clone();
                let endpoint = endpoint.clone();
                let token = token.clone();
                let transport = transport.clone();
                let expected_count = texts.len();
                Box::pin(async move {
                    let response = bounded_provider_call(
                        transport.embed(OpenAiEmbeddingRequest {
                            request_id: request_id.clone(),
                            endpoint,
                            bearer_token: token,
                            model_id: identity.model_id.clone(),
                            dimensions: identity.dimensions,
                            texts,
                        }),
                        identity.timeout_ms,
                        "OPENAI_COMPATIBLE_EMBEDDING_TIMEOUT",
                    )
                    .await?;
                    validate_openai_embedding_response(
                        response,
                        &request_id,
                        &identity,
                        expected_count,
                    )
                })
            }
            Self::LocalOnnx {
                identity,
                model_path,
                executor,
            } => {
                let identity = identity.clone();
                let model_path = model_path.clone();
                let executor = executor.clone();
                let expected_count = texts.len();
                Box::pin(async move {
                    let response = bounded_provider_call(
                        executor.embed(LocalEmbeddingRequest {
                            request_id: request_id.clone(),
                            model_path,
                            model_id: identity.model_id.clone(),
                            dimensions: identity.dimensions,
                            texts,
                        }),
                        identity.timeout_ms,
                        "LOCAL_EMBEDDING_TIMEOUT",
                    )
                    .await?;
                    validate_embedding_response(response, &request_id, &identity, expected_count)
                })
            }
            Self::Mcp {
                identity,
                server,
                tool,
                caller,
            } => {
                let identity = identity.clone();
                let server = server.clone();
                let tool = tool.clone();
                let caller = caller.clone();
                let expected_count = texts.len();
                Box::pin(async move {
                    let response = bounded_provider_call(
                        caller.embed(McpEmbeddingRequest {
                            request_id: request_id.clone(),
                            server,
                            tool,
                            model_id: identity.model_id.clone(),
                            dimensions: identity.dimensions,
                            texts,
                        }),
                        identity.timeout_ms,
                        "MCP_EMBEDDING_TIMEOUT",
                    )
                    .await?;
                    validate_embedding_response(response, &request_id, &identity, expected_count)
                })
            }
        }
    }
}

impl RerankProvider for ConfiguredRerankProvider {
    fn identity(&self) -> &RerankProviderIdentity {
        match self {
            Self::OpenAiCompatible { identity, .. }
            | Self::LocalOnnx { identity, .. }
            | Self::Mcp { identity, .. } => identity,
        }
    }

    fn rerank<'a>(
        &'a self,
        request_id: &'a str,
        query: &'a str,
        inputs: &'a [RerankInput],
    ) -> ProviderFuture<'a, Vec<RerankScore>> {
        if let Err(error) = validate_request_id(request_id) {
            return Box::pin(async move { Err(error) });
        }
        let request_id = request_id.to_owned();
        let query = query.to_owned();
        let inputs = inputs.to_vec();
        match self {
            Self::OpenAiCompatible {
                identity,
                endpoint,
                token,
                transport,
            } => {
                let identity = identity.clone();
                let endpoint = endpoint.clone();
                let token = token.clone();
                let transport = transport.clone();
                Box::pin(async move {
                    let response = bounded_provider_call(
                        transport.rerank(OpenAiRerankRequest {
                            request_id: request_id.clone(),
                            endpoint,
                            bearer_token: token,
                            model_id: identity.model_id.clone(),
                            query,
                            inputs: inputs.clone(),
                        }),
                        identity.timeout_ms,
                        "OPENAI_COMPATIBLE_RERANK_TIMEOUT",
                    )
                    .await?;
                    validate_openai_rerank_response(response, &request_id, &identity, &inputs)
                })
            }
            Self::LocalOnnx {
                identity,
                model_path,
                executor,
            } => {
                let identity = identity.clone();
                let model_path = model_path.clone();
                let executor = executor.clone();
                Box::pin(async move {
                    let response = bounded_provider_call(
                        executor.rerank(LocalRerankRequest {
                            request_id: request_id.clone(),
                            model_path,
                            model_id: identity.model_id.clone(),
                            query,
                            inputs: inputs.clone(),
                        }),
                        identity.timeout_ms,
                        "LOCAL_RERANK_TIMEOUT",
                    )
                    .await?;
                    validate_rerank_response(response, &request_id, &identity, &inputs)
                })
            }
            Self::Mcp {
                identity,
                server,
                tool,
                caller,
            } => {
                let identity = identity.clone();
                let server = server.clone();
                let tool = tool.clone();
                let caller = caller.clone();
                Box::pin(async move {
                    let response = bounded_provider_call(
                        caller.rerank(McpRerankRequest {
                            request_id: request_id.clone(),
                            server,
                            tool,
                            model_id: identity.model_id.clone(),
                            query,
                            inputs: inputs.clone(),
                        }),
                        identity.timeout_ms,
                        "MCP_RERANK_TIMEOUT",
                    )
                    .await?;
                    validate_rerank_response(response, &request_id, &identity, &inputs)
                })
            }
        }
    }
}

pub fn fts_only_degradation(
    selected: Option<&VectorProviderIdentity>,
    reason_code: &str,
) -> RetrievalProviderStageStatus {
    RetrievalProviderStageStatus {
        kind: selected.map_or(RetrievalProviderStageKind::None, |value| value.kind),
        state: if selected.is_some() {
            RetrievalStageState::Degraded
        } else {
            RetrievalStageState::Disabled
        },
        provider_id: selected.map(|value| value.provider_id.clone()),
        model_id: selected.map(|value| value.model_id.clone()),
        reason_codes: selected.map_or_else(
            || vec!["VECTOR_DISABLED".to_owned()],
            |_| vec![reason_code.to_owned()],
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures_executor::block_on;
    use tempfile::tempdir;

    use super::*;

    struct StaticSecretResolver(Option<String>);

    impl ProviderSecretResolver for StaticSecretResolver {
        fn resolve(&self, _variable: &str) -> RetrievalResult<Option<ProviderSecret>> {
            Ok(self.0.clone().map(ProviderSecret))
        }
    }

    #[derive(Default)]
    struct EchoOpenAi {
        embed_calls: AtomicUsize,
        rerank_calls: AtomicUsize,
    }

    impl OpenAiCompatibleTransport for EchoOpenAi {
        fn embed(
            &self,
            request: OpenAiEmbeddingRequest,
        ) -> ProviderFuture<'_, OpenAiEmbeddingAdapterResponse> {
            self.embed_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(OpenAiEmbeddingAdapterResponse {
                    request_id: None,
                    model_id: request.model_id,
                    dimensions: Some(request.dimensions),
                    data: Some(
                        request
                            .texts
                            .iter()
                            .enumerate()
                            .map(|(index, _)| IndexedEmbedding {
                                index: u32::try_from(index).unwrap(),
                                embedding: vec![1.0; request.dimensions as usize],
                            })
                            .collect(),
                    ),
                    vectors: None,
                })
            })
        }

        fn rerank(
            &self,
            request: OpenAiRerankRequest,
        ) -> ProviderFuture<'_, OpenAiRerankAdapterResponse> {
            self.rerank_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(OpenAiRerankAdapterResponse {
                    request_id: None,
                    model_id: request.model_id,
                    scores: request
                        .inputs
                        .iter()
                        .enumerate()
                        .map(|(index, _)| IndexedRerankScore {
                            index: u32::try_from(index).unwrap(),
                            score: 1.0 / (index + 1) as f64,
                        })
                        .collect(),
                })
            })
        }
    }

    #[derive(Default)]
    struct EchoLocal {
        embed_calls: AtomicUsize,
        rerank_calls: AtomicUsize,
    }

    impl LocalOnnxExecutor for EchoLocal {
        fn embed(
            &self,
            request: LocalEmbeddingRequest,
        ) -> ProviderFuture<'_, EmbeddingAdapterResponse> {
            self.embed_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(EmbeddingAdapterResponse {
                    request_id: request.request_id,
                    model_id: request.model_id,
                    dimensions: request.dimensions,
                    vectors: request
                        .texts
                        .iter()
                        .map(|_| vec![1.0; request.dimensions as usize])
                        .collect(),
                })
            })
        }

        fn rerank(&self, request: LocalRerankRequest) -> ProviderFuture<'_, RerankAdapterResponse> {
            self.rerank_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(RerankAdapterResponse {
                    request_id: request.request_id,
                    model_id: request.model_id,
                    scores: request
                        .inputs
                        .iter()
                        .enumerate()
                        .map(|(index, _)| IndexedRerankScore {
                            index: u32::try_from(index).unwrap(),
                            score: 1.0 / (index + 1) as f64,
                        })
                        .collect(),
                })
            })
        }
    }

    #[derive(Default)]
    struct EchoMcp {
        embed_calls: AtomicUsize,
        rerank_calls: AtomicUsize,
    }

    impl McpProviderCaller for EchoMcp {
        fn embed(
            &self,
            request: McpEmbeddingRequest,
        ) -> ProviderFuture<'_, EmbeddingAdapterResponse> {
            self.embed_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(EmbeddingAdapterResponse {
                    request_id: request.request_id,
                    model_id: request.model_id,
                    dimensions: request.dimensions,
                    vectors: request
                        .texts
                        .iter()
                        .map(|_| vec![1.0; request.dimensions as usize])
                        .collect(),
                })
            })
        }

        fn rerank(&self, request: McpRerankRequest) -> ProviderFuture<'_, RerankAdapterResponse> {
            self.rerank_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(RerankAdapterResponse {
                    request_id: request.request_id,
                    model_id: request.model_id,
                    scores: request
                        .inputs
                        .iter()
                        .enumerate()
                        .map(|(index, _)| IndexedRerankScore {
                            index: u32::try_from(index).unwrap(),
                            score: 1.0 / (index + 1) as f64,
                        })
                        .collect(),
                })
            })
        }
    }

    fn select_from_toml(
        text: &str,
        dependencies: &ProviderAdapterDependencies,
    ) -> RetrievalResult<SelectedProviders> {
        let directory = tempdir().unwrap();
        let path = directory.path().join("gkos.toml");
        fs::write(&path, text).unwrap();
        let config = crate::config::discover_trusted_gkos_config(
            &crate::config::TrustedConfigDiscoveryOptions {
                explicit_config: Some(path),
                user_config_path: Some(directory.path().join("missing-user.toml")),
                ..crate::config::TrustedConfigDiscoveryOptions::default()
            },
        )?
        .unwrap();
        select_configured_providers(&config, dependencies)
    }

    struct MemorySourceReader(BTreeMap<String, Vec<u8>>);

    impl crate::coordinator::SourceReader for MemorySourceReader {
        fn read(&self, source_path: &str) -> RetrievalResult<Vec<u8>> {
            self.0
                .get(source_path)
                .cloned()
                .ok_or_else(|| RetrievalError::InvalidEnvelope("source is absent".to_owned()))
        }
    }

    fn configs() -> Vec<VectorProviderConfig> {
        vec![
            VectorProviderConfig::OpenaiCompatible {
                enabled: true,
                provider_id: "operator-choice-a".to_owned(),
                model_id: "unrestricted-model-a".to_owned(),
                dimensions: 2,
                endpoint: "https://operator-selected.invalid/v1/embeddings".to_owned(),
                token_env: Some("GKOS_PROVIDER_TOKEN".to_owned()),
                timeout_ms: Some(1_000),
            },
            VectorProviderConfig::LocalOnnx {
                enabled: true,
                provider_id: "operator-choice-b".to_owned(),
                model_id: "local-model".to_owned(),
                dimensions: 2,
                model_path: "operator/models/anything.onnx".to_owned(),
                timeout_ms: None,
            },
            VectorProviderConfig::Mcp {
                enabled: true,
                provider_id: "operator-choice-c".to_owned(),
                model_id: "upstream-model".to_owned(),
                dimensions: 2,
                server: "operator-mcp".to_owned(),
                tool: "custom.embed".to_owned(),
                timeout_ms: None,
            },
        ]
    }

    #[test]
    fn all_provider_families_are_neutral_operator_choices() {
        let identities = configs()
            .iter()
            .map(VectorProviderConfig::identity)
            .collect::<RetrievalResult<Vec<_>>>()
            .unwrap();
        assert_eq!(
            identities
                .iter()
                .map(|identity| identity.kind)
                .collect::<Vec<_>>(),
            [
                RetrievalProviderStageKind::OpenaiCompatible,
                RetrievalProviderStageKind::LocalOnnx,
                RetrievalProviderStageKind::Mcp
            ]
        );
        assert!(identities
            .iter()
            .all(|identity| identity.configuration_digest.starts_with("sha256:")));
        assert_eq!(
            identities
                .iter()
                .map(|identity| identity.timeout_ms)
                .collect::<Vec<_>>(),
            [
                1_000,
                DEFAULT_PROVIDER_TIMEOUT_MS,
                DEFAULT_PROVIDER_TIMEOUT_MS
            ]
        );
    }

    #[test]
    fn runtime_provider_identity_preflight_is_complete_and_provider_neutral() {
        let valid = configs()[0].identity().unwrap();
        validate_vector_provider_identity(&valid).unwrap();
        for invalid in [
            VectorProviderIdentity {
                kind: RetrievalProviderStageKind::SqliteFts5,
                ..valid.clone()
            },
            VectorProviderIdentity {
                provider_id: String::new(),
                ..valid.clone()
            },
            VectorProviderIdentity {
                model_id: String::new(),
                ..valid.clone()
            },
            VectorProviderIdentity {
                dimensions: 0,
                ..valid.clone()
            },
            VectorProviderIdentity {
                dimensions: 1_000_001,
                ..valid.clone()
            },
            VectorProviderIdentity {
                timeout_ms: 0,
                ..valid.clone()
            },
            VectorProviderIdentity {
                configuration_digest: "sha256:INVALID".to_owned(),
                ..valid.clone()
            },
        ] {
            assert!(matches!(
                validate_vector_provider_identity(&invalid),
                Err(RetrievalError::InvalidConfig(_))
            ));
        }

        let valid_reranker = RerankProviderIdentity {
            kind: RetrievalProviderStageKind::Mcp,
            provider_id: "operator-selected-reranker".to_owned(),
            model_id: "arbitrary-reviewed-model".to_owned(),
            timeout_ms: 1_000,
            configuration_digest: crate::digest::sha256(b"arbitrary-rerank-config"),
        };
        validate_rerank_provider_identity(&valid_reranker).unwrap();
        for invalid in [
            RerankProviderIdentity {
                kind: RetrievalProviderStageKind::SqliteLexicalScan,
                ..valid_reranker.clone()
            },
            RerankProviderIdentity {
                provider_id: String::new(),
                ..valid_reranker.clone()
            },
            RerankProviderIdentity {
                model_id: String::new(),
                ..valid_reranker.clone()
            },
            RerankProviderIdentity {
                timeout_ms: 300_001,
                ..valid_reranker.clone()
            },
            RerankProviderIdentity {
                configuration_digest: String::new(),
                ..valid_reranker.clone()
            },
        ] {
            assert!(matches!(
                validate_rerank_provider_identity(&invalid),
                Err(RetrievalError::InvalidConfig(_))
            ));
        }
    }

    #[test]
    fn embedding_response_fails_closed_on_every_binding() {
        let identity = configs()[0].identity().unwrap();
        let valid = EmbeddingAdapterResponse {
            request_id: "request-a".to_owned(),
            model_id: identity.model_id.clone(),
            dimensions: 2,
            vectors: vec![vec![1.0, 0.0]],
        };
        assert_eq!(
            validate_embedding_response(valid.clone(), "request-a", &identity, 1).unwrap(),
            [vec![1.0, 0.0]]
        );
        let mut wrong = valid;
        wrong.model_id = "different-space".to_owned();
        assert!(validate_embedding_response(wrong, "request-a", &identity, 1).is_err());
    }

    #[test]
    fn selected_provider_failure_degrades_only_to_explicit_fts() {
        let identity = configs()[2].identity().unwrap();
        let status = fts_only_degradation(Some(&identity), "VECTOR_UNAVAILABLE");
        assert_eq!(status.kind, RetrievalProviderStageKind::Mcp);
        assert_eq!(status.state, RetrievalStageState::Degraded);
        assert_eq!(status.reason_codes, ["VECTOR_UNAVAILABLE"]);
        let absent = fts_only_degradation(None, "ignored");
        assert_eq!(absent.state, RetrievalStageState::Disabled);
        assert_eq!(absent.reason_codes, ["VECTOR_DISABLED"]);
    }

    #[test]
    fn reranker_identity_is_dimensionless_and_provider_neutral() {
        let config = RerankProviderConfig::Mcp {
            enabled: true,
            provider_id: "any-reranker".to_owned(),
            model_id: "any-model".to_owned(),
            server: "operator-server".to_owned(),
            tool: "custom.rerank".to_owned(),
            timeout_ms: None,
        };
        let identity = config.identity().unwrap();
        assert_eq!(identity.kind, RetrievalProviderStageKind::Mcp);
        assert_eq!(identity.timeout_ms, DEFAULT_PROVIDER_TIMEOUT_MS);
        let serialized = serde_json::to_value(identity).unwrap();
        assert!(serialized.get("dimensions").is_none());
    }

    #[test]
    fn openai_compatible_endpoints_require_absolute_http_or_https_without_allowlists() {
        for endpoint in [
            "https://arbitrary.operator.invalid:8443/v1/embeddings",
            "http://127.0.0.1:8091/operator-selected-route",
            "https://[::1]:9443/custom",
        ] {
            let vector = VectorProviderConfig::OpenaiCompatible {
                enabled: true,
                provider_id: "any-provider".to_owned(),
                model_id: "any-model-or-routing-label".to_owned(),
                dimensions: 1,
                endpoint: endpoint.to_owned(),
                token_env: None,
                timeout_ms: None,
            };
            let reranker = RerankProviderConfig::OpenaiCompatible {
                enabled: true,
                provider_id: "another-provider".to_owned(),
                model_id: "another-model".to_owned(),
                endpoint: endpoint.to_owned(),
                token_env: None,
                timeout_ms: None,
            };
            vector.validate().unwrap();
            reranker.validate().unwrap();
        }

        for endpoint in [
            "operator.invalid/v1",
            "/relative/provider",
            "file:///tmp/model",
            "ftp://operator.invalid/model",
            "https://",
            "https:///missing-host",
            "HTTPS://operator.invalid/v1",
            "https://operator.invalid:99999/v1",
            "https://operator.invalid\\redirect",
            "https://user:password@operator.invalid/v1",
            "https://user@operator.invalid/v1",
        ] {
            let vector = VectorProviderConfig::OpenaiCompatible {
                enabled: true,
                provider_id: "provider".to_owned(),
                model_id: "model".to_owned(),
                dimensions: 1,
                endpoint: endpoint.to_owned(),
                token_env: None,
                timeout_ms: None,
            };
            let reranker = RerankProviderConfig::OpenaiCompatible {
                enabled: true,
                provider_id: "provider".to_owned(),
                model_id: "model".to_owned(),
                endpoint: endpoint.to_owned(),
                token_env: None,
                timeout_ms: None,
            };
            assert!(
                vector.validate().is_err(),
                "accepted vector endpoint {endpoint}"
            );
            assert!(
                reranker.validate().is_err(),
                "accepted rerank endpoint {endpoint}"
            );
        }
    }

    #[test]
    fn vector_dimensions_match_the_frozen_resource_bounds() {
        let config = |dimensions| VectorProviderConfig::Mcp {
            enabled: true,
            provider_id: "operator-selected/provider".to_owned(),
            model_id: "operator-selected:model".to_owned(),
            dimensions,
            server: "any-operator-mcp".to_owned(),
            tool: "custom.embed".to_owned(),
            timeout_ms: None,
        };
        config(1).validate().unwrap();
        config(MAX_VECTOR_DIMENSIONS).validate().unwrap();
        assert!(config(0).validate().is_err());
        assert!(config(MAX_VECTOR_DIMENSIONS + 1).validate().is_err());
    }

    #[test]
    fn provider_request_debug_output_redacts_content_queries_routes_and_credentials() {
        let sentinel_text = "SENTINEL-NOTE-CONTENT".to_owned();
        let sentinel_query = "SENTINEL-QUERY-CONTENT".to_owned();
        let sentinel_secret = "SENTINEL-BEARER-TOKEN".to_owned();
        let input = RerankInput {
            chunk_id: "chunk-a".to_owned(),
            text: sentinel_text.clone(),
        };
        let rendered = [
            format!(
                "{:?}",
                OpenAiEmbeddingRequest {
                    request_id: "request-a".to_owned(),
                    endpoint: "https://route-must-not-log.invalid/v1".to_owned(),
                    bearer_token: Some(ProviderSecret(sentinel_secret.clone())),
                    model_id: "model-a".to_owned(),
                    dimensions: 2,
                    texts: vec![sentinel_text.clone()],
                }
            ),
            format!(
                "{:?}",
                OpenAiRerankRequest {
                    request_id: "request-a".to_owned(),
                    endpoint: "https://route-must-not-log.invalid/v1".to_owned(),
                    bearer_token: Some(ProviderSecret(sentinel_secret.clone())),
                    model_id: "model-a".to_owned(),
                    query: sentinel_query.clone(),
                    inputs: vec![input.clone()],
                }
            ),
            format!(
                "{:?}",
                LocalEmbeddingRequest {
                    request_id: "request-a".to_owned(),
                    model_path: "route-must-not-log/model.onnx".to_owned(),
                    model_id: "model-a".to_owned(),
                    dimensions: 2,
                    texts: vec![sentinel_text.clone()],
                }
            ),
            format!(
                "{:?}",
                LocalRerankRequest {
                    request_id: "request-a".to_owned(),
                    model_path: "route-must-not-log/rerank.onnx".to_owned(),
                    model_id: "model-a".to_owned(),
                    query: sentinel_query.clone(),
                    inputs: vec![input.clone()],
                }
            ),
            format!(
                "{:?}",
                McpEmbeddingRequest {
                    request_id: "request-a".to_owned(),
                    server: "route-must-not-log".to_owned(),
                    tool: "secret.embed.route".to_owned(),
                    model_id: "model-a".to_owned(),
                    dimensions: 2,
                    texts: vec![sentinel_text.clone()],
                }
            ),
            format!(
                "{:?}",
                McpRerankRequest {
                    request_id: "request-a".to_owned(),
                    server: "route-must-not-log".to_owned(),
                    tool: "secret.rerank.route".to_owned(),
                    model_id: "model-a".to_owned(),
                    query: sentinel_query.clone(),
                    inputs: vec![input],
                }
            ),
        ]
        .join("\n");
        for secret in [
            sentinel_text.as_str(),
            sentinel_query.as_str(),
            sentinel_secret.as_str(),
            "route-must-not-log",
            "secret.embed.route",
            "secret.rerank.route",
        ] {
            assert!(!rendered.contains(secret), "debug output leaked {secret}");
        }
        assert!(rendered.contains("item_count"));
        assert!(rendered.contains("credential_present: true"));
    }

    #[test]
    fn openai_response_correlation_and_index_bindings_match_full() {
        let identity = configs()[0].identity().unwrap();
        let valid = OpenAiEmbeddingAdapterResponse {
            request_id: None,
            model_id: identity.model_id.clone(),
            dimensions: Some(identity.dimensions),
            data: Some(vec![
                IndexedEmbedding {
                    index: 1,
                    embedding: vec![0.0, 1.0],
                },
                IndexedEmbedding {
                    index: 0,
                    embedding: vec![1.0, 0.0],
                },
            ]),
            vectors: None,
        };
        assert_eq!(
            validate_openai_embedding_response(valid.clone(), "request-a", &identity, 2).unwrap(),
            [vec![1.0, 0.0], vec![0.0, 1.0]]
        );
        let mut mismatch = valid.clone();
        mismatch.request_id = Some("wrong".to_owned());
        assert!(validate_openai_embedding_response(mismatch, "request-a", &identity, 2).is_err());
        let mut duplicate = valid.clone();
        duplicate.data.as_mut().unwrap()[1].index = 1;
        assert!(validate_openai_embedding_response(duplicate, "request-a", &identity, 2).is_err());
        let mut out_of_range = valid;
        out_of_range.data.as_mut().unwrap()[1].index = 2;
        assert!(
            validate_openai_embedding_response(out_of_range, "request-a", &identity, 2).is_err()
        );

        let rerank_identity = RerankProviderConfig::OpenaiCompatible {
            enabled: true,
            provider_id: "operator-rerank".to_owned(),
            model_id: "rerank-model".to_owned(),
            endpoint: "https://operator.invalid/rerank".to_owned(),
            token_env: None,
            timeout_ms: None,
        }
        .identity()
        .unwrap();
        let inputs = vec![
            RerankInput {
                chunk_id: "a".to_owned(),
                text: "alpha".to_owned(),
            },
            RerankInput {
                chunk_id: "b".to_owned(),
                text: "beta".to_owned(),
            },
        ];
        let response = OpenAiRerankAdapterResponse {
            request_id: None,
            model_id: rerank_identity.model_id.clone(),
            scores: vec![
                IndexedRerankScore {
                    index: 1,
                    score: 0.9,
                },
                IndexedRerankScore {
                    index: 0,
                    score: 0.8,
                },
            ],
        };
        let scores = validate_openai_rerank_response(
            response.clone(),
            "request-a",
            &rerank_identity,
            &inputs,
        )
        .unwrap();
        assert_eq!(scores[0].chunk_id, "b");
        let mut mismatch = response;
        mismatch.request_id = Some("wrong".to_owned());
        assert!(
            validate_openai_rerank_response(mismatch, "request-a", &rerank_identity, &inputs)
                .is_err()
        );
    }

    #[test]
    fn trusted_config_selects_each_concrete_provider_family_without_fallback() {
        let dependencies = ProviderAdapterDependencies {
            openai_compatible: Some(Arc::new(EchoOpenAi::default())),
            local_onnx: Some(Arc::new(EchoLocal::default())),
            mcp: Some(Arc::new(EchoMcp::default())),
            secret_resolver: Arc::new(StaticSecretResolver(None)),
        };
        let family_sections = [
            (
                "openai_compatible",
                r#"endpoint = "https://any.operator.invalid/custom"
"#,
                RetrievalProviderStageKind::OpenaiCompatible,
            ),
            (
                "local_onnx",
                r#"model_path = "operator/models/custom.onnx"
"#,
                RetrievalProviderStageKind::LocalOnnx,
            ),
            (
                "mcp",
                r#"server = "operator-mcp"
tool = "operator.embed"
"#,
                RetrievalProviderStageKind::Mcp,
            ),
        ];
        for (provider, family_fields, expected_kind) in family_sections {
            let rerank_fields = match provider {
                "openai_compatible" => "endpoint = \"https://any.operator.invalid/rerank\"\n",
                "local_onnx" => "model_path = \"operator/models/rerank.onnx\"\n",
                "mcp" => "server = \"operator-mcp\"\ntool = \"operator.rerank\"\n",
                _ => unreachable!(),
            };
            let document = format!(
                "config_version = 1\n\n[vectors]\nenabled = true\nprovider = \"{provider}\"\nprovider_id = \"any-vector\"\nmodel_id = \"any-vector-model\"\ndimensions = 2\ntimeout_ms = 1000\n{family_fields}\n[reranker]\nenabled = true\nprovider = \"{provider}\"\nprovider_id = \"any-reranker\"\nmodel_id = \"any-rerank-model\"\ntimeout_ms = 1000\n{rerank_fields}"
            );
            let selected = select_from_toml(&document, &dependencies).unwrap();
            let vector = selected.vector.unwrap();
            let reranker = selected.reranker.unwrap();
            assert_eq!(vector.identity().kind, expected_kind);
            assert_eq!(reranker.identity().kind, expected_kind);
            assert_eq!(
                block_on(vector.embed("request-a", &["one".to_owned(), "two".to_owned()]))
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                block_on(reranker.rerank(
                    "request-b",
                    "query",
                    &[
                        RerankInput {
                            chunk_id: "a".to_owned(),
                            text: "one".to_owned(),
                        },
                        RerankInput {
                            chunk_id: "b".to_owned(),
                            text: "two".to_owned(),
                        },
                    ],
                ))
                .unwrap()
                .len(),
                2
            );
        }
    }

    #[test]
    fn explicit_token_env_is_fail_closed_and_omission_supports_open_endpoints() {
        let configured = r#"config_version = 1
[vectors]
enabled = true
provider = "openai_compatible"
provider_id = "operator-vector"
model_id = "operator-model"
dimensions = 2
endpoint = "https://operator.invalid/embed"
token_env = "GKOS_MISSING_TOKEN"
"#;
        let missing = ProviderAdapterDependencies {
            openai_compatible: Some(Arc::new(EchoOpenAi::default())),
            secret_resolver: Arc::new(StaticSecretResolver(None)),
            ..ProviderAdapterDependencies::default()
        };
        let error = match select_from_toml(configured, &missing) {
            Ok(_) => panic!("missing explicitly configured secret was accepted"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .ends_with("GKOS_CONFIG_SECRET_UNAVAILABLE:vectors.token_env"));

        let open = configured.replace("token_env = \"GKOS_MISSING_TOKEN\"\n", "");
        let selected = select_from_toml(&open, &missing).unwrap();
        assert!(selected.vector.is_some());

        let supplied = ProviderAdapterDependencies {
            openai_compatible: Some(Arc::new(EchoOpenAi::default())),
            secret_resolver: Arc::new(StaticSecretResolver(Some("secret".to_owned()))),
            ..ProviderAdapterDependencies::default()
        };
        assert!(select_from_toml(configured, &supplied)
            .unwrap()
            .vector
            .is_some());
    }

    #[test]
    fn trusted_toml_alone_swaps_adapters_with_contract_equivalent_index_and_search() {
        let openai = Arc::new(EchoOpenAi::default());
        let local = Arc::new(EchoLocal::default());
        let mcp = Arc::new(EchoMcp::default());
        let dependencies = ProviderAdapterDependencies {
            openai_compatible: Some(openai.clone()),
            local_onnx: Some(local.clone()),
            mcp: Some(mcp.clone()),
            secret_resolver: Arc::new(StaticSecretResolver(None)),
        };
        let families = [
            (
                "openai_compatible",
                "endpoint = \"https://arbitrary.operator.invalid/embed\"\n",
                "endpoint = \"https://arbitrary.operator.invalid/rerank\"\n",
                RetrievalProviderStageKind::OpenaiCompatible,
            ),
            (
                "local_onnx",
                "model_path = \"operator/models/embed.onnx\"\n",
                "model_path = \"operator/models/rerank.onnx\"\n",
                RetrievalProviderStageKind::LocalOnnx,
            ),
            (
                "mcp",
                "server = \"operator-mcp\"\ntool = \"operator.embed\"\n",
                "server = \"operator-mcp\"\ntool = \"operator.rerank\"\n",
                RetrievalProviderStageKind::Mcp,
            ),
        ];
        let mut baseline = None;
        for (provider, vector_fields, rerank_fields, expected_kind) in families {
            let before = [
                openai.embed_calls.load(Ordering::SeqCst),
                openai.rerank_calls.load(Ordering::SeqCst),
                local.embed_calls.load(Ordering::SeqCst),
                local.rerank_calls.load(Ordering::SeqCst),
                mcp.embed_calls.load(Ordering::SeqCst),
                mcp.rerank_calls.load(Ordering::SeqCst),
            ];
            let document = format!(
                "config_version = 1\n\n[vectors]\nenabled = true\nprovider = \"{provider}\"\nprovider_id = \"fixed-vector\"\nmodel_id = \"fixed-vector-model\"\ndimensions = 2\ntimeout_ms = 1000\n{vector_fields}\n[reranker]\nenabled = true\nprovider = \"{provider}\"\nprovider_id = \"fixed-reranker\"\nmodel_id = \"fixed-rerank-model\"\ntimeout_ms = 1000\n{rerank_fields}"
            );
            let selected = select_from_toml(&document, &dependencies).unwrap();
            let vector = selected.vector.unwrap();
            let reranker = selected.reranker.unwrap();
            assert_eq!(vector.identity().kind, expected_kind);
            assert_eq!(reranker.identity().kind, expected_kind);

            let state = tempdir().unwrap();
            let source =
                |source_id: &str, source_path: &str, text: &str| crate::contract::RetrievalSource {
                    contract_version: crate::contract::RETRIEVAL_CONTRACT.to_owned(),
                    vault_id: "vault-a".to_owned(),
                    source_id: source_id.to_owned(),
                    source_path: source_path.to_owned(),
                    source_digest: crate::digest::sha256(text.as_bytes()),
                    text: text.to_owned(),
                    discoverability: crate::contract::DiscoverabilityDecision::Allow,
                    lineage: crate::contract::CanonicalLineageEnvelope::default(),
                    temporal: crate::contract::CanonicalTemporalEnvelope::default(),
                    metadata: crate::contract::RetrievalChunkMetadata::default(),
                };
            let sources = vec![
                source(
                    "019b2d14-4230-7db7-87d4-7d81cfaeca01",
                    "alpha.md",
                    "# Alpha\nneedle canonical alpha",
                ),
                source(
                    "019b2d14-4230-7db7-87d4-7d81cfaeca02",
                    "beta.md",
                    "# Beta\nneedle canonical beta",
                ),
            ];
            let reader = MemorySourceReader(
                sources
                    .iter()
                    .map(|source| (source.source_path.clone(), source.text.as_bytes().to_vec()))
                    .collect(),
            );
            let indexed = block_on(crate::coordinator::index_retrieval_generation(
                crate::sqlite_store::RetrievalGenerationInput {
                    state_directory: state.path().to_path_buf(),
                    engine_version: "lite-phase1".to_owned(),
                    vault_id: "vault-a".to_owned(),
                    source_snapshot_digest: crate::digest::sha256(b"fixed-snapshot"),
                    configuration_digest: crate::digest::sha256(b"fixed-config"),
                    policy_digest: crate::digest::sha256(b"fixed-policy"),
                    sources,
                    chunking: crate::chunker::ChunkingOptions::default(),
                    vectors: Vec::new(),
                    embedding_provider_id: None,
                    embedding_model_id: None,
                    embedding_dimensions: None,
                },
                Some(vector.as_ref()),
            ))
            .unwrap();
            crate::activate_retrieval_generation(state.path(), &indexed.generation).unwrap();
            let policy = |_chunk: &crate::contract::RetrievalChunk| {
                Ok(crate::contract::DiscoverabilityDecision::Allow)
            };
            let mut options =
                crate::coordinator::RetrievalCoordinatorOptions::fts_only(&policy, &reader);
            options.vector_provider = Some(vector.as_ref());
            options.rerank_provider = Some(reranker.as_ref());
            let coordinator =
                crate::coordinator::RetrievalCoordinator::open_active(state.path(), options)
                    .unwrap();
            let result = block_on(
                coordinator.search(&crate::contract::RetrievalSearchRequest {
                    query: "needle canonical".to_owned(),
                    limit: Some(5),
                    lexical_top_k: Some(10),
                    semantic_top_k: Some(10),
                    filters: Some(crate::contract::RetrievalFilters::default()),
                    rrf_k: Some(60),
                    mmr: Some(false),
                    mmr_lambda: None,
                    parent_expansion: Some(false),
                    parent_expansion_max_child_tokens: None,
                }),
            )
            .unwrap();
            let signature = result
                .hits()
                .iter()
                .map(|hit| (hit.chunk().chunk_id.clone(), hit.stage_scores().clone()))
                .collect::<Vec<_>>();
            if let Some(baseline) = baseline.as_ref() {
                assert_eq!(&signature, baseline);
            } else {
                baseline = Some(signature);
            }

            let after = [
                openai.embed_calls.load(Ordering::SeqCst),
                openai.rerank_calls.load(Ordering::SeqCst),
                local.embed_calls.load(Ordering::SeqCst),
                local.rerank_calls.load(Ordering::SeqCst),
                mcp.embed_calls.load(Ordering::SeqCst),
                mcp.rerank_calls.load(Ordering::SeqCst),
            ];
            let delta = std::array::from_fn::<_, 6, _>(|index| after[index] - before[index]);
            let expected = match expected_kind {
                RetrievalProviderStageKind::OpenaiCompatible => [2, 1, 0, 0, 0, 0],
                RetrievalProviderStageKind::LocalOnnx => [0, 0, 2, 1, 0, 0],
                RetrievalProviderStageKind::Mcp => [0, 0, 0, 0, 2, 1],
                _ => unreachable!(),
            };
            assert_eq!(delta, expected);
        }
    }
}
