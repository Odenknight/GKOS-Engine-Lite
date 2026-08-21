# ADR-0002: Use Full-owned retrieval contracts, local SQLite projections, and provider-neutral inference adapters

## Context

Lite currently delegates deterministic commands to a pinned `gkos-engine` package and bundles the Full desktop sidecar. No verified existing Postgres retrieval schema, Qdrant collection, or remote lifecycle authority was found at the Phase 0 baseline. The current Node runtime exposes SQLite 3.53.1 with FTS5 on the inspected host. Optional vector and model dependencies have not yet passed the required target, license, crash-safety, packaging, or AVX-only qualification.

Operators require freedom to choose inference routing without the engine privileging a vendor, model, domain, or transport.

## Decision

Full owns the versioned retrieval contracts and TypeScript reference implementation, initially exposed through a package boundary such as `gkos-engine/retrieval`. Under ADR-0005, Lite implements the same pin-bound contracts in Rust and proves conformance against the Full reference; it does not import the TypeScript runtime into the required static executable and cannot define independent ranking or GKX semantics. The default Lite lexical projection is SQLite FTS5 beneath the controlled `.gkx/derived/` state directory. The default remains fully functional FTS-only.

Embedding and reranking use configuration-selected interfaces with three adapter families:

- `openai_compatible`: any endpoint, model, and token chosen by a trusted operator;
- `local_onnx`: any local model path chosen by a trusted operator;
- `mcp`: an explicitly configured upstream MCP embedding or reranking tool.

Ranking code has no vendor, domain, model, or routing allowlist and no provider preference. Provider configuration records adapter identity, model identity, dimensions, and content/config digests in the projection manifest. A model or embedding-space change creates a new vector projection. There is no silent fallback between embedding spaces. If the selected embedding provider is unavailable, the service reports degradation and remains FTS-only. If optional reranking is unavailable, that stage is skipped and reported.

Configuration provenance remains a security boundary: only trusted operator configuration may set endpoints, credentials, model/executable paths, upstream MCP tools, bind/authentication settings, or authorization policy. An untrusted vault note or vault-local configuration cannot redirect credentials or requests. Tokens remain in protected secret sources rather than committed configuration.

SQLite vector support is optional and may be selected only after target/platform qualification. Absent a verified external database contract, no external database adapter is advertised as active and no service is provisioned by GKOS-Engine or Lite.

## Alternatives rejected

- Define an independent Lite retrieval contract or unpinned semantic implementation. That violates the consumer boundary and parity requirement.
- Require a remote vector database or vector provider. FTS-only must remain complete and local.
- Hard-code one vendor, model, endpoint, or preferred routing path. Provider choice belongs to trusted operator configuration.
- Silently switch models or providers after failure. Scores from different embedding spaces are not interchangeable.
- Trust configuration discovered inside an arbitrary vault for credentials or routing. Source content cannot control host authority.

## Consequences

Lite stays dependency-light by default. Optional native/model assets expand qualification and licensing work but not deterministic authority. Status and evaluation must disclose the selected adapter and degraded/skipped stages. Derived stores require explicit migrations, coherent manifests, atomic replacement, and rebuild on incompatible metadata.

## Status

Accepted — 2026-08-20. ADR-0005 supersedes only the direct TypeScript-runtime-consumption clause: the required Lite artifact uses a Rust contract-conforming implementation pinned to Full's contracts and reference behavior. Full remains the contract and semantic owner. Optional vector implementations remain unqualified until their later phase gates pass.

## Evidence

- Lite package pin and wrapper inventory in [Phase 0 baseline and reconciliation](../evidence/phase-0-baseline.md).
- Inspected Node v24.18.0 reports SQLite 3.53.1, `ENABLE_FTS5`, and a successful in-memory FTS5 query.
- Repository and organization searches found no verified existing Postgres retrieval schema or Qdrant collection/authority at this baseline; no service was provisioned.
