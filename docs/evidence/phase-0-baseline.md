# Phase 0 baseline and reconciliation

Date: 2026-08-20

Repository: `Odenknight/GKOS-Engine-Lite`

Phase state: **DONE**

Phase 0 reconnaissance, ADRs, JavaScript baselines, compatibility fixtures, provider/Standard decisions, and the exact locked native test are complete. The owner's explicit requirement that GKOS-Engine-Lite build as one statically linked binary ratifies ADR-0005's separately governed Rust/static frontend-adapter with exact cross-language conformance. The local workstation still lacks `link.exe` and the Windows SDK, so its untouched native attempt remains recorded as blocked; the same committed tree passed `cargo test --locked` on the authorized GitHub-hosted Windows MSVC runner. No required test was skipped or replaced by a mock.

## Exact coordinates

| Subject | Exact coordinate | Disposition |
|---|---|---|
| GKOS-Engine-Lite implementation base | `2ebbf77583af3e83032054f1256188dc56376907` | Branch `codex/phase-0-recon-adrs`, initially clean |
| Phase 0 implementation commit | `8e4de16ae52c5825c2b6e02d6e4aaad1920d5f3e` | DCO-signed commit under draft pull request 15 |
| Installed Full dependency | tag `v1.1.3` → `72c4a3268c9db132f2f9dd5aaa7eb7075e6bab2a` | Exact immutable SHA in `package-lock.json` |
| Inspected current GKOS-Engine | `2fbd4ec68ec825b09e5194c9878a7ae90a281392`, package 2.1.2 | Read-only sibling |
| Pinned GKOS Standard | `a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6`, `v0.79-5-ga2a2a6c` | Current release line v0.79; exact post-release study commit pinned |
| Studied GrooveSeek | `313514b793d12ea5c3b8eedc32fd213212e38d75`, `v0.27.0-15-g313514b` | Exact study commit; not “main” |
| Inspected Kosmos-Oden | `a7113c0ca3be8dd230a9549940e2f387d4cb2a96` | Read-only identity-format study |

## Environment

| Item | Value |
|---|---|
| Node | `v24.18.0` |
| npm | `11.16.0` |
| Platform | `Windows_NT 10.0.26200`, `win32 x64` |
| CPU | 11th Gen Intel Core i7-11800H |
| Rust | `rustc 1.98.0 (88d9e12ae 2026-08-18)`, target `x86_64-pc-windows-msvc` |
| Cargo | `cargo 1.98.0 (797e8a9bc 2026-08-05)` |
| SQLite via Node | 3.53.1, `ENABLE_FTS5`; in-memory FTS5 create/insert/query passed |

This modern CPU is not evidence for Sandy Bridge/Ivy Bridge or R720 AVX-only compatibility.

## Untouched baseline results

Root commands were run before source changes:

| Command | Exact result |
|---|---|
| `npm ci` | PASS; audit reported 0 vulnerabilities |
| `npm test` | PASS; 18 tests, 0 failures, 0 skipped |
| `npm run check:metadata` | PASS; Lite 1.1.3, Engine 1.1.3, Apache-2.0 |
| `node scripts/check-lockfile-sha.mjs` | PASS; one immutable git dependency SHA |
| `npm pack --dry-run --json` | PASS; 5 files, 7,827-byte tarball, 19,643 bytes unpacked |

The requested Full scripts `typecheck`, `test:navigation`, `test:intelligence`, `check:nomenclature`, `check:license`, and `pack:check` do not exist at the Lite root and are recorded as not applicable, not passed. The actual Lite gates above were used.

Desktop commands were also run before source changes:

| Command | Exact result |
|---|---|
| `npm ci` | PASS; npm audit summary reported 2 vulnerabilities |
| `npm run typecheck` | PASS |
| `npm test` | PASS; 18 tests, 0 failures, 0 skipped |
| `npm run build` | PASS; Vite 8.1.5 built 15 modules |
| `npm audit --json` | FAIL; 1 high (`nanoid`) and 1 moderate (`postcss`); not an existing CI gate, retained as debt |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml` | **BLOCKED**; Cargo resolved 524 packages, then Rust could not find MSVC `link.exe`; Visual C++ Build Tools and Windows SDK are absent |

The Rust toolchain was installed in isolation beneath the uplift workspace and added only to the command-local `PATH`; it did not alter the global `PATH` or install system-wide components. The untouched baseline had no `Cargo.lock`. Phase 0 retains the generated `desktop/src-tauri/Cargo.lock` as a reproducibility addition and makes the new hosted native job use `cargo test --locked`. No external service was provisioned.

The latest root CI run observed for the baseline was GitHub Actions run `30778637504` on 2026-08-03 and passed. The latest `desktop-build` run was `30215113831` on 2026-07-26 at older commit `5554894339...`; no native desktop-build run was found for the exact Phase 0 baseline.

Post-change verification:

| Command | Exact result |
|---|---|
| `npm ci` | PASS; 1 package installed, 0 vulnerabilities |
| `npm test` | PASS; 24 tests, 0 failures, 0 skipped, including 6 Phase 0 compatibility tests |
| `npm run check:metadata` | PASS |
| `node scripts/check-lockfile-sha.mjs` | PASS; one immutable git dependency SHA |
| `npm pack --dry-run --json` | PASS; unchanged 5-file package payload and checksums |
| `desktop: npm ci` | PASS; 23 packages audited; retained 1 high and 1 moderate baseline vulnerability |
| `desktop: npm run typecheck` | PASS |
| `desktop: npm test` | PASS; 20 tests, 0 failures, 0 skipped, including 2 Phase 0 compatibility tests |
| `desktop: npm run build` | PASS; Vite built 15 modules |
| `cargo metadata --manifest-path desktop/src-tauri/Cargo.toml --locked --no-deps --format-version 1` | PASS with the isolated Rust/Cargo toolchain; validates the retained lockfile without replacing the blocked native test |
| GitHub Actions `desktop-native`, run `32439308236`, job `96646635932` | PASS in 4m26s on `windows-latest`; digest-pinned compile assets and Rust 1.98.0, followed by `cargo test --locked` |
| ADR headings, forbidden provider-reference scan, trailing-whitespace scan, and `git diff --check` | PASS |
| `git check-attr eol` for compatibility code/goldens, CI YAML, and `Cargo.lock` | PASS; all resolve to `eol: lf` |

Tracked production code, package manifests, and existing source-note fixtures remained byte-identical. Phase 0 adds a Cargo lockfile, ADR/evidence and compatibility files, and modifies the CI workflow. The DCO-signed focused branch was pushed and draft pull request 15 was opened to run the required CI. No merge, tag, release, deployment, or publication occurred.

## Lite boundary inventory

- Root package: `gkos-engine-lite` 1.1.3, ESM, Node `>=22 <25`, npm `>=10`, Apache-2.0.
- Published executable: `okf-lite`; package contents are limited to `bin/`, `package.json`, `README.md`, and `LICENSE`. Current dry-run packaging omits a third-party notice and packaged stability document.
- Installed Full: `gkos-engine` 1.1.3 at exact commit `72c4a3268c9db132f2f9dd5aaa7eb7075e6bab2a`, MIT.
- Delegated allowlist: `validate`, `assess`, `graph`, and `export graphiti`. The wrapper implementation forwards every allowed invocation through the installed Full CLI entry point. The tests execution-compare Lite versus Full for `validate` and `assess --json`; `graph` and `export graphiti` are boundary-locked as allowed but are not claimed as execution-compared in Phase 0 because their file outputs contain runtime fields. Their deterministic Engine graph and Graphiti projections are instead covered separately by fixed-time, host-timing-normalized byte goldens.
- Local proposal-only command: `assist`; its optional loopback response is validated before display and does not write source notes.
- Other Full/future commands are rejected by the Lite boundary.
- Full current main is package 2.1.2 and exposes additional package boundaries unavailable through the old Lite pin. Lite must not claim current Full parity until a reviewed release coordinate passes the new fixture.
- Existing root history and docs contain published `okf-lite`, `OKF+`, `okf_`, and Kosmos identifiers. They are recorded compatibility debt, not new nomenclature. Phase 0 retains the executable; removal needs separate owner authorization.

The Phase 0 fixture captures package metadata, all installed Full public exports, the exact delegated/local-only/blocked wrapper boundary, CLI help and exit behavior, deterministic graph bytes, and Graphiti episode bytes. The `local_proposal_only_argv` fixture is exercised and proven ineligible for Engine delegation. The pinned 1.1.3 dependency does not export `getNavigationCapabilities()`; the fixture records that absence explicitly rather than inventing an output. Host timing fields are omitted only where already nondeterministic. Deterministic artifacts and CLI help are read as raw bytes. Narrow `.gitattributes` rules force LF for compatibility code/goldens, CI YAML, and the Cargo lockfile; a simulated CRLF golden is proven unequal and rejected. Tests also deliberately perturb the executable map and prove the comparison fails.

## Desktop, workflow, and installer inventory

- `desktop/` is a Tauri 2 tray/presentation shell; it does not implement GKX semantics.
- Product/version: GKOS Engine Desktop 0.2.0, identifier `org.gkos.engine.desktop`.
- Bundles: NSIS and DMG; external binary `binaries/kosmos-agent`; resource `resources/vault-kosmos.html`.
- Persisted settings: `notes_dir`, `default_sensitivity`, `port`, `enabled`, `wizard_completed`.
- Sidecar status fields: `pid`, `port`, `url`, `token_path`, `notes_dir`, `default_sensitivity`, `notes_indexed`, `state`, `last_scan_iso`.
- Existing service boundary is `127.0.0.1`, default port 4814, bearer protected. The shell constructs `/health` and `/mcp` client snippets. The separately bundled viewer consumes the existing read-only note/graph routes.
- Baseline security debt: current client snippets and viewer URL construction embed the bearer token in generated text/query state. This fixture records existing behavior; later authenticated UI work must remove credential exposure rather than treating it as acceptable precedent.
- CI root matrix runs on Ubuntu with Node 22 and exercises root plus desktop frontend/type/build, but not Cargo.
- Phase 0 adds a 30-minute-bounded `desktop-native` job on `windows-latest`. It grants the job only `contents: read`, builds the frontend in the job-local workspace, pins checkout, Node setup, Rust toolchain, and Rust-cache actions to reviewed immutable revisions, installs Rust 1.98.0, downloads the existing Tauri compile-time sidecar/viewer assets, verifies their published SHA-256 digests (`29ab43c...f12c2` and `11e004a5...68dd`), and runs `cargo test --locked`. The existing JavaScript jobs were moved from mutable major action tags to the same reviewed checkout and Node-setup revisions. Run `32439308236`, job `96646635932`, passed this exact Phase 0 commit in 4m26s.
- Native `desktop-build` targets macOS arm64, macOS x86_64, and Windows x86_64. It has no Linux x86_64/aarch64 legs.
- Published prerelease `desktop-v0.2.0` has unsigned arm64/x64 DMGs and a Windows x64 installer. It predates the exact baseline, has no Linux artifacts, no required top-level `sha256.sum`, and no R720 result. It is stale/unqualified for the uplift matrix.
- The desktop compatibility fixture captures product/bundle fields, status/settings shapes, loopback defaults, generated MCP snippets, and viewer URL behavior. A deliberate `0.0.0.0` perturbation is proven to fail.

## Standard reconciliation

The exact authority is `Odenknight/gkos-standard@a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6`. Current publication is GKOS v0.79 with the GKX 2.0 machine exchange contract. `archive/illustrated/GKOS-v0.76-Illustrated-Edition.md` is historical material, not an active schema coordinate.

The pinned schema `schemas/gkx-frontmatter-2.0.schema.json` requires `gkx_version`, `uid`, `title`, `type`, `created_at`, and `epistemic_state`; it also defines optional `updated_at`, `sensitivity`, and `authorship_origin`, with extension fields permitted. The Standard assessment profile coordinate is `gkx-2.0-validating-projection`; current Full separately reports its implemented `gkx-2.3-validating-projection` coordinate.

Resolved mappings:

- Retrieval `source_id` maps to a valid canonical `uid`. `gkx_id` is not a GKX 2.0 alias.
- Canonical authored creation is `created_at`. `created` is not an alias.
- `supersedes` and `superseded_by` are authored declarations accepted according to the current parser/profile, including profile-permitted stable target identifiers and quoted wikilinks. Full resolves and normalizes them into one canonical newer-to-older lineage edge (and its inverse view); retrieval never infers that edge from timestamps.
- `valid_from` and `valid_to` are retrieval-envelope names only. They map to Full's canonical `validAt` and `invalidAt` projections and are never accepted as frontmatter aliases.
- Full's `projectAtTime` defines the half-open interval `validAt <= T && (invalidAt == null || invalidAt > T)`, or `[validAt, invalidAt)`.
- Retrieval `lineage_id` remains null unless a canonical profile/projection supplies one. It is never guessed from path, component, timestamp, or rank.

## Provider, identity, graph, and commit decisions

- Provider interfaces are neutral and selected only by trusted operator configuration: `openai_compatible`, `local_onnx`, and upstream `mcp` embedding/rerank adapters. There is no vendor/domain/model/routing allowlist or provider preference.
- Untrusted vault content cannot redirect endpoints, credentials, model paths, MCP tools, bind/authentication, or authorization policy.
- Provider failure never causes a silent embedding-space switch: embeddings degrade to reported FTS-only; absent optional reranking is reported and skipped.
- No verified Postgres retrieval schema, Qdrant collection, external Graphiti endpoint, or durable production ledger binding was established. Local derived adapters are selected where applicable; nothing was provisioned.
- Kosmos-Oden's current client label is explicitly best-effort and unauthenticated, so it is metadata only. Credential-bound local GKOS agent identity remains primary.
- No formal document named “Semantic Commit Protocol” was found in Lite, Full, Standard, or Kosmos-Oden. Standard `CONTRIBUTING.md` mandates DCO sign-off. Lite history mixes prose with conventional prefixes. Until an owner supplies a formal protocol, the conservative merge convention is `<type>: <imperative summary>` with DCO sign-off; this is an observed fallback, not a claimed formal policy.

## License study

GrooveSeek was studied at exact commit `313514b793d12ea5c3b8eedc32fd213212e38d75`. The six required retrieval/evaluation/citation/MCP/behavior/stability documents, English ADR-0000 through ADR-0010, and both license files were read. GrooveSeek is MIT OR Apache-2.0, copyright 2026 koshian.

Phase 0 uses a clean-room implementation plan and copies no upstream code or unusually expressive prose. Therefore no GrooveSeek attribution entry is required yet. Any later copying must add exact source-file/commit/destination/license evidence to `THIRD-PARTY-NOTICES.md`.

## Gate disposition

| Phase 0 criterion | State |
|---|---|
| ADR-0000 through ADR-0005 accepted | DONE |
| Exact baselines and GrooveSeek study commit recorded | DONE |
| Current Standard fields/profile pinned and ambiguous draft names reconciled | DONE |
| External contracts verified or explicitly unavailable | DONE |
| Compatibility fixtures detect deliberate breakage | DONE |
| Existing root and desktop frontend suites reproducible | DONE |
| Native Tauri/Cargo test baseline reproducible | DONE; exact locked tree passed hosted Windows MSVC job `96646635932` |

Final Phase 0 state: **DONE**. The packaging architecture is ratified, every Phase 0 compatibility and JavaScript gate passed, and the retained lockfile plus exact hosted Windows MSVC job make the native Tauri baseline reproducible. The local missing-linker failure remains honest workstation-environment evidence; it does not replace or weaken the passing hosted test. Platform packaging, true-static closure, Linux targets, signing, and R720/AVX qualification remain later-phase obligations rather than Phase 0 claims.
