# GKOS-Engine-Lite Phase 5 watcher-recovery Slice A evidence

Qualification date: 2026-08-23

Repository: Odenknight/GKOS-Engine-Lite

State: **DONE** for the exact qualified implementation commit below. Full
approved the exact 21-path local implementation freeze with no blocker, HIGH,
or MEDIUM finding before publication. The implementation was committed with a
valid ED25519 signature and DCO signoff, pushed only to the Phase 5 branch, and
qualified by the draft pull-request workflow. At this closeout boundary, the
evidence-only delta is exactly this one unstaged, uncommitted, and unpushed
file. Its own commit and hosted rerun remain **UNASSIGNED** pending a separate
reciprocal review.

No merge, tag, release, deployment, package publication, service activation,
or artifact publication is authorized or claimed. Phase 5 host watcher
execution remains outside this Slice A.

## Exact coordinates

| Coordinate | Value |
| --- | --- |
| Qualified Lite Phase 4 base | `d1c0d5d60e5380d4c1cb9fb1562585852307e657` |
| Working branch | `codex/phase-5-watcher-recovery` |
| Lite qualified implementation | `0bce4db2ed4dfd7b6ae825cb624637470a9c7ed4` (direct child of the qualified Phase 4 base; valid local and GitHub ED25519 signature; DCO signoff matches the author) |
| Lite evidence-only closeout | **UNASSIGNED** at this reciprocal-review freeze |
| Lite pull request | Draft [#20](https://github.com/Odenknight/GKOS-Engine-Lite/pull/20), open, CLEAN, mergeable, and unmerged against `codex/phase-4-retrieval-evaluation` |
| Lite head equality | Local branch, upstream, origin branch, and PR head all exactly `0bce4db2ed4dfd7b6ae825cb624637470a9c7ed4` before this evidence-only file |
| Hosted qualification | PASS; [PR run 32651029941](https://github.com/Odenknight/GKOS-Engine-Lite/actions/runs/32651029941), all 8 jobs terminal success |
| Hosted tested merge | `439e4bfbb6cec74ba2a6abe05bbc19d8f1fa4f4c`, exact synthetic merge with parents `d1c0d5d...` and `0bce4db...` and tree `fb4c070ca92644b7c67e6ee26ba15df876ae28fc` |
| Full reciprocal review | PASS; Full approved the corrected exact 21-path freeze with no blocker, HIGH, or MEDIUM finding |
| Full qualified Slice A implementation | `420a9d704f1fd12a6a61e4dd60abeb70757a9b2d` (signed, DCO, published, and hosted green) |
| Existing runtime Full dependency | `a57b98c00c1913f5b7ed96839b3f8effe5be9c4a` (qualified Full Phase 4 Slice B; package and lock bytes intentionally unchanged in this Slice A) |
| Full package | `gkos-engine` 2.1.2 |
| Watcher-recovery contract | `gkos-watcher-recovery/1.0.0-draft.1` |
| Full watcher pack | 18 leaves; manifest governs the other 17; 5,860,943 governed bytes; digest `sha256:c08520c1392d6be04c71159050c0d60f5bf03afeeb915ae44920e758e35cb49a` |
| Pack manifest | raw SHA-256 `d3c6808bb33049a2cfc13e71b7de03fdfd477f296de938103503d14ee70ef72a` |
| Frozen SamplePlan | 3,978 bytes, no terminal LF; raw/self digest `sha256:6ab764aad47cbb072469f19760b772df90b2138acaf6a9f022041d38094bb695` |
| Local Node runtimes | 22.23.2, 23.11.1, 24.18.0 |
| Rust latest / MSRV | 1.98.0 / 1.85.0 |

The adjacent Lite-only `FULL-PIN.json` binds the exact Full repository,
implementation commit, package version, contract version, publication state,
18-name file map, manifest coordinates, aggregate bytes, and every copied file
SHA-256. It is not itself a member of the Full 18-leaf pack.

## Exact implementation scope and source coordinates

Commit `0bce4db...` changes exactly 21 paths relative to `d1c0d5d...`: one
modified private Rust crate root, one new crate-private Rust verifier, the exact
18 copied Full pack leaves, and one adjacent Lite pin. The commit has 116,984
insertions and no deletions.

| Source | Exact coordinate |
| --- | --- |
| `rust/crates/gkos-retrieval/src/lib.rs` | 3,294 bytes; SHA-256 `69b05fb005f61dc5aa23c4183573005db79272fb658362a175585a99ea90008c` |
| `rust/crates/gkos-retrieval/src/watcher.rs` | 275,573 bytes; 6,434 LF; SHA-256 `6b90853f79c36e81e4dc5746bbdc6cde9ee517cb8dea8a5f7bda2c60aa75f7ea` |
| `rust/contracts/gkos-watcher-recovery-1.0.0-draft.1/FULL-PIN.json` | 2,951 bytes; SHA-256 `b31ae9f24c3694e8ee438eab64548f1153d189424696f55a8a710fef2ca42ea0` |

All 21 implementation paths are strict UTF-8 with no BOM, CR, or NUL. The
pack leaves preserve the Full terminal-LF rules exactly: every leaf has one
terminal LF except the frozen SamplePlan, which has none. The copied Full
pack has zero filename, size, raw-SHA, aggregate-byte, manifest, or pack-digest
mismatch.

## Authority and public-surface boundary

- The watcher verifier is crate-private. It introduces no public Rust item,
  package export, executable, host route, state directory, or watcher service.
- It consumes inert Full-produced records and fixtures and independently seals
  exact keys, versions, canonical digests, transition/DAG relations, pointer
  recovery, journal/outbox relations, status/CLI contracts, measurement
  relations, path grammar, graph material, and operation-result domains.
- It owns no raw-vault scan, source mutation, process, filesystem, network,
  SQLite, provider, service-lifecycle, pointer-publication, removal-adapter, or
  ledger authority. It cannot activate a watcher or publish host state.
- The private Graphiti verifier matches the pinned Full production projection,
  including governed `gkx` metadata, authored UUIDs, content hashes, lineage,
  rich fields/nulls, effective relationship triples and ordering, and the
  Phase 3/JavaScript timestamp acceptance and normalization boundary.
- The direct Full/Lite parity corpus includes authored-UUID, offset-time,
  content-hash, rich four-episode/three-effective-triple, `24:00`, and February
  overflow timestamp cases. The latter two exact Graphiti digests are
  `sha256:8af2a73d49741d5c61e27a250258e9e848d53fed7e0e75c1114375f23c75eb0e`
  and
  `sha256:e0104a94bc958c1bd43ff5bff430e6c0e0b51294e4205606fd7118800b371943`.
  The overflow case is also cascaded through graph, transitions 4-6, coherent
  Manifest, Pointer, Intent, Outcome, Active, and outer Guard sealing.
- Phase 0-4 contracts, fixtures, prior evidence, workflows, desktop sources,
  package metadata, binaries, and existing public exports have zero
  implementation diff from `d1c0d5d...`.

## Executable conformance coverage

- The pin and pack gates bind all 18 copied leaves, the live manifest's exact
  17 governed rows, 5,860,943 bytes, the pack digest, repository/package/state
  coordinates, and the exact all-and-only filename map.
- Recursive schema inspection enforces absolute owned references, unique
  definition ownership, and complete-object closure. All 85 schema cases run
  through the independent Rust validation path, including positive cross-owner
  composition and closed-object negatives.
- All 360 semantic cases execute through their exact operation/arity/result
  domains. The recovery fixture partitions all 360 exactly once; the final
  generated catalogs include 117 pointer cases and 47 transition cases.
- Canonical compact and pretty bytes use recursive UTF-16 code-unit key order,
  including astral-versus-BMP parity. Timestamp sealing includes year 0000 and
  the separately pinned Graphiti reference-time behavior.
- Pointer recipes bind the external guard argument through the approved arity-2
  operation, cover retained non-genesis recovery and outer-only Genesis F4
  v2.2, and execute the complete governed Decision relation.
- The focused private test also proves pack/pin integrity, schema and recovery
  catalog consumption, rich Graphiti parity, and absence of public watcher
  symbols or production filesystem/network/process/SQLite authority.

## Local qualification

| Gate | Exact result |
| --- | --- |
| Focused private watcher verifier | PASS; 8 passed, 0 failed, 0 skipped; includes all 360 semantic cases and all 85 schema cases |
| Rust 1.98.0 Windows GNU workspace all-targets | PASS; 174 library + 11 Full-conformance tests, 0 failed; bundled FTS5 green |
| Rust 1.98.0 docs / fmt / check / clippy | PASS; 4 compile-fail docs; formatting clean; check clean; clippy `-D warnings` clean |
| Rust 1.85.0 Windows GNU MSRV workspace all-targets | PASS; 174 library + 11 Full-conformance tests, 0 failed |
| Rust 1.85.0 docs / check / clippy | PASS; 4 compile-fail docs; check clean; clippy used `-D warnings` with the exact reviewed MSRV compatibility allowances `-A clippy::nonminimal-bool`, `-A clippy::overly-complex-bool-expr`, and `-A clippy::bool-comparison` |
| Node 22.23.2 root suite | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo |
| Node 23.11.1 root suite | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo |
| Node 24.18.0 root suite | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo |
| Desktop | PASS; typecheck and production build clean; 20 passed, 0 failed, 0 skipped |
| Root metadata and dependency lock | PASS; unchanged Full 2.1.2 runtime dependency resolves exact `a57b98c...`; the separate private Slice A pin binds `420a9d7...` |
| Root dry package | PASS; 5 files; 9,821 packed bytes; 25,870 unpacked bytes; SHA-1 `f854a50e8122121f6815df14ac9683b2c74d4912`; SHA-512 `X009ggJ0RqOPXrGOSmPQUVT9PzyKjMwsxCK1cfm7/XJnsMaegLonMdB/2238Sia+6LEoOVYIr2MqoqUVzmU3Uw==` |
| Protected-byte and public closure | PASS; only the exact private 21-path implementation scope differs from the Phase 4 base |
| Diff, staging, archive, residue | PASS before publication; `git diff --check` clean, staging empty, no `.tgz`, and no task-owned residue |

The local machine qualifies the GNU latest and MSRV toolchains. Hosted Windows
MSVC supplies the mandatory native Windows build, alias, all-target, and
documentation qualification.

## Hosted qualification

The workflow's push trigger is limited to `main`, so publishing the Phase 5
branch created no push-event run. Draft PR #20 created the sole run associated
with implementation head `0bce4db...`. GitHub tested synthetic merge
`439e4bf...`, whose parents are the exact qualified base and implementation
head. Run `32651029941` completed successfully from 16:13:59Z through
16:18:23Z; all eight jobs reached terminal success:

| Hosted job | Exact result |
| --- | --- |
| `retrieval-rust-windows-msvc` / `97222547275` | PASS; Windows check; 174 library + 11 Full-conformance + 4 docs; `GKOS_REQUIRE_ALIAS_FIXTURE=1`; zero failures |
| `desktop-native` / `97222547365` | PASS; digest-pinned compile assets and native Tauri build/test on `windows-latest` |
| `desktop` / `97222547383` | PASS; typecheck, production build, and 20/20 frontend tests |
| `retrieval-rust-latest` / `97222547396` | PASS; 178 Linux library + 11 Full-conformance + 4 docs; formatting and clippy `-D warnings` green |
| `test (22)` / `97222547455` | PASS; 43/43 root tests and five-file package smoke green |
| `test (23)` / `97222547491` | PASS; root suite and five-file package smoke green |
| `test (24)` / `97222547591` | PASS; root suite and five-file package smoke green |
| `retrieval-rust-msrv` / `97222547605` | PASS; Rust 1.85.0; 178 Linux library + 11 Full-conformance + 4 docs; zero failures |

The only run annotations are GitHub's non-failing Node 20 action-runtime
deprecation notices; the pinned actions were automatically run on Node 24.
The current CI workflow has no artifact-upload step. The GitHub run artifact
API independently reports `total_count: 0`, so no artifact, receipt, archive,
database, corpus, or provider material exists to audit or is claimed here.

GitHub reports the implementation commit signature as verified. Local Git
verifies the same ED25519 key, and the DCO signoff matches the author. There is
no local or remote tag at the implementation commit. The draft PR remains open,
unmerged, CLEAN, and mergeable at the exact qualified base and head.

## Closeout boundary

The implementation publication gates are complete: reciprocal approval, a
signed+DCO implementation commit, exact local/upstream/origin/PR-head equality,
and all eight hosted jobs green. The qualified implementation remains
`0bce4db2ed4dfd7b6ae825cb624637470a9c7ed4` even if a later evidence-only
commit advances the draft PR head.

This DONE closeout is currently exactly one unstaged, uncommitted, and unpushed
evidence file. It must receive a separate reciprocal read-only approval before
any evidence commit. Its own commit and any resulting hosted rerun are honestly
**UNASSIGNED** at this freeze.

No merge, tag, release, deployment, package publication, service activation,
or artifact publication is authorized or claimed.
