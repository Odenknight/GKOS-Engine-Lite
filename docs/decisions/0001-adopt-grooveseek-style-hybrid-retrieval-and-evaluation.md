# ADR-0001: Adopt GrooveSeek-style hybrid retrieval and evaluation; GKX and GKOS governance remain canonical

## Context

GrooveSeek demonstrates useful local retrieval concepts: heading-aware parent/child chunks, SQLite full-text and optional vector candidates, Reciprocal Rank Fusion, optional reranking, MMR diversity, bounded exact citations, golden-query evaluation, MCP composition, and explicit stability boundaries.

GKOS-Engine already owns deterministic parsing, identity, temporal lineage, discoverability, graph construction, and governance behavior. GKOS-Engine-Lite is a JavaScript CLI and Tauri presentation wrapper around a pinned Full engine, not an independent deterministic engine.

## Decision

Adopt the documented retrieval and evaluation concepts through clean-room implementations governed by GKOS-Engine's versioned contracts. Full owns the TypeScript reference implementation and publishes the canonical retrieval, citation, filtering, confidence, evaluation, and graph-query contracts. Under ADR-0005's ratified static-artifact decision, Lite implements those contracts in Rust, pinned to an exact Full contract/reference release and exercised through cross-language conformance. Lite does not fork ranking or GKX semantics, and its Rust implementation is never a separate authority.

GKX remains canonical for stable source identity, lineage, temporal validity, sensitivity, provenance, authority, and discoverability. Retrieval rank, vector similarity, graph centrality, timestamps, model output, or chunk identity cannot create or alter those properties. Derived retrieval and graph stores remain disposable projections. Discoverability is applied before scoring, aggregation, citation, or traversal, and deny, indeterminate, and policy error outcomes fail closed.

FTS-only operation is mandatory. Optional embedding and reranking providers are selected through neutral interfaces and trusted operator configuration as described by ADR-0002. Search reports stage signals rather than presenting fused rank as probability.

The existing loopback-authenticated REST and Tauri boundaries remain compatible. New implementation stays outside NavigationCore and does not add source-note writes.

No GrooveSeek source code or unusually expressive text was copied during Phase 0. If later work copies any material, it must record the exact upstream file and commit, local destination, applicable MIT or Apache-2.0 license, and attribution in `THIRD-PARTY-NOTICES.md`.

## Alternatives rejected

- Port GrooveSeek wholesale. Its authority and storage assumptions are not GKOS contracts.
- Let Lite define independent retrieval contracts or semantics. That would make Lite a fork and create Full/Lite semantic drift.
- Make vectors mandatory. That would break dependency-light and degraded operation.
- Promote retrieval or graph projections to authority. Rank and similarity cannot establish governed facts.

## Consequences

Full must expose versioned retrieval, citation, filtering, confidence, evaluation, and graph-query contracts plus its TypeScript reference behavior. Lite upgrades its Full contract/reference pin only after the Rust implementation passes the required cross-language fixtures. Fixed inputs and provider outputs must rank identically in both products, with byte equality wherever the contract designates deterministic artifacts. Attribution remains unnecessary for clean-room concepts alone but becomes mandatory if copying occurs.

## Status

Accepted — 2026-08-20. ADR-0005 supersedes only this ADR's earlier direct runtime-code-sharing assumption: Lite implements the Full-owned contracts in Rust instead of importing Full's TypeScript retrieval runtime. Contract ownership, reference behavior, parity, and every GKX/GKOS authority constraint remain in force.

## Evidence

- Studied upstream: `alphabet-h/grooveseek` commit `313514b793d12ea5c3b8eedc32fd213212e38d75`.
- Reviewed upstream documents: `docs/retrieval-pipeline.md`, `docs/eval.md`, `docs/citations.md`, `docs/mcp-tools.md`, `docs/behavior.md`, `docs/stability.md`, English ADR-0000 through ADR-0010, `LICENSE-MIT`, and `LICENSE-APACHE`.
- Upstream license: MIT OR Apache-2.0; copyright 2026 koshian.
- [Phase 0 baseline and reconciliation](../evidence/phase-0-baseline.md).
