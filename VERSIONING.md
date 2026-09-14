# Versioning

The canonical versioning policy for all GKOS repositories lives in
[`gkos-standard/VERSIONING.md`](https://github.com/Odenknight/gkos-standard/blob/main/VERSIONING.md).
This file is a pointer; the standard governs.

## Rule for this repo (GKOS-Engine-Lite) — two independent axes

**1. CLI — engine-verbatim package version, immutable Engine revision.**
The `okf-lite` CLI is a pass-through wrapper, so it carries the exact version of
the engine package it pins. Normal releases use a reviewed `vX.Y.Z` tag. During
an integration branch, a reviewed 40-character Engine commit may be pinned
before a matching tag exists:

- pin `gkos-engine` to the exact reviewed tag or commit;
- set root `package.json` `"version"` to the resolved Engine package version;
- create a Lite tag only as part of a separately authorized release.

The CLI never invents its own number, so its tag sequence skips any engine
version that produced no Lite release. The active Phase 3 integration pins
Engine commit `e7cc0dd478af3d0bda216c5258dec5f77932def7`, whose package version is
`2.1.2`. Both source tags now exist: Full `v2.1.2` resolves to
`7bf14b481e78c5ae9d1e14661602be4f24559d0e`, and Lite `v2.1.2` resolves to
`4027bfc4499ad0a2f3e753401f1320468e283823`. The Full dependency pin is a
different revision from the Full tag; matching package versions do not prove
behavioral equivalence. Source tags do not establish npm or installer publication.
`bin/okf-lite.mjs` reads the version from
`package.json` at runtime — do not hardcode it. The
`.github/workflows/pin-bump.yml` `workflow_dispatch` job automates this.

**2. Desktop app — independent, tagged `desktop-vA.B.C`.**
`desktop/` versions on its own product cadence and is never renumbered to match
the engine.
