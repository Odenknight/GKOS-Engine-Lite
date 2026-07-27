# Versioning

The canonical versioning policy for all GKOS repositories lives in
[`gkos-standard/VERSIONING.md`](https://github.com/Odenknight/gkos-standard/blob/main/VERSIONING.md).
This file is a pointer; the standard governs.

## Rule for this repo (GKOS-Engine-Lite) — two independent axes

**1. CLI — engine-verbatim, tagged `vX.Y.Z`.**
The `okf-lite` CLI is a pass-through wrapper, so it carries the exact version of
the engine it pins. When GKOS-Engine releases `vX.Y.Z`:

- set the `gkos-engine` dependency pin to `github:Odenknight/GKOS-Engine#vX.Y.Z`
- set root `package.json` `"version"` to the same `X.Y.Z`
- tag `vX.Y.Z`

The CLI never invents its own number, so its tag sequence skips any engine
version that produced no Lite release. `bin/okf-lite.mjs` reads the version from
`package.json` at runtime — do not hardcode it. The
`.github/workflows/pin-bump.yml` `workflow_dispatch` job automates this.

**2. Desktop app — independent, tagged `desktop-vA.B.C`.**
`desktop/` versions on its own product cadence and is never renumbered to match
the engine.
