# Changelog

## Unreleased — Phase 4 Slice B integration

- Advanced the delegated CLI dependency to signed, hosted-green GKOS-Engine
  Slice B correction `a57b98c00c1913f5b7ed96839b3f8effe5be9c4a`; no tag or
  release is claimed.
- Added byte-identical `retrieval eval` and `retrieval tune` delegation plus an
  exact private copy of Full's nested CLI conformance fixture. Full remains the
  only fixture, provider, search, tuning, temporary-state, and guarded-output
  authority.
- Preserved the exact Slice A 37-file contract mirror and crate-private Rust
  verifier without widening any Rust or package public API.

## Unreleased — Phase 4 Slice A integration

- Pinned the delegated CLI and exact 37-file evaluation contract mirror to
  signed, hosted-green GKOS-Engine Slice A commit
  `cac029a5b570135b26f3585bc86f4c9beb00c36d`; no tag or release is claimed.
- Added a crate-private pure verifier for normalized evaluation envelopes,
  u128 metric math, baseline comparison, policy-leak and citation accounting,
  temporal/scenario outcomes, and deterministic tuning priority.
- Preserved Full as the sole golden-TOML, GKX, provider, search, tuning,
  filesystem, output-publication, and CLI authority; no Phase 4 command is
  added by this slice.

## Unreleased — Phase 3 integration

- Pinned the delegated CLI and frozen 21-file ingest contract mirror to signed,
  hosted-green GKOS-Engine implementation commit
  `e7cc0dd478af3d0bda216c5258dec5f77932def7`; no tag or release is claimed.
- Added byte-exact `validate --kb-path ... [--schema ...]` and `index`
  delegation, including strict/non-strict results, safe operational failures,
  and unchanged source-note bytes.
- Added a private Rust verifier for Full-produced normalized profiles,
  findings, rejections, journals, owner/state envelopes, and digests without a
  second YAML, TOML, GKX identity, profile, or Decision-A authority.
- Added the Full-compatible durable guard around the frozen public legacy
  schema-2 writer APIs and the internal schema-3 writer, preventing provider or
  pointer work when Phase 3 owner authority exists.

## Unreleased — Phase 2 integration

- Pinned the five-command JavaScript delegate and the static Rust retrieval
  contract mirror to signed, hosted-green GKOS-Engine commit
  `6e2df27d33ede62ee0d2e3cb7610df478a7d66ce`; no tag or release is claimed.
- Added point-in-time `search --as-of` delegation with byte-equivalent
  Full/Lite exit status, stdout, stderr, and unchanged source-note bytes.
- Added Decision-A authorization-scoped lineage, temporal provenance, and
  exact citation conformance without creating a second GKX authority.

## Unreleased — Phase 1 integration

- Pinned the delegated CLI to exact reviewed GKOS-Engine commit
  `bbc2ea874f4dde37e6376e46c080cb1c69ab1bb3` and mirrored its package version
  `2.1.2`; no matching Lite tag or release is claimed.
- Added `search` to the explicit read-only delegation boundary with byte-exact
  Full/Lite output and diagnostic conformance tests.
- Added the owner-ratified Rust retrieval workspace for the future static Lite
  path, bound to Full's versioned retrieval contracts and never a second GKX
  authority.
- Preserved the immutable Phase 0 compatibility bytes and added separately
  classified Phase 1 graph and Graphiti migration goldens.

## 1.1.3

- Engine-verbatim catch-up to `GKOS-Engine#v1.1.3`: bumped the `gkos-engine`
  pin, the root `package.json` version, and the release tag all to `1.1.3`.
- Re-resolved `package-lock.json` to the engine's v1.1.3 commit SHA (not a bare
  tag string), so the installed engine can no longer silently stay stale.
- Hardened `.github/workflows/pin-bump.yml`: the commit-and-tag step now runs
  `git pull --rebase origin <branch>` before pushing, so a merge that lands
  mid-run can no longer reject the push with `! [rejected] (fetch first)`.

## 1.1.2

- Adopted the engine-verbatim CLI versioning rule: the `okf-lite` CLI now
  carries the exact version of the `gkos-engine` release it pins. This release
  closes the tag gap between v1.0.4 and HEAD; the intermediate history is not
  retro-tagged.
- Bumped the engine pin to `GKOS-Engine#v1.1.2`.
- Derived the CLI version banner from `package.json` at runtime instead of the
  hardcoded literal in `bin/okf-lite.mjs`, which had drifted from the tags.
- Corrected the stale wrapper comment that referenced "gkos-engine v1.0.0".
- Added `.github/workflows/pin-bump.yml`, a `workflow_dispatch` job that adopts
  a given engine tag: updates the pin and version, tests, commits, and tags.
- Added VERSIONING.md documenting the two independent axes (CLI `vX.Y.Z`
  engine-verbatim; desktop `desktop-vA.B.C`). Desktop versioning is unchanged.

## 1.1.0

- Added opt-in `okf-lite assist <task> <note.md>` support.
- Restricted sidecar connections to loopback with optional bearer tokens.
- Validated every proposal with the canonical engine and rejected unsafe
  authoritative patches.
- Kept all four deterministic commands operational without DSPy.
- Updated the desktop package to 0.2.0.
- Simplified setup and settings language, moved ports, tokens, endpoints, and
  raw configuration behind Advanced controls, and added one-click setup copy.
- Added friendly assistance actions such as `explain`, `improve`, `repair`,
  `find-links`, and `check-privacy`.
- Updated CI to use reproducible lockfile installs and package-content checks.
- Updated the desktop installer workflow to consume and smoke-test the matching
  GKOS Engine v1.1.0 executable.
- Updated the desktop build toolchain to a patched Vite release; npm audit is
  clean.
