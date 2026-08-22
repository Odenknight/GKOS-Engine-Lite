# GKOS-Engine-Lite Phase 4 retrieval-evaluation Slice B evidence

Qualification date: 2026-08-22

Repository: Odenknight/GKOS-Engine-Lite

State: **DONE**. The bounded Slice B implementation was reciprocally approved,
committed with a valid ED25519 signature and DCO signoff, pushed only to the
Phase 4 branch, and qualified by the exact-head draft pull-request workflow.
This evidence file was included in that implementation commit in its historical
local-freeze form. The sole current unstaged change converts it to this DONE
closeout and records the assigned publication and hosted coordinates. The
evidence-only closeout commit and its fresh hosted run remain **UNASSIGNED**
pending a separate reciprocal review. No merge, tag, release, deployment,
package publication, or artifact publication is authorized or claimed.

## Exact coordinates

| Coordinate | Value |
| --- | --- |
| Lite Slice A evidence base | `408701f18e7fdf8caea1555e3b271d7621a53e4e` (valid ED25519 signature and DCO; direct parent of the Slice B implementation) |
| Working branch | `codex/phase-4-retrieval-evaluation` |
| Lite Slice B implementation | `af8e3ee28bd5c4618cde694365cc8a2713ce2ca6` (direct 13-path child of the Slice A evidence base; valid local and GitHub ED25519 signature; DCO signoff matches the author) |
| Lite pull request | [Draft #19](https://github.com/Odenknight/GKOS-Engine-Lite/pull/19), open and unmerged against exact Phase 3 base `41912fd6db279f1b46e67cb4b88c1f1b4ba86e63`; local, upstream, origin, and PR head all equal the Slice B implementation |
| Hosted qualification | PR run [32601823604](https://github.com/Odenknight/GKOS-Engine-Lite/actions/runs/32601823604), exact implementation head, terminal success in all 8 jobs |
| Full qualified Slice B implementation | `a57b98c00c1913f5b7ed96839b3f8effe5be9c4a` (valid ED25519 signature and DCO; hosted push and pull-request jobs green) |
| Full package | `gkos-engine` 2.1.2 |
| Frozen evaluation-pack implementation | `cac029a5b570135b26f3585bc86f4c9beb00c36d` |
| Evaluation contract | `gkos-retrieval-evaluation/1.0.0-draft.1` |
| Frozen evaluation pack | 37 files / 4,948,463 bytes |
| Full CLI conformance fixture | 23,770 bytes; raw SHA-256 `fce5308d252d9e693244250543f6642af1cc4a7ef9404ac604313f6f37f107be`; internal digest `sha256:958c06ed5b2d063e6b9530261ed74fd17bba5e599d6326aafe5bc7f1ac6c0ff6` |
| Local Node runtimes | 22.23.2, 23.11.1, 24.18.0 |
| Rust latest / MSRV | 1.98.0 / 1.85.0 |

The package dependency, lockfile, active compatibility fixture, README, and
versioning text bind the exact Full Slice B head. The evaluation pack's
Lite-only `FULL-PIN.json` deliberately remains bound to `cac029a5...`: it
describes the immutable Slice A contract bytes, not the later host-only CLI
implementation. The active compatibility fixture records both coordinates.

## Authority and public-surface boundary

- Lite adds only the two delegated paths `retrieval eval` and `retrieval tune`.
  The original argv array and object cross the wrapper unchanged. The exact
  pinned Full parser owns nested syntax, help, path validation, status, output,
  and every finite rejection branch.
- Full remains the sole golden-TOML, GKX, source-corpus, provider, search,
  comparison, tuning, temporary-state, counter, and guarded-output authority.
  Lite does not parse, normalize, execute, select, stage, recover, or publish
  any Phase 4 host state itself.
- The copied Full CLI fixture and executable differential tests are private test
  material. The npm package still ships only `LICENSE`, `README.md`, the two
  existing `bin/` files, and `package.json`; no fixture, Rust module, host seam,
  or evaluation API is exported.
- The crate-private Slice A verifier and every Rust public API are unchanged.
  Its four downstream compile-fail documentation tests remain green.
- Phase 0-3 contracts, fixtures, prior evidence, workflows, desktop sources,
  and the exact 37-file Phase 4 pack have zero implementation diff.

## Executable wrapper parity

The copied fixture is byte-identical to Full and consumes the exact case sets:
18 argv rows, 2 help rows, 15 local-path rejections, 6 fixed error rows,
14 general-execution rows, 6 optional-companion rows, 18 recovery states,
8 guard mutations, and 6 candidate-TOML rows. The presentation and selection
objects retain their exact key sets and canonical fixture digest.

The wrapper differential proves:

- all valid, invalid, help, path, and ordinary error argv reach Full without
  Lite interpretation or mutation;
- eval text and pretty JSON equal the frozen Full bytes exactly;
- the actual exhaustive tune evaluates 900 candidates and 21,600 query
  occurrences, emits the frozen selection bytes, and publishes the exact
  candidate TOML through Full's guarded no-clobber protocol;
- fixture bytes are unchanged, only the requested candidate exists, and the
  isolated execution-temp directory is empty after completion; and
- historical validate/index/search/help behavior and Phase 3 differentials
  remain byte-identical.

## Local qualification

| Gate | Exact result |
| --- | --- |
| Focused actual wrapper replay | PASS; eval text/JSON plus exhaustive 900-candidate, 21,600-query tune; 162.301 seconds |
| Node 22.23.2 root suite | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped; exhaustive wrapper case 193.956 seconds |
| Node 23.11.1 root suite | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped; exhaustive wrapper case 192.892 seconds |
| Node 24.18.0 root suite | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped; exhaustive wrapper case 171.956 seconds |
| Phase 4 focused Rust verifier | PASS; 11 passed, 0 failed, 0 skipped |
| Rust 1.98.0 Windows GNU workspace all-targets | PASS; 166 library + 11 Full-conformance = 177 passed, 0 failed, 0 skipped |
| Rust 1.98.0 docs / fmt / clippy | PASS; 4 docs passed; fmt clean; clippy `-D warnings` clean |
| Rust 1.85.0 Windows GNU MSRV workspace all-targets | PASS; 177 passed, 0 failed, 0 skipped |
| Rust 1.85.0 docs | PASS; 4 passed, 0 failed, 0 skipped |
| Desktop typecheck, tests, production build | PASS; typecheck/build clean; 20 passed, 0 failed, 0 skipped |
| Full Slice B CLI fixture | PASS; exact 23,770 bytes and zero raw-SHA mismatch |
| Frozen Slice A pack | PASS; 37 files / 4,948,463 bytes and zero filename, byte-count, or SHA-256 mismatch against Full |
| Root metadata and dependency lock | PASS; Lite/Full package 2.1.2; exact `a57b98c...` dependency and resolution; one immutable 40-hex Git dependency |
| Installed dependency | PASS; `npm ls` resolves `gkos-engine@2.1.2` at exact `a57b98c...` |
| Root dry package | PASS; 5 files / 9,821 packed / 25,870 unpacked / SHA-1 `f854a50e8122121f6815df14ac9683b2c74d4912` / SHA-512 `X009ggJ0RqOPXrGOSmPQUVT9PzyKjMwsxCK1cfm7/XJnsMaegLonMdB/2238Sia+6LEoOVYIr2MqoqUVzmU3Uw==` |
| Protected-byte and public closure | PASS; Rust/workflow/desktop/scripts/prior evidence/Phase 0-3 paths have zero diff; package remains five-file CLI-only surface |
| Diff, staging, archive, residue | PASS; `git diff --check` clean; staging empty; no `.tgz`; no task-owned execution-temp residue |

The local machine qualifies the GNU latest and MSRV toolchains. Native MSVC is
reserved for the mandatory hosted Windows job, including its all-target,
documentation, and alias fixture gates.

## Hosted qualification

The sole workflow run for the implementation head was pull-request run
`32601823604`. It completed successfully at exact head
`af8e3ee28bd5c4618cde694365cc8a2713ce2ca6`; all eight jobs were terminal green.

| Hosted job | Exact result |
| --- | --- |
| `test (22)` / `97101137908` | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo; exhaustive reviewed eval/tune differential 189,125.038823 ms; five-file package smoke green |
| `test (23)` / `97101137856` | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo; exhaustive reviewed eval/tune differential 199,016.028629 ms; five-file package smoke green |
| `test (24)` / `97101137891` | PASS; 43 passed, 0 failed, 0 cancelled, 0 skipped, 0 todo; exhaustive reviewed eval/tune differential 131,133.762151 ms; five-file package smoke green |
| `retrieval-rust-latest` / `97101137791` | PASS; 170 library + 11 Full-conformance tests, 4 docs, formatting, sealed-API check, and clippy `-D warnings` |
| `retrieval-rust-msrv` / `97101137871` | PASS; 170 library + 11 Full-conformance tests, 4 docs, and sealed-API check at Rust 1.85.0 |
| `retrieval-rust-windows-msvc` / `97101137847` | PASS; `GKOS_REQUIRE_ALIAS_FIXTURE=1`, check, 166 library + 11 Full-conformance tests, 4 docs, and sealed-API check |
| `desktop` / `97101137895` | PASS; typecheck, production build, and 20/20 frontend tests |
| `desktop-native` / `97101137842` | PASS; frontend prerequisite build and native Tauri build/test qualification |

GitHub reports the implementation commit signature as valid. The draft PR is
open, unmerged, and mergeable at the exact qualified head and base. There is no
local or remote tag at the implementation commit.

## Exact implementation and closeout scope

At local qualification time, the bounded Slice B implementation was exactly 12
unstaged paths: nine modified tracked files and three untracked test/fixture
files. This evidence made the reciprocally approved snapshot exactly 13 paths.
Commit `af8e3ee...` preserves that exact scope with 812 insertions and 28
deletions. The only new production behavior is the narrow CLI delegation and
exact Full pin update.

This DONE closeout changes only this evidence file. It is intentionally
unstaged; staging and archive counts remain zero and `git diff --check` remains
clean. Its evidence-only commit, updated branch/PR head, and fresh hosted run
remain unassigned until a separate reciprocal approval and authorization.

No merge, tag, release, deployment, package publication, or artifact
publication is authorized or claimed.
