# GKOS-Engine-Lite Phase 5 watcher-recovery repin evidence

Qualification date: 2026-08-25

Repository: Odenknight/GKOS-Engine-Lite

State: **FULL PUBLICATION QUALIFIED; LITE PHASE 5 REPIN QUALIFIED**. Signed ED25519+DCO Lite implementation commit `6d94e40dc11e1bb43693b225e32ec6110d4e03b1` copies and independently verifies the final Full Phase 5 watcher pack from signed Full commit `7b5262baee9fcda23d50b0cee0c4977d6e4305e7`. Lite hosted run `32807434279` completed `SUCCESS` with all 8 jobs green and an exact-zero artifact audit. Full hosted runs `32803396417` (push) and `32803399153` (pull request) completed successfully, including the complete Linux watcher, Windows watcher, Windows path-security Node 22/23/24 matrices and terminal artifact audits. PR #20 remains draft.

No merge, tag, release, deployment, package publication, service activation, or artifact publication is authorized or claimed.

## Exact authority coordinates

| Coordinate | Value |
| --- | --- |
| Lite qualified Phase 4 base | `d1c0d5d60e5380d4c1cb9fb1562585852307e657` |
| Lite prior Phase 5 implementation | `0bce4db2ed4dfd7b6ae825cb624637470a9c7ed4` |
| Lite prior evidence head / repin base | `9dafbec38d95d9a4daad7901a023e681a8778b31` |
| Working branch | `codex/phase-5-watcher-recovery` |
| Lite repin implementation commit | `6d94e40dc11e1bb43693b225e32ec6110d4e03b1`; valid ED25519 signature and matching DCO signoff |
| Lite hosted qualification | `32807434279`; **SUCCESS**; 8/8 jobs green; exact-zero artifacts |
| Lite pull request | #20; draft; no merge claimed or authorized |
| Full final Phase 5 head | `7b5262baee9fcda23d50b0cee0c4977d6e4305e7`; valid ED25519 signature and matching DCO signoff |
| Full hosted push run | `32803396417`; **SUCCESS**; complete Linux watcher, Windows watcher, Windows path-security Node 22/23/24 matrices and terminal artifact audit green |
| Full hosted PR run | `32803399153`; **SUCCESS**; complete Linux watcher, Windows watcher, Windows path-security Node 22/23/24 matrices and terminal artifact audit green |
| Full package | `gkos-engine` 2.1.2 |
| Watcher contract | `gkos-watcher-recovery/1.0.0-draft.1` |
| Full watcher pack | 18 leaves; manifest governs 17; 8,907,164 governed bytes; `sha256:a8e0eed2a829db8c80cede489c871f938e432a09e6f1c34aa0940fcfe381519f` |
| Pack manifest | 3,250 bytes; raw SHA-256 `d794236b4e7618d6eaa37024b4bd8660bac625758043612d039e15ef6e9ab023` |
| Frozen SamplePlan | draft.2; 4,363 bytes; no terminal LF; raw/self digest `sha256:75b011dc253a445ec9c5fc192f600f57ec62411e8125dfa20c74a08f5faf301b` |
| Existing ordinary runtime dependency | Full Phase 4 `a57b98c00c1913f5b7ed96839b3f8effe5be9c4a`; deliberately unchanged |

The adjacent Lite-only `FULL-PIN.json` binds the Full repository, exact signed commit, package and contract versions, publication-qualified Full state, complete 18-name file map, every raw file digest, manifest coordinates, and aggregate governed bytes. It is not one of the Full pack's 18 leaves.

## Qualified repin scope

The repin copies all 18 Full leaves byte-for-byte. Eleven pack leaves differ from the prior Lite pin; the other seven were already identical. Outside those copied bytes, implementation commit `6d94e40dc11e1bb43693b225e32ec6110d4e03b1` is limited to:

- `rust/contracts/gkos-watcher-recovery-1.0.0-draft.1/FULL-PIN.json`;
- the crate-private verifier at `rust/crates/gkos-retrieval/src/watcher.rs`;
- this evidence file.

The verifier remains private and inert: it exposes no public Rust item, binary, route, watcher service, state authority, filesystem writer, network client, SQLite mutation, provider invocation, or package export. It independently checks the final exact pin and pack, 99 schema cases, and all 401 semantic cases. The final corpus includes PlannedTarget/Witness draft.2 bootstrap relations, reset-recovery Plan and handoff records, reset adoption, failure retry no-op authority, multisource `(source_path, ordinal)` identity, and SamplePlan draft.2 production coordinates. The pinned retry/adoption bodies contain Full's exact compact owner projection rather than the complete public Phase 3 owner envelope; Lite seals that projection's exact keys, zero-count state, rejection filename/digest, owner self-digest and complete schema-3 retrieval-manifest coordinates, but does not describe it as independently reconstructing omitted Phase 3 profile, validation-summary, observation, or rejection bodies. The sealers additionally bind scan and Plan artifact coordinates, mutation-set preimages, native/adopted activation DAGs, complete-transition retrieval/graph states, and all receipt/transition coordinates; fully resealed owner-projection, Plan, manifest, pointer, intent, outcome, active, and receipt cascades are direct negatives.

Phase 0-4 governed bytes, public/root/navigation surfaces, package metadata, and the separately qualified ordinary Full Phase 4 runtime dependency remain unchanged. The private Phase 5 pin does not replace or broaden that runtime dependency.

## Implementation qualification

The following table records the implementation gates for the exact bytes in signed commit `6d94e40dc11e1bb43693b225e32ec6110d4e03b1` and its terminal hosted run `32807434279`.

| Gate | Result |
| --- | --- |
| Focused private watcher verifier | PASS; 9 passed, 0 failed; all 401 semantic and 99 schema cases consumed, plus direct self-resealed authority-splice negatives |
| Full pack/pin integrity | PASS; exact 18/17/8,907,164 and pack digest above; all raw hashes and filenames match |
| Rust formatting | PASS; `cargo fmt --all -- --check` clean |
| Rust 1.98 Windows GNU workspace tests/check/clippy | PASS; 175 library + 11 Full-conformance tests, 0 failed; check clean; strict Clippy `-D warnings` clean |
| Rust 1.85 Windows GNU MSRV tests/check/clippy | PASS; 175 library + 11 Full-conformance tests, 0 failed; check clean; Clippy uses only the three preapproved compatibility allowances |
| Native Windows MSVC local attempt | NOT A CLAIMED LOCAL GATE; unavailable because this shell has no Visual C++ `link.exe`; terminal hosted run `32807434279` supplies the required hosted platform qualification |
| Implementation scope and residue | PASS; exact 14-path implementation scope, all 18 copied leaves byte-equal Full, signed ED25519+DCO commit pushed to the existing Phase 5 branch, and hosted exact-zero artifact audit |
| Lite hosted CI | PASS; run `32807434279` terminal `SUCCESS`, 8/8 jobs green, exact-zero artifacts |

For MSRV, the only reviewed Clippy allowances are `clippy::nonminimal-bool`, `clippy::overly-complex-bool-expr`, and `clippy::bool-comparison`; latest Rust remains strict `-D warnings`.

## Hosted and publication boundary

Full runs `32803396417` and `32803399153` are terminal `SUCCESS`. Their complete build, Linux watcher, Windows watcher, and Windows path-security Node 22/23/24 matrices passed, together with the terminal artifact audits. Lite implementation commit `6d94e40dc11e1bb43693b225e32ec6110d4e03b1` is signed+DCO, pushed, and qualified by terminal hosted run `32807434279`: all 8 jobs passed and the artifact audit found exactly zero artifacts.

This document update is an evidence-only closure follow-up. Its containing commit and hosted run cannot be embedded here without self-reference and will be recorded externally after that follow-up is published. It does not alter or requalify the implementation coordinate above. PR #20 remains draft.

No merge, tag, release, deployment, package publication, service activation, or artifact publication is authorized or claimed.
