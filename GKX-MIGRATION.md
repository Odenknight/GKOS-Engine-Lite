# GKX naming migration

GKX (Governed Knowledge Exchange) is the name of the note exchange model
formerly published as OKF+.

This is a breaking rename with no aliases, matching GKOS-Engine 2.0.0:

- The CLI command `okf-lite` is now `gkx-lite`, installed from
  `bin/gkx-lite.mjs`. No `okf-lite` alias bin is installed; update scripts,
  shell aliases, and integrations that invoke the old name.
- Notes use the `gkx_version` frontmatter key. A note that still carries the
  historical `okf_version` key is read as legacy input and reported with a
  `GKX-SCHEMA-003` diagnostic by the pinned engine; it is not accepted as an
  alias.
- Runtime state, diagnostics, and profile identifiers use the GKX namespace
  (`.gkx/`, `GKX-*`, `gkx-2.3-validating-projection`, `gkx23-default-v1`), as
  defined by the pinned GKOS-Engine.

The Desktop app's display name is now GKOS-Engine-Lite Desktop; its bundle
identifier is unchanged.

**Desktop limitation.** The Desktop app still bundles the `kosmos-agent`
sidecar from GKOS-Engine v1.1.3, which predates the GKX rename and reads only
`okf_version`. Until the sidecar is re-pinned to a GKX engine, notes renamed to
`gkx_version` are treated as unversioned by the Desktop app. The `gkx-lite`
CLI is not affected. Historical changelog entries and dated evidence
records keep the names that were current when they were written.
