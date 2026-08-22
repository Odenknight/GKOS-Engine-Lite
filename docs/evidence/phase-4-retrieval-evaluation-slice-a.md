# GKOS-Engine-Lite Phase 4 retrieval-evaluation Slice A evidence

Qualification date: 2026-08-22

Repository: Odenknight/GKOS-Engine-Lite

State: **FROZEN_LOCAL**. This is the exact unstaged implementation candidate
prepared for reciprocal Full review. A Lite implementation commit, push, pull
request, hosted run, and reciprocal-review result are all **UNASSIGNED**. This
document makes no merge, tag, release, deployment, package publication, or
artifact-publication claim.

## Exact coordinates

| Coordinate | Value |
| --- | --- |
| Lite Phase 3 evidence base | `41912fd6db279f1b46e67cb4b88c1f1b4ba86e63` |
| Working branch | `codex/phase-4-retrieval-evaluation` |
| Lite implementation commit | **UNASSIGNED** |
| Lite pull request | **UNASSIGNED** |
| Lite hosted CI | **UNASSIGNED** |
| Full reciprocal review | **UNASSIGNED** |
| Full qualified Slice A implementation | `cac029a5b570135b26f3585bc86f4c9beb00c36d` (signed and DCO; draft PR #29 hosted 12/12 green) |
| Full package | `gkos-engine` 2.1.2 |
| Evaluation contract | `gkos-retrieval-evaluation/1.0.0-draft.1` |
| Full Slice A pack | 37 files / 4,948,463 bytes |
| Local Node runtimes | 22.23.2, 23.11.1, 24.18.0 |
| Rust latest / MSRV | 1.98.0 / 1.85.0 |

The root dependency, lockfile, Phase 4 compatibility fixture, and private
`FULL-PIN.json` all bind the corrected Full implementation commit above. The
37 copied files are byte-for-byte the signed Full Slice A pack; the adjacent
Lite-only pin records every file SHA-256. The pack contract deliberately
remains provisional and excludes host search execution, CLI presentation,
candidate publication, temporal CI, observation execution, and final contract
hashing. This evidence does not promote those excluded slices.

## Lite authority boundary

- The evaluation module is crate-private and has an external compile-fail
  public-surface gate. It exposes no raw or normalized evaluation API to a
  downstream crate.
- The module consumes only Full-produced normalized JSON envelopes, public
  result fixtures, the authoritative integer discount table, comparison
  coordinates, and bounded precomputed tuning-priority tuples.
- It independently verifies canonical digests and cross-envelope relations,
  u128 round-half-up metric math, relevance union and first-source dedupe,
  citation applicability/correctness/staleness, the complete finite policy
  identity traversal, temporal/confidence/projection zero gates, baseline
  comparison, ScenarioOutcome branches, and deterministic tuning order.
- It never parses golden TOML or GKX, reads fixture paths, constructs a source
  corpus, calls a provider or search coordinator, opens a database, selects a
  production candidate, writes state or configuration, or publishes output.
  Full remains the sole parser, host, search, tuning, filesystem, CLI, and
  publication authority.
- Existing Phase 1 public legacy writer APIs and the Phase 2/3 private writer,
  authority, and owner-verifier boundaries are unchanged.

## Executable fixture coverage

- All 37 copied file hashes, aggregate bytes, UTF-8/BOM/CR/terminal-LF rules,
  and the exact Full pin are checked mechanically.
- All 58 metric-computation cases execute independently: 53 exact metrics and
  5 exact finite errors.
- All ScenarioOutcome union branches and comparison/precedence rows execute
  with exact case-set and multiplicity checks.
- All 11 tune-priority cases exercise zero-gate exclusion and every ordered
  tie-break key. The exact 900-candidate grid and bounded tune matrix are
  independently checked.
- Normalized golden, environment/environment-set, provider-role, metrics-set,
  baseline, observation, conformance, reviewed-bundle, corpus/provider, and
  host-only rows are sealed or exhaustively bound without acquiring their raw
  host authority.
- UTF-16 code-unit bounds, well-formed Unicode, ECMAScript edge trim,
  provider/model identity semantics, portable UTF-8 path bounds, lexical
  query grammar, timestamp normalization, Unicode citation spans, and the
  NDCG u128 boundary execute as semantic supplements beyond JSON Schema.

## Local qualification

| Gate | Exact result |
| --- | --- |
| Phase 4 focused Rust verifier | PASS; 11 passed, 0 failed, 0 skipped |
| Rust 1.98.0 Windows GNU workspace all-targets | PASS; 166 library + 11 full-conformance = 177 passed, 0 failed, 0 skipped |
| Rust 1.98.0 docs / fmt / clippy | PASS; 4 docs passed; fmt clean; clippy `-D warnings` clean |
| Rust 1.85.0 Windows GNU MSRV workspace all-targets | PASS; 177 passed, 0 failed, 0 skipped |
| Rust 1.85.0 docs | PASS; 4 passed, 0 failed, 0 skipped |
| Node 22.23.2 root suite | PASS; 38 passed, 0 failed, 0 skipped |
| Node 23.11.1 root suite | PASS; 38 passed, 0 failed, 0 skipped |
| Node 24.18.0 root suite | PASS; 38 passed, 0 failed, 0 skipped |
| Desktop typecheck, tests, production build | PASS; typecheck/build clean; 20 passed, 0 failed, 0 skipped |
| Full Slice A pack | PASS; 37 files / 4,948,463 bytes / zero SHA-256, BOM, CR, UTF-8, or terminal-LF mismatch |
| Root metadata and dependency lock | PASS; Lite/Full package 2.1.2; exact Full SHA; one immutable 40-hex Git dependency |
| Root dry package | PASS; 5 files / 9,266 packed / 24,000 unpacked / SHA-1 `d53bb81f16056643b6cba0182326db21d9744d42` / SHA-512 `L0QEmTJ6tHyqHWD4K9wVw9SfpGcAH73B5hs3InxYYpWpVPHM8gJCgwlFj4ZB9DmCOuMiuyOPocQ55dGYRQWRNw==` |
| Phase 0–3 contract bytes | PASS; no diff from base `41912fd6db279f1b46e67cb4b88c1f1b4ba86e63` |
| Public/export closure | PASS; Phase 4 module is private and downstream import fails to compile |
| Diff, staging, archive | PASS; `git diff --check` clean; staging empty; no `.tgz` |
| Candidate scope | PASS; exactly 48 unstaged paths, including this evidence file |

The local machine has the Rust MSVC target but no Visual Studio Build Tools or
Windows SDK linker; native MSVC stopped before crate compilation with
`link.exe not found`. That is a recorded host capability absence, not a local
pass or skip. The mandatory Windows MSVC all-target, alias, and documentation
qualification remains assigned to hosted CI after the reciprocally approved
implementation is published.

## Freeze boundary

This local freeze is ready only for Full's reciprocal read-only review. The
working tree remains unstaged, uncommitted, and unpushed. Publication is
authorized only after that exact review approves the bytes; subsequent hosted
qualification must reach terminal success before any evidence closeout can
claim DONE.

No merge, tag, release, deployment, package publication, or artifact
publication is authorized or claimed.
