# Phase 1 retrieval core evidence (draft)

Date: 2026-08-21

Repository: `Odenknight/GKOS-Engine-Lite`

Qualification status: **ONGOING — terminal phase state not assigned**

This is a pre-commit evidence draft for the owner-ratified Rust/static Lite
retrieval path. It records completed local implementation and verification but
does not assign `DONE`, `BLOCKED`, or `NEEDS_HUMAN` while hosted qualification
and final reciprocal review remain open. Nothing described here was staged,
committed, pushed, merged, released,
deployed, or published by this Phase 1 worktree.

## Exact coordinates

| Subject | Exact coordinate | Disposition |
|---|---|---|
| Lite branch | `codex/phase-1-retrieval-core` | Phase 1 implementation branch |
| Lite branch base/current committed `HEAD` | `83b0baac033f469abe508802faad7f6b3873ade9` | Phase 1 changes remain uncommitted in the worktree |
| Full reference repository | `Odenknight/GKOS-Engine` | TypeScript reference implementation and contract owner |
| Full reference commit | `5b72aae1aad5b6416b8cb86a4137a7e536d8bb59` | Signed, pushed Full Phase 1 reference; package version `2.1.2` |
| Retrieval contract | `gkos-retrieval/1.0.0-draft.1` | Draft integration contract, not a GKX Standard version or authority claim |
| Rust workspace version / MSRV | `0.1.0` / Rust `1.85` | Future static frontend-adapter foundation under `rust/` |
| Local reviewed current Rust | `rustc 1.98.0 (88d9e12ae 2026-08-18)` | Both GNU and MSVC toolchains are isolated beneath the uplift workspace |

`rust/contracts/gkos-retrieval-1.0.0-draft.1/FULL-PIN.json` binds the exact
Full implementation commit, package version, and every copied contract byte.

## Frozen Full contract pack

All copied contract files have exactly one terminal LF. Local byte-hash checks
and the Rust `imported_full_contract_pack_has_the_pinned_exact_bytes`
conformance test verified these values:

| File | SHA-256 |
|---|---|
| `README.md` | `2028882032f2292bd0bbc937016a128449babf5f9adc395824029ee0047cc942` |
| `canonical-fixture.json` | `f30dd5c3e71407e6544b9c727ff5597c4809936dbbd14a5fdca87dcb99031db2` |
| `chunk.schema.json` | `2474a40e8abc930cbc6e713aa8966be41f0fba87c5b38c5868b42403e8f3f721` |
| `conformance-fixture.json` | `462de9f327585ec2eed019a4b728403b6c0bbfa3ade158ed618cafd214a4b009` |
| `contract.json` | `418fffcf3954c634453c3f3e8dd756dd2636ee0030d9ecf6c4ccb5147b8d0c6e` |
| `gkos-config.schema.json` | `e42fe89d102ec602b0738aa01a3c8f98cc8fb55d9edc25265f6feb168a0ca8d6` |
| `gkos-toml-lexical-fixture.json` | `abdb26527fd5c047db96801c22ebf30efca544fe306744c666b858ba57bd039b` |
| `projection.schema.json` | `99f7eb70530dd44866c8c28b71f97d9d76af2e00b8ea399dc4f2e5f3e6467fa3` |
| `result.schema.json` | `b9bb7e360fa04ee1e0b75984d313cd63488ef698149ff0b3f29b3c003126faa3` |

Full froze these bytes at the signed reference commit after Node 22, 23, and 24
qualification. Projection schema 2 records the actual lexical backend. Lite
publishes `sqlite_fts5`; it can decode but deliberately does not implement the
Node-only `sqlite_lexical_scan` compatibility backend.

## Authority and implementation boundary

- Full's TypeScript implementation remains the reference. Lite's Rust code is
  a pin-bound, cross-language contract implementation for the owner-required
  future static executable path.
- The Rust workspace is not a second GKX parser, identity source, lineage
  resolver, temporal authority, classification authority, governance store, or
  NavigationCore. It accepts canonical `RetrievalSource` envelopes and never
  infers identity, lineage, validity, authority, supersession, discoverability,
  or write permission.
- The Tauri presentation shell was not rewritten. The published JavaScript
  wrapper is pinned to Full commit `5b72aae1aad5b6416b8cb86a4137a7e536d8bb59`
  and delegates the prior read-only boundary plus additive `search`; all other
  Full commands remain blocked.
- The new Rust workspace currently supplies a library and conformance boundary,
  not a completed one-file distribution. Static artifact assembly and platform,
  CPU, signing, and installer qualification remain later packaging work.
- Bundled SQLite supplies the dependency-light immutable derived store and FTS5
  path. FTS-only indexing and search require no vector provider, model, or model
  runtime.
- Provider routing is neutral and selected only from trusted operator
  configuration. `openai_compatible`, `local_onnx`, and upstream `mcp` vector
  and rerank adapters share the same contract. There is no vendor, host, domain,
  model, or route allowlist and no ranking preference for one provider family.
- An unavailable selected vector provider degrades explicitly to a coherent
  FTS-only generation. An unavailable optional reranker skips only that stage.
  Provider/model/dimension mismatches and persisted vector-space corruption are
  hard errors; there is no silent fallback across embedding spaces.
- No provider-specific adapter, constraint, or privileged route is present in
  the Phase 1 Rust implementation or CI workflow. No inference, vector,
  database, or remote service was provisioned.

## Implemented Phase 1 scope

The Rust workspace implements the frozen contract structs, strict canonical
source envelopes, deterministic heading/setext chunking, exact UTF-8 byte and
LF/CRLF line coordinates, stable path-independent chunk identities, bundled
SQLite/FTS5 generations, verified immutable active pointers, prior-generation
embedding-cache reuse, deterministic RRF and MMR, overlap evidence collapse,
typed filters, confidence and stage envelopes, bounded parent expansion,
provider-neutral configuration/adapters, provider deadlines, source-level
discoverability, exact live citation verification, result budgets, and sealed
external result redaction.

The Phase 1 pin migration preserves the immutable Phase 0 fixtures. Separate
Phase 1 byte goldens and `phase1-lite.json` classify the authorized Full
1.1.3-to-2.1.2 GKX namespace/profile migration. An executable exact diff-path
allowlist and semantic invariants prove stable UIDs, paths, sensitivity,
authored/effective lineage, temporal state, scores, source hashes, Graphiti
episode order, relationship targets, and source content. Historical
`okf_version` remains raw compatibility input; current output reports it as
legacy with `GKX-SCHEMA-003` rather than rewriting the source.

Derived stores reject aliases, hard links, unverified sidecars, corrupt or
mixed manifests, partial vector spaces, inconsistent source envelopes, and
forged parent bindings. Parent integrity is checked both before publication and
after reopening persisted state: each child must name the nearest structural
ancestor's first chunk. Relationship-bearing and unknown metadata are retained
only in the immutable projection digest and suppressed from Phase 1 external
results until later authorized endpoint resolvers exist.

## Reciprocal review and debugging record

Full and Lite owners reviewed one another's Phase 1 trees while implementation
was still uncommitted. Findings were fixed rather than classified as harmless.
The principal corrections were:

- exact Full/Lite setext behavior, blank/frontmatter-only represented-source
  counts, RRF/MMR and negative-cosine rules, parent token threshold, parent-first
  result budgeting, normalized Unicode citation spans, and overlap de-duplication;
- strict canonical JSON and strict shared TOML-subset behavior, including
  unsafe numbers, UTF-16 key order, invalid Unicode, duplicate sections, and
  presence-preserving configuration digests;
- provider deadlines and cancellation for indexing, query embedding, and
  reranking; exact model/item/dimension/correlation/index validation; secret-
  redacted request/provider debug surfaces; and configuration-only swapping
  among all three neutral provider families;
- trusted configuration discovery that rejects vault-local routing/credential
  shadowing, endpoint userinfo, absent configured secrets, aliased roots, and
  untrusted workspace discovery while allowing arbitrary operator-selected
  HTTP(S) hosts, local model paths, and MCP tools;
- coordinator-only public search, source-level policy eligibility before SQL
  scoring, policy-eligible vector joins, relationship/agent metadata redaction,
  hidden-ID filter rejection, typed-filter and raw-query resource bounds, and
  non-forgeable authorized result envelopes;
- immutable generation/sidecar/pointer/path hardening, corrupt-cache-as-miss
  behavior, all-or-none vector publication, unchanged-content cache reuse, and
  exact live byte, line, span, parent, and source bindings;
- symmetric persisted-parent verification. SQLite tamper/reopen tests now reject
  a sibling parent, missing parent, extra root parent, and non-first parent part,
  while a canonical skipped heading depth remains valid.

Lite's review found and Full corrected three final defects: persisted vector
store failures had been swallowed as provider degradation, malformed JavaScript
source envelopes were coercively transformed, and parent validation accepted a
shallower sibling or non-first parent. A later Node 23 compatibility review
also required honest runtime backend identity, policy-first fallback scanning,
strict cross-backend query/citation semantics, and explicit degraded reason
codes. Lite independently reran the frozen Full focused suite under Node 23 at
43/43 and approved that delta with no remaining blocking, high, or medium
findings. Full previously approved the Rust implementation; final reciprocal
review of the exact wrapper/pin migration remains part of the open qualification.

No source-note fixture contains PHI-adjacent or organization-confidential
material. Provider request debug tests use synthetic sentinels and prove query,
note text, routes, and credentials are not formatted into ordinary debug output.

## Exact local verification

The GNU toolchains were selected explicitly from the isolated workspace
toolchain directory so local verification could link without changing the
machine-wide toolchain or `PATH`.

| Command | Exact result |
|---|---|
| `cargo +1.98.0-x86_64-pc-windows-gnu test --manifest-path rust/Cargo.toml --workspace --all-targets --locked` | PASS; 89 unit tests and 11 Full-contract conformance tests, 0 failures, 0 skipped |
| `cargo +1.98.0-x86_64-pc-windows-gnu check --manifest-path rust/Cargo.toml --workspace --all-targets --locked` | PASS |
| `cargo +1.98.0-x86_64-pc-windows-gnu clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo +1.98.0-x86_64-pc-windows-gnu test --manifest-path rust/Cargo.toml --doc --workspace --locked` | PASS; 2 compile-fail API-sealing doc tests |
| `cargo +1.98.0-x86_64-pc-windows-gnu fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS |
| `cargo +1.85.0-x86_64-pc-windows-gnu test --manifest-path rust/Cargo.toml --workspace --all-targets --locked` | PASS; 89 unit tests and 11 Full-contract conformance tests, 0 failures, 0 skipped |
| `cargo +1.85.0-x86_64-pc-windows-gnu check --manifest-path rust/Cargo.toml --workspace --all-targets --locked` | PASS |
| `cargo +1.85.0-x86_64-pc-windows-gnu test --manifest-path rust/Cargo.toml --doc --workspace --locked` | PASS; 2 compile-fail API-sealing doc tests |
| `npm run check:metadata` | PASS; Lite `2.1.2`, Engine package `2.1.2` at exact commit `5b72aae1aad5b6416b8cb86a4137a7e536d8bb59`, Apache-2.0 |
| `node scripts/check-lockfile-sha.mjs` | PASS; one git dependency pinned to a 40-hex commit SHA |
| Node `24.18.0`: `npm test` | PASS; 30 tests, 0 failures, 0 skipped, including Full/Lite `search` stdout and stderr parity |
| Node `23.11.1`: `node --test "test/*.test.mjs"` | PASS; 30 tests, 0 failures, 0 skipped; Full's explicit degraded lexical backend passes through unchanged |
| Node `22.23.2`: `node --test "test/*.test.mjs"` | PASS; 30 tests, 0 failures, 0 skipped |
| `npm pack --dry-run --json` | PASS; 5 files, 8,347-byte tarball, 21,069 bytes unpacked, SHA-1 `965ba2056063645c80e6ceae0ec3deb7f07cc5c9` |
| `npm --prefix desktop run typecheck` | PASS |
| `npm --prefix desktop test` | PASS; 20 tests, 0 failures, 0 skipped |
| `npm --prefix desktop run build` | PASS; Vite 8.1.5 built 15 modules |
| Node 23 frozen Full focused retrieval/CLI run in the read-only Full worktree | PASS; 43 tests, 0 failures, 0 skipped |
| `git diff --check` | PASS |

The root 30-test result validates the migrated published wrapper boundary and
execution-compares `search` against the exact pinned Engine CLI for the same
corpus. Raw stderr is retained. The comparison normalizes only the numeric PID
prefix on the exact Node SQLite experimental-warning line; it preserves that
warning's text and every unrelated stderr byte. A focused negative test proves
an unrelated warning remains byte-significant. The five-file npm payload
remains the JavaScript package; it is not the future static artifact.

Additional dependency and workflow scans passed:

- all nine Full contract files matched `FULL-PIN.json` by SHA-256;
- all 17 `uses:` occurrences in `.github/workflows/ci.yml` use immutable
  40-hex action revisions; no mutable action tag was introduced;
- `rust/Cargo.lock` contains 70 registry source entries and 70 matching
  SHA-256 checksums, with zero git sources;
- the Phase 1 Rust tree and CI workflow contain no removed provider-specific
  reference;
- the bundled SQLite runtime smoke test confirms `ENABLE_FTS5`, creates an FTS5
  table, and executes an FTS-only query;
- documentation trailing-whitespace and forbidden-shorthand scans, contract
  hashes, LF attributes, and `git diff --check` are rerun as handoff hygiene.

## Local MSVC limitation

The exact current-tree command:

```text
cargo +1.98.0 test --manifest-path rust/Cargo.toml --workspace --all-targets --locked
```

selected host `x86_64-pc-windows-msvc` and exited 1 during dependency build:

```text
error: linker `link.exe` not found
  = note: program not found
note: the msvc targets depend on the msvc linker but `link.exe` was not found
```

The machine does not have Visual C++ Build Tools/Windows SDK. GNU-linked local
results do not replace Windows MSVC qualification. The committed workflow's
separately observable `retrieval-rust-windows-msvc` job must pass with
`GKOS_REQUIRE_ALIAS_FIXTURE=1`; that environment variable makes the Windows
symlink/junction alias fixture mandatory rather than silently skipped.

## Remaining qualification blockers

1. Full must complete the final reciprocal read-only review of the exact Lite
   commit-pin, wrapper, compatibility migration, and evidence delta.
2. The Phase 1 pull request must run and pass the hosted Ubuntu MSRV/current
   Rust jobs and the Windows MSVC retrieval job, along with the Node 22/23/24
   root matrix and existing desktop/desktop-native jobs. Local GNU results and
   the expected local missing-linker failure are not substitutes for those
   hosted gates.

No terminal Phase 1 state is assigned in this draft. The executor must update
this evidence with the exact Full and Lite final commits, wrapper parity result,
and hosted job/run identifiers before applying the executor-state protocol.
