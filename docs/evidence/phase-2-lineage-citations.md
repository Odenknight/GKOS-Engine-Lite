# Phase 2 lineage and citation evidence

Date: 2026-08-21

Repository: `Odenknight/GKOS-Engine-Lite`

Qualification status: **DONE**

Phase 2 is implemented and qualified against Full's exact signed,
hosted-green draft.2 implementation and contract pack. Owner-ratified Decision
A governs every cross-record class. The JavaScript wrapper, npm lock, contract
pin, and Rust conformance gate all bind Full commit
`6e2df27d33ede62ee0d2e3cb7610df478a7d66ce`; point-in-time command execution
parity is proven locally and the complete Lite hosted matrix is green. No
merge, tag, release, deployment, package or artifact publication, provider
provisioning, or source-content mutation is claimed.

## Coordinates

| Subject | Current coordinate | Disposition |
|---|---|---|
| Lite branch | `codex/phase-2-lineage-citations` | Published branch; no merge or release |
| Lite Phase 1 evidence base | `eda2e2105b41de683dfce88b5666443145682a5e` | Qualified and unchanged |
| Lite Phase 2 implementation | `42df8b047797a725f8b7c31f2f02d123a798515e` | Signed, DCO-compliant, and hosted-green |
| Lite evidence-only closeout | Not assigned at qualification time | A later one-file head must not replace the implementation coordinate |
| Lite draft pull request | `Odenknight/GKOS-Engine-Lite#17` | Open draft; unmerged |
| Lite hosted CI | PR run `32520112318` | 8/8 jobs green |
| Full Phase 2 implementation | `6e2df27d33ede62ee0d2e3cb7610df478a7d66ce` | Signed and hosted-green |
| Full draft pull request | `Odenknight/GKOS-Engine#27` | Open draft; unmerged |
| Full hosted CI | push `32518027278`; PR `32518043098` | 12/12 jobs green |
| Wrapper Full dependency pin | `6e2df27d33ede62ee0d2e3cb7610df478a7d66ce` | Exact Phase 2 package pin; package `2.1.2` |
| Retrieval/result contracts | `gkos-retrieval/1.0.0-draft.2`; `gkos-retrieval-result/1.0.0-draft.2` | Frozen at the signed Full commit |
| Provenance contract | `gkos-retrieval-provenance/1.0.0-draft.1` | Frozen at the signed Full commit |
| Projection schema | 3 | Additive; schema 2 behavior remains exact |
| Standard/profile binding | `a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6`; `gkx-2.3-validating-projection` | Full-owned canonical coordinate |

`rust/contracts/gkos-retrieval-1.0.0-draft.2/FULL-PIN.json` records the exact
eight frozen hashes, reference commit
`6e2df27d33ede62ee0d2e3cb7610df478a7d66ce`, and
`publication_qualified:true`. Lite's independent implementation and hosted
qualification are now complete, so the terminal phase state is `DONE`.

## Exact signed Full pack

All eight Full files are copied byte-for-byte, parse as JSON where applicable,
and have exactly one terminal LF. The Rust executable fixture asserts the
complete FTS5 manifest and result envelopes, the ratified authorization
matrix, and these exact hashes.

| File | Bytes | SHA-256 |
|---|---:|---|
| `chunk.schema.json` | 3,535 | `d1ffd008bf360807d50494bc34610670732ee9fb61ed15fcd8f0aee7496a6fab` |
| `conformance-fixture.json` | 35,230 | `eb4b77590ae3d113a129f5f9baa7adb77737789d3fc1dfbcd8e9aa6ec353ae61` |
| `contract.json` | 9,657 | `203ba5d54e1eeecd88a4d706f0394f9b517667194bb78e4f197911ac0358d4d5` |
| `projection.schema.json` | 2,512 | `97ae4481f3780536de4ba743fc0f7067f342f6ca8ce69b8a976ed1894a5ee753` |
| `provenance.schema.json` | 3,014 | `bcad32df33e5fe3e28aa85f4674b7c9eec92bf6e05c14a7d8a1858808231fb84` |
| `README.md` | 11,601 | `cf983c2e6269a856aece443f9cf16c0fafbe024ebc00714154838c7e0a7618ec` |
| `result.schema.json` | 5,940 | `a12f1ba4a25ab746425fc279425e92579812beb5879d1b8d778571539198eb97` |
| `stored-provenance.schema.json` | 4,798 | `de3261a093e65cba11cc05490947c914a9a3f880183b0a1cf2f52ef1dfd5a861` |

## Implemented Decision-A scope

- Rust accepts only trusted host-supplied immutable candidate sources,
  candidate chunks, opaque record/chunk keys, and parser-owned canonical
  resolution-tier receipts. It does not parse GKX, assign identity, reparse a
  raw reference, choose a canonical resolver tier, or become a second GKX
  authority.
- Schema 3 persists physical candidate sources, declarations, chunks, FTS
  rows, policy-bound embedding eligibility, and vectors under opaque keys.
  Duplicate public identities and valid zero-chunk sources remain physical
  inputs until the authorization-scoped view is known. Exact counts, canonical
  JSON, digests, FTS key bijection/tokenizer DDL, vector spaces, parent/source
  bindings, and all immutable reopen invariants are verified. Schema 2 remains
  unchanged.
- Search order is runtime policy digest, source policy, typed filters,
  candidate-key SQL chunk fetch, whole-source chunk policy, candidate-key SQL
  receipt fetch, explicit-`as_of` known/future/unknown partition, and only then
  authorization-scoped identity, resolution, declaration, topology, and
  temporal derivation. Hidden chunk text and raw receipts never cross SQLite
  before their policy gates.
- `as_of` uses the current GKX timestamp grammar, normalizes to UTC within the
  four-digit result range, and selects half-open
  `valid_from <= as_of < valid_to` intervals. Unknown validity never becomes
  all-time and contributes only coverage; future candidates and endpoints are
  suppressed.
- Decision A makes hidden or future candidates byte-identical to physical
  absence in the complete ordinary result. Any conflict among authorized
  known-created candidates in the four ratified classes returns only
  `RETRIEVAL_AUTHORIZED_VIEW_CONFLICT`, before live reads, query providers,
  lexical/vector SQL, ranking, counts, confidence, citations, or parents. A
  known conflict takes precedence over simultaneous insufficient coverage.
- Candidate-key lexical/vector SQL sees only the scoped eligible set. The
  complete temporal candidate-key set must match persisted policy-bound vector
  eligibility before a live source read or query provider call, so stale
  suppression cannot hide a missing eligibility row.
- Result coordinates bind the scoped provenance origins, interval/state,
  safe metadata, lineage-neutral value, authorized non-future canonical
  endpoints, `lineage_id:null`, and `ledger_binding_verified:false`. No ledger
  hash, physical count, opaque key, parser receipt, or raw reference is public.
- Hits and content-bearing parents bind exact live UTF-8 bytes, SHA-256 source
  and content digests, zero-based half-open byte ranges, recomputed one-based
  inclusive line coordinates, normalized `as_of`, and scoped provenance.
- Immutable path, pointer, sidecar, hard-link, cache-authority, provider
  identity, metadata-quality, duplicate-content-vector, and tamper checks fail
  closed. Provider selection remains neutral among trusted configured
  OpenAI-compatible, local ONNX, and MCP families; no vendor/model/host/route
  allowlist or preference was introduced.

## Ratified authorization matrix

The final fixture freezes all four owner-ratified classes:

| Class | Scoped outcome |
|---|---|
| Canonical identity collisions | Hidden/future equals absence; all authorized conflicts generically |
| Endpoint resolution | Canonical receipt-tier fallthrough over the scoped set; unresolved/ambiguous governed declarations conflict generically |
| Forward/inverse/conflicting declarations | Hidden/future equals absence; all authorized conflicts generically |
| Multiple successor, cycle, and temporal order | Hidden/future equals absence; all authorized conflicts generically |

The cycle check is iterative and handles a 25,000-record valid chain without
recursive stack growth. Ordinary broken links remain outside governed lineage
conflicts, and intrinsic malformed candidates still reject before publication.

## Frontend boundary

The additive help syntax is
`search <query> --kb-path <dir> --as-of <GKX timestamp>`. Lite preserves the
original flag and value byte-for-byte at the delegation boundary. The exact
Phase 2 dependency executes an identical corpus and trusted configuration
through Lite and Full with equal exit status and byte-identical stdout/stderr,
apart from the already-approved normalization of only the PID on the exact
Node SQLite experimental-warning line. The test also proves the source bytes
are unchanged. Phase 0/1 help, graph, Graphiti, and compatibility goldens
remain immutable; the Phase 2 migration fixture permits only the exact Full
dependency/resolved-SHA coordinate, the authorized help-line delta, and the
draft.2 retrieval-result contract transition.

## Reciprocal review and debugging record

Full and Lite reciprocally reviewed the canonical candidate layer, candidate
persistence, authorized view, coordinator ordering, provenance/citation seal,
and final contract pack. Corrections included failure-atomic canonical ledger
updates, Windows 8.3-safe alias handling, hidden-global-validity removal,
strict receipt/source/chunk/vector/FTS reopen checks, candidate-named manifest
counts, duplicate-content vector consistency, exact public-coordinate fields,
unknown-only coverage, iterative cycle detection, record-key-scoped policy
reads, and complete pre-live vector-eligibility verification. The obsolete
pre-Decision-A shadow store and one malformed in-worktree toolchain artifact
were verified untracked and removed before qualification.
The final differential review also added complete coordinator-envelope
coverage for future identity and all four unknown-only classes, proved
known-conflict precedence with zero live/provider/rerank work, and corrected
the current-view interval so an unknown-validity source remains
`valid_to:null` even when its scoped lineage edge is visible.

Full's final read-only reciprocal review approved the exact 34-path Lite local
freeze with no blocker, HIGH, or MEDIUM finding. Lite's reciprocal review
approved Full's final Decision-A implementation and frozen pack before Full
published its signed Phase 2 commit. Those approvals precede the qualified
implementation coordinates above; this later evidence-only closeout changes no
code, contract byte, dependency pin, package metadata, or compatibility
fixture.

Full's final local freeze independently passed Node `22.23.2`, `23.11.1`, and
`24.18.0` typecheck/build plus 404/404 repository tests, 143/143 retrieval
tests, and 44/44 Navigation tests on each runtime, all with zero skipped. Its
schema/reference gate was 18/18, configuration/credential gate 13/13,
intelligence 4/4, and sequential package gate 231 files / 696,216 bytes packed
/ 2,923,970 bytes unpacked.

Signed Full commit `6e2df27d33ede62ee0d2e3cb7610df478a7d66ce`
is published on draft PR #27. Push run `32518027278` passed build jobs
`96883933030`, `96883933079`, `96883933039` and mandatory Windows path-security
jobs `96883933168`, `96883932749`, `96883933130` for Node 22/23/24. Pull-request
run `32518043098` passed build jobs `96883981685`, `96883981904`, `96883981786`
and Windows jobs `96883981474`, `96883981793`, `96883981725`. Both runs are
complete and successful; the PR remains draft and unmerged.

Signed, DCO-compliant Lite implementation commit
`42df8b047797a725f8b7c31f2f02d123a798515e` is published on draft PR #17.
Pull-request run `32520112318` passed all eight required jobs:

- Node 22 `test (22)` — `96890260101`;
- Node 23 `test (23)` — `96890260007`;
- Node 24 `test (24)` — `96890259890`;
- Rust MSRV — `96890259718`;
- Rust latest — `96890259900`;
- Windows MSVC retrieval, including the mandatory alias fixture —
  `96890259952`;
- desktop frontend — `96890259855`;
- desktop native — `96890259986`.

The run completed successfully at the exact implementation SHA. PR #17
remains draft and unmerged. The future commit that records this one-file
evidence closeout is evidence-only and is not the qualified implementation
coordinate.

## Current Lite local verification

| Gate | Exact result |
|---|---|
| Rust `1.98.0` GNU all-target check/test | PASS; 121 unit + 11 Phase 1 conformance, 0 failed/ignored |
| Rust `1.98.0` GNU fmt/clippy/docs | PASS; clippy `-D warnings`; 2/2 compile-fail doc tests |
| Rust `1.85.0` GNU all-target check/test | PASS; 121 unit + 11 Phase 1 conformance, 0 failed/ignored |
| Rust `1.85.0` GNU docs | PASS; 2/2 compile-fail doc tests |
| Frozen draft.2 executable fixture | PASS; complete bundled-FTS5 manifest/result canonical equality and exact 8-file hash/LF gate |
| Node `22.23.2` root tests | PASS; 32 passed, 0 failed, 0 skipped |
| Node `23.11.1` root tests | PASS; 32 passed, 0 failed, 0 skipped |
| Node `24.18.0` root tests | PASS; 32 passed, 0 failed, 0 skipped |
| `npm ci` / metadata / lock | PASS; exact Phase 2 Full SHA, package `2.1.2`, and one 40-hex git dependency |
| `npm pack --dry-run --json` | PASS; 5 files, 8,645 bytes packed, 21,959 unpacked, SHA-1 `3a63ca3649c2b649c1a437eb72f7fb62b4dfa030` |
| Desktop typecheck/test/build | PASS; 20/20 tests; Vite built 15 modules |
| Draft.2 byte/hash/one-LF scan | PASS; 8/8 exact |
| Provider/authority/action/Cargo-source/stale-state/merge-marker scans | PASS |
| `git diff --check` | PASS |

## Terminal qualification

The final exact-pinned Lite delta received reciprocal read-only approval,
published as a signed and DCO-compliant implementation commit on draft PR #17,
and passed every required hosted Ubuntu and Windows job. Phase 2 is therefore
terminal **DONE**.

This qualification is not a merge, tag, release, deployment, package publish,
or artifact publication. Any later evidence-only branch head records the
qualification but does not supersede implementation commit
`42df8b047797a725f8b7c31f2f02d123a798515e` or Full pin
`6e2df27d33ede62ee0d2e3cb7610df478a7d66ce`.
