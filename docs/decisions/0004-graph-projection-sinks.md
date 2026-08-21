# ADR-0004: Project the canonical graph through replaceable sinks

## Context

GKOS-Engine already builds a deterministic canonical graph and a non-authoritative Graphiti episode projection. Lite consumes those Full behaviors. The uplift needs a dependency-light graph database and bounded agent graph tools without creating a second graph authority or changing existing Graphiti bytes.

## Decision

Full defines and owns a versioned `GraphProjectionSink` contract. A built-in SQLite graph projection is the dependency-light default. An optional Graphiti sink wraps the existing Graphiti episode builders without changing their public exports, identity, temporal ordering, or non-authoritative status. Lite consumes the same sink orchestration and graph-query contracts through Full.

Every projection manifest binds vault, engine and contract versions, source snapshot digest, canonical GKX graph digest, policy/config digests, schema and sink versions, counts, and a completion digest. Incremental updates consume Full's canonical `GraphDelta`; full replacement is used when that delta requires a rebuild. A clean rebuild and equivalent incremental replay must converge on the same canonical projection digest.

Authored and deterministic-derived edges remain distinguishable. Similarity-derived edges are stored separately or carry `origin = "similarity-derived"`, embedding projection identity, and score. They never become authored relationship types or authority. Traversal applies DiscoverabilityPolicy at every seed, node, and edge, suppresses incident edges to hidden nodes, and enforces depth, fan-out, node, and time bounds.

## Alternatives rejected

- Replace the canonical graph with SQLite or Graphiti. Both stores are disposable projections.
- Duplicate graph semantics in Lite or Tauri. Presentation must not become a schema authority.
- Merge similarity edges into authored relationship types. Similarity is a discovery hint, not a governed assertion.
- Expose arbitrary SQL or graph query languages to agents. Bounded typed tools are the public boundary.

## Consequences

SQLite graph state can be rebuilt locally and queried without an external service. Graphiti remains optional and compatible. Policy filtering may reduce result counts and paths without hidden placeholders. Source-note relationship writeback remains out of scope.

## Status

Accepted — 2026-08-20.

## Evidence

- Pinned Full baseline `2fbd4ec68ec825b09e5194c9878a7ae90a281392`: `src/graph.ts`, `src/graphiti.ts`, `src/graphiti-adapter.ts`, and `src/incremental.ts`.
- Current Lite deterministic graph and Graphiti compatibility bytes under `test/fixtures/compatibility/`.
- [Phase 0 baseline and reconciliation](../evidence/phase-0-baseline.md).
