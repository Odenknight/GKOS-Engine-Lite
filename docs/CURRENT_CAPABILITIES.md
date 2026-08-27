# GKOS-Engine-Lite current capabilities

Status date: 2026-08-27

This inventory describes the functional-uplift implementation qualified at
`41912fd6db279f1b46e67cb4b88c1f1b4ba86e63` and admitted through PR #22 from
pre-admission Lite `main`
`2ebbf77583af3e83032054f1256188dc56376907`. The `v2.1.2` tag identifies the
merged JavaScript/Rust source release. It does not publish or qualify a
one-file static Lite distribution or a Desktop installer release.

## Capability matrix

| Capability | Surface | Standing | Authority and effects boundary |
| --- | --- | --- | --- |
| GKX validation | `okf-lite validate` | Implemented as an exact delegate to pinned Full | Full owns parsing, projection, profiles, and diagnostics. Lite forwards arguments and does not parse YAML or TOML. |
| Ingest validation and indexing | `okf-lite index` | Implemented on the Phase-3 branch as an exact delegate | Full owns validation, generation publication, activation, journals, and filesystem authority. Indexing may write sealed derived state; source-note bytes are never changed. |
| Assessment | `okf-lite assess` | Implemented as a restricted Full delegate | Documentation-quality assessment only; no approval or write authority. |
| Retrieval | `okf-lite search`, including `--as-of` | Implemented as a restricted Full delegate | Lexical baseline, optional trusted provider configuration, temporal search, and exact verified citations through the pinned Full executable. |
| Canonical graph | `okf-lite graph` | Implemented as a restricted Full delegate | Writes only the explicitly named derived graph output; GKX source remains canonical. |
| Graphiti export | `okf-lite export graphiti` | Implemented as a restricted Full delegate | Deterministic projection, not a second source of truth. |
| Proposal-only assistance | `okf-lite assist` | Optional | Loopback intelligence sidecar; engine-validated candidates are labeled non-authoritative and have no automatic note-write path. |
| Rust retrieval core | `rust/` library and conformance boundary | Phase 1 qualified | Pin-bound implementation of Full-owned retrieval contracts, SQLite/FTS5 generations, provider-neutral adapters, ranking, filtering, and citation verification. It is not an independent GKX parser or authority. |
| Lineage and citation preservation | Rust contracts, fixtures, and CLI equivalence gates | Phase 2 qualified | Preserves Full-minted identity, authored/effective lineage, temporal state, sensitivity, and verified citation bindings without minting new authority. |
| Ingest envelope validation | Private Rust ingest modules and frozen Full pack | Phase 3 qualified | Validates strict canonical JSON envelopes from Full. No public raw-ingest API and no Lite owner-plane writer, pointer publisher, or activation authority. |
| Desktop presentation | `desktop/` Tauri package | Source and CI workflows present | Installer workflows build unsigned artifacts. Current Engine integration still requires fresh installer-matrix and clean-machine qualification before availability is claimed. |
| JavaScript package | `gkos-engine-lite` 2.1.2 | Thin CLI package | Published files are limited to `bin/`; the engine dependency is pinned to Full commit `e7cc0dd478af3d0bda216c5258dec5f77932def7`. |

## Exact command boundary

The delegated GKX command set is exactly:

1. `validate`
2. `index`
3. `assess`
4. `search`
5. `graph`
6. `export graphiti`

`assist` is a separate proposal-only surface. Full commands such as migration,
proposal/decision mutation, move, and service hosting remain blocked by the
Lite wrapper.

## Explicitly absent or deferred

- An independent Lite GKX parser, identity authority, lineage resolver,
  temporal authority, classification authority, governance store, or
  Navigation authority.
- A public Rust ingest API or Lite-owned Phase-3 writer/activation plane.
- Automatic source-note mutation, automatic sensitivity lowering, or
  self-approval.
- A completed one-file static executable distribution.
- Platform, CPU, installer, signing, notarization, or published-artifact claims
  for the future static Rust path.
- A claim that Phase 0–3 qualification alone establishes static artifact,
  installer, or Desktop availability.
- A claim that the pinned Full commit equals current Full `main` or a matching
  release tag.

## Qualification evidence

- [Phase 0 baseline](evidence/phase-0-baseline.md)
- [Phase 1 retrieval core](evidence/phase-1-retrieval-core.md)
- [Phase 2 lineage and citations](evidence/phase-2-lineage-citations.md)
- [Phase 3 ingest validation](evidence/phase-3-ingest-validation.md)

The Phase-3 evidence records successful Node 22–24 root suites, desktop checks,
Rust Windows GNU, native Linux, MSRV, frozen-pack, metadata, package, boundary,
and reciprocal-review gates. Refer to that evidence for exact test counts and
signed implementation coordinates.
