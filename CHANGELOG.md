# Changelog

## 1.1.1

- Built the Intel macOS desktop target on the `macos-latest` (arm64) runner
  instead of a dedicated Intel runner: Tauri cross-compiles to
  `x86_64-apple-darwin` on an Apple Silicon host, so the `.dmg` no longer
  depends on the Intel runner pool that queued for ~24h and left the x64
  installer unpublished in rc3 and rc4.
- Pointed the sidecar download at GKOS-Engine `v1.1.1`, the first engine
  release that publishes `kosmos-agent-x86_64-apple-darwin`.
- Made the in-run SEA fallback pass `--target-arch x64` for the Intel leg so
  it matches the engine's cross-arch build script.
- Added hard gates on the acquired sidecar: exact Mach-O architecture and a
  valid code signature, so a wrong-arch binary can no longer be bundled
  silently.
- Asserted the Rust target is actually installed before invoking the Tauri
  build.

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
