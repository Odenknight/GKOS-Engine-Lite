# ADR-0005: Build Lite as a governed true-static Rust frontend-adapter

## Context

Lite is currently a JavaScript CLI wrapper plus a Tauri 2 presentation shell. Full owns a Node SEA desktop sidecar, while the current Lite desktop bundle carries that sidecar and a viewer asset. That architecture cannot honestly satisfy the owner's explicit requirement that GKOS-Engine-Lite build as one statically linked binary.

The required future matrix is Linux x86_64/aarch64, macOS arm64, and Windows x86_64. Mandatory paths must operate on Sandy Bridge/Ivy Bridge-class AVX hardware without AVX2 or FMA. The current desktop workflow builds unsigned macOS arm64/x86_64 and Windows x86_64 installers only; it does not qualify Linux or the required R720-class host.

## Decision

Build the required GKOS-Engine-Lite artifact as one statically linked Rust frontend-adapter for each supported target. The owner has selected the separately governed true-static option. Here, statically linked means one self-contained executable with all non-system runtime and library dependencies linked into it; unavoidable operating-system ABI/framework dependencies are documented platform prerequisites, not adjacent distributed assets. The mandatory executable must run without an adjacent language runtime, non-system dynamic native library, or packaged sidecar. Required notices, checksums, and documentation may accompany the executable in a distribution archive, but the archive is not itself called a static binary.

Full remains the owner of GKX semantics and the versioned retrieval, citation, filtering, confidence, evaluation, and graph-query contracts. The Rust implementation is a frontend/storage/transport adapter to those contracts, never a second GKX schema or deterministic authority. It must pass exact cross-language conformance fixtures for canonical inputs, including deterministic bytes where the Full contract requires byte equality and contract-equivalent results elsewhere. It cannot add identity, lineage, temporal, validation, sensitivity, discoverability, relationship, or governance semantics.

FTS-only is the mandatory dependency-light path. Optional vector/model assets remain feature- and configuration-gated until each target passes license, CPU-feature, packaging, and crash-safety qualification. AVX2/FMA acceleration requires runtime detection and a tested AVX fallback.

The mandatory FTS path and its SQLite/FTS5 support must be statically linked and dependency-light. Optional operator-configured network providers do not become packaged runtimes. An optional local model file is runtime input, must be declared honestly, and cannot be required for FTS-only operation. No native vector or local inference dependency may enter a mandatory path until it passes license, target, crash-safety, and AVX-only qualification.

Retain the published `okf-lite` executable as a compatibility alias at this baseline. All new commands, contracts, schemas, and documentation use GKOS-Engine and GKX names. Removal or versioned deprecation of the published alias requires separate owner authority; no new legacy surfaces are added.

## Alternatives rejected

- Use the Node SEA bundle as the required Lite artifact. It does not meet the ratified true-static requirement.
- Claim the current Tauri bundle or an installable archive is a static single file. It contains an external sidecar and viewer resource.
- Create an independent Rust GKX or retrieval authority. Rust must implement the Full-owned contracts under exact conformance rather than inventing semantics.
- Make local vectors mandatory. Native/model packaging is not yet qualified.
- Remove the published compatibility executable during reconnaissance. Its migration policy is not implicit.

## Consequences

This choice expands implementation and qualification scope. Before feature work, Full must publish the cross-language contracts and fixtures needed to distinguish adapter conformance from a semantic fork. Rust storage, ranking, transport, and packaging work must stay synchronized with those versioned contracts.

Phase 9 must add honest per-target artifact inventories, checksums, notices, clean-machine results, signing status, and real AVX-only hardware evidence before distribution claims. Reproducible Rust dependency resolution requires a committed lockfile. The current Node/Tauri installers remain stale, unqualified compatibility artifacts and do not satisfy this ADR.

## Status

Accepted — 2026-08-20. The owner's explicit requirement that GKOS-Engine-Lite build as one statically linked binary ratifies the governed Rust/static frontend-adapter option.

## Evidence

- Lite baseline `package.json`, `bin/okf-lite.mjs`, `desktop/src-tauri/tauri.conf.json`, and `.github/workflows/desktop-build.yml` at `2ebbf77583af3e83032054f1256188dc56376907`.
- Current workflow matrix and published artifact assessment in [Phase 0 baseline and reconciliation](../evidence/phase-0-baseline.md).
- Rust baseline used rustc/cargo 1.98.0 but could not link because this host lacks the MSVC linker and Windows SDK. The untouched baseline had no `Cargo.lock`; Phase 0 now adds a generated lockfile and a `--locked` hosted native test gate, whose exact run is still pending.
- Owner direction: “Engine-Lite builds as one statically linked binary.”
