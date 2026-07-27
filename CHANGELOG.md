# Changelog

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
