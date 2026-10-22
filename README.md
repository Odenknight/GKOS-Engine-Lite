# GKOS-Engine-Lite

**Command-line GKX Notes (2.2) + Agent-Ready (flat 2.3) tooling for any
folder of Markdown notes.**

GKX (Governed Knowledge Exchange) is the name of the note exchange model.

> **Breaking rename:** the `okf-lite` command is now `gkx-lite`. No `okf-lite`
> alias is installed; update scripts and shell aliases. See
> [GKX-MIGRATION.md](GKX-MIGRATION.md).

GKOS-Engine-Lite is the standalone, non-Obsidian counterpart to
[Kosmos-Oden-Lite](https://github.com/Odenknight/Kosmos-Oden-Lite): it gives
individuals and small vaults the same GKOS-Engine-Lite schema — GKX Notes
(2.2) with the optional Agent-Ready flat 2.3 profile — as a command-line tool
you can point at any folder of Markdown notes. The CLI package has no Obsidian
dependency, plugin, or GUI: just `gkx-lite validate`, `index`, `assess`,
`search`, `graph`, and `export` over a directory.

## Current build status

The qualified functional-uplift source is
`codex/phase-3-ingest-validation` at
`41912fd6db279f1b46e67cb4b88c1f1b4ba86e63`. PR #22 admits its Phase 0–3
chain from pre-admission Lite `main`
`2ebbf77583af3e83032054f1256188dc56376907` and records the `v2.1.2` source
release. This does not publish or qualify a one-file static Lite artifact or a
Desktop installer release.

The current bounded Desktop qualification candidate is
`codex/qualify-lite-review-20260901`. Its CI-enablement change begins at
`1fabff81caf2286f24d4cab45188eeb6a59d21ad`: review-branch pushes now exercise
the normal CI lanes, and the Windows native lane executes the frontend's
packaged-runtime tests after acquiring and checksum-verifying the sidecar.
Local tests and builds are review evidence only. Hosted Windows, Rust/Tauri,
MSRV, formatting, Clippy, installer-matrix, and clean-machine results remain
required before any corresponding platform or availability claim.

For the exact implemented/deferred matrix, read
[Current capabilities](docs/CURRENT_CAPABILITIES.md). Builders continuing the
qualified branch should start with the
[current build handoff](docs/HANDOFF_2026-08-27.md).

The published JavaScript CLI remains a thin delegate. Under the hood it depends
directly on [gkos-engine](https://github.com/Odenknight/GKOS-Engine) — the
canonical, deterministic engine that also powers KRS and Kosmos-Oden-Lite — and
delegates its allowed GKX commands unchanged. Same parser, same validation and
assessment behavior, through the same upstream execution path. The new
`rust/` retrieval core is the separately governed future static-Lite path: a
pin-bound implementation of Full-owned retrieval contracts with exact
cross-language conformance, never a second GKX parser, lineage model, or
authority. No static distribution or platform qualification is claimed yet.
CI verifies both the restricted command boundary and compatibility fixtures.

Version 1.1 also offers an optional `assist` command backed by a separately
configured local intelligence sidecar. It never modifies a note and prints
only engine-validated candidate proposals. Python, DSPy, a model, and provider
credentials are optional; `validate`, `assess`, `search`, `graph`, and `export` work
unchanged without them.

## Why "Lite"

The full [gkos-engine](https://github.com/Odenknight/GKOS-Engine) can read
and report on both GKX 2.3 dialects (the human/agent-editable **Agent-Ready
flat** profile, and the nested **Machine Dialect** used by heavier
governance workflows), and diagnostic commands are always honest about what
they find in a vault regardless of dialect — GKOS-Engine-Lite never hides or
misreports a note just because it's outside its intended audience.

What "Lite" narrows is documentation and positioning, not behavior: this
README and this package describe and support the everyday, individual-vault
workflow — GKX Notes (2.2) and Agent-Ready (flat 2.3) — and don't document
Machine-Dialect-specific workflows, sidecar governance, or proposal/decision
records. If you need those, use gkos-engine directly. The Phase 3 `index`
command writes only sealed derived retrieval generations under the pinned
Engine's fail-closed owner protocol; it never changes source notes. If
gkos-engine later grows broader write-capable commands (migrate, proposals,
decisions, mv, or serve), GKOS-Engine-Lite's CLI surface will stay limited to
the six delegated commands below plus the separate, proposal-only `assist`
surface.

## Install

Requires Node >=22 <25.

```sh
npm install gkos-engine-lite
```

`gkos-engine` has no npm registry publish; this integration branch installs
Engine package 2.1.2 from the exact reviewed commit
`e7cc0dd478af3d0bda216c5258dec5f77932def7`. This commit pin is not a claim
that a matching Engine or Lite release tag exists. Git installation runs the
engine package's standard `prepare` build.

Derived graph and projection envelopes produced by this pin use the canonical
GKX namespace. Notes that still carry the historical `okf_version` key are read
as legacy input and reported with a `GKX-SCHEMA-003` diagnostic; no alias is
accepted, so migrate them to `gkx_version`. The preserved Phase 0 and
classified Phase 1 goldens document that migration explicitly.

## CLI: `gkx-lite`

```sh
node bin/gkx-lite.mjs validate ./my-notes
node bin/gkx-lite.mjs validate --kb-path ./my-notes --schema gkos:frontmatter-profile/current --format json
node bin/gkx-lite.mjs index --kb-path ./my-notes --strict
node bin/gkx-lite.mjs assess   ./my-notes --json
node bin/gkx-lite.mjs search   "canonical policy" --kb-path ./my-notes --limit 5
node bin/gkx-lite.mjs search   "historical policy" --kb-path ./my-notes --as-of 2026-07-15T00:00-04:00 --limit 5
node bin/gkx-lite.mjs graph    ./my-notes -o graph.json
node bin/gkx-lite.mjs export graphiti ./my-notes --episodes episodes.json
```

Optional assistance, when the loopback sidecar is running:

```sh
node bin/gkx-lite.mjs assist explain ./my-notes/example.md
node bin/gkx-lite.mjs assist improve ./my-notes/example.md
node bin/gkx-lite.mjs assist repair ./my-notes/example.md
node bin/gkx-lite.mjs assist find-links ./my-notes/example.md
node bin/gkx-lite.mjs assist check-privacy ./my-notes/example.md
```

`GKOS_INTELLIGENCE_URL` may only name a loopback URL (default
`http://127.0.0.1:8765`). `GKOS_INTELLIGENCE_TOKEN` supplies an optional bearer
token. Every response is validated by the canonical engine and output is
labeled `authoritative: false`; there is no automatic write path.

The short action names let users choose what they want help with without
learning DSPy or internal contract vocabulary. Running `gkx-lite assist` with
no other arguments prints friendly examples.

See [ROADMAP.md](ROADMAP.md) for completed work and the next usability,
evaluation, installer, and signing milestones.

### `gkx-lite validate <dir>`

Runs the deterministic parser/projection/validation over every note in
`<dir>` and prints a summary plus per-note diagnostics. Exits non-zero if any
`error` or `critical` diagnostics are found.

The additive Phase 3 form is
`gkx-lite validate --kb-path <path> [--schema <path-or-id>] [--format text|json]`.
Omitting `--schema` or selecting exact
`gkos:frontmatter-profile/current` uses the frozen built-in profile; an
explicit local strict-TOML overlay may only tighten it. Lite delegates the
original arguments and output bytes to Full and does not parse YAML or TOML.

### `gkx-lite index --kb-path <path> [--schema <path-or-id>] [--strict]`

Delegates Phase 3 ingest validation and atomic derived-generation publication
to the pinned Engine. Strict mode blocks publication on intrinsic validation
errors; non-strict mode excludes invalid sources whole and may publish a
sealed generation with rejections. Full remains the sole parser, profile,
owner-storage, and activation authority; source-note bytes are never changed.

### `gkx-lite assess <dir> [--json]`

Runs the assessment engine over every note and prints per-note
documentation-quality scores and labels. `--json` emits a
stable-key-ordered JSON array instead of the human-readable table.

### `gkx-lite search <query> --kb-path <dir> [--limit <n>] [--as-of <GKX timestamp>]`

Delegates to the pinned Engine retrieval CLI and returns the versioned
retrieval result contract with explicit provider stages, confidence, and exact
verified citations. The mandatory path is local lexical retrieval; optional
provider configuration does not change GKX identity or authority. On this
Phase 3 branch, Lite preserves the optional `--as-of` flag and its value
unchanged at the delegation boundary. The wrapper and pinned Full CLI execute
the same corpus and trusted configuration with byte-equivalent exit status,
stdout, and stderr; only the process-specific PID on the exact Node SQLite
experimental-warning line is normalized by the test. See
[Citation and temporal provenance](docs/citations.md) for the pin-bound result
semantics and authority boundary.

### `gkx-lite graph <dir> -o <graph.json> [--watch]`

Builds the canonical Kosmos graph (nodes, links, stats, diagnostics) with
stable serialization. `--watch` rebuilds on change.

### `gkx-lite export graphiti <dir> --episodes <out.json> [--group-id <ns>]`

Exports Graphiti episodes for the corpus.

Every command embeds a deterministic `build:` block
(`engine_version`, `policy_hash`, `corpus_hash`, `generated_at`) so output is
reproducible and auditable.

## Desktop app — GKOS-Engine-Lite Desktop

`desktop/` contains the separate **GKOS-Engine-Lite Desktop** presentation package, a Tauri 2 tray app for macOS and
Windows that wraps the engine's headless sidecar (`kosmos-agent`). The CLI
integration is bound to [Engine package 2.1.2 at exact commit
`e7cc0dd`](https://github.com/Odenknight/GKOS-Engine/commit/e7cc0dd478af3d0bda216c5258dec5f77932def7);
desktop sidecar artifacts remain separately qualified and are not implied by
that source pin. The bundled sidecar is still GKOS-Engine v1.1.3, which predates
the GKX rename and reads only `okf_version`; see [GKX-MIGRATION.md](GKX-MIGRATION.md).
Point it at a notes folder; it watches and projects (GKX 2.3 + Graphiti) and
serves a **loopback-only, read-only, token-gated, GET-only REST API** for local
clients. The checksum-pinned packaged sidecar is GKOS-Engine `v1.1.3`, SHA-256
`29ab43c9ce79b8c14978594a18523a04e3f7518d87560f1e4c17744da08f12c2`.
It does not expose MCP: `/mcp` is deliberately absent, and quick-connect
commands target only implemented REST routes. No cloud, remote-bind, built-in
proxy, or built-in tunnel is provided.

A mandatory first-run wizard makes you choose a default sensitivity **before**
the API can ever be enabled (fail-closed to `secret`). Installer workflows are implemented and build **unsigned** artifacts. The
current Engine package 2.1.2 integration requires a fresh installer-matrix and
clean-machine verification before availability is claimed (`.github/workflows/desktop-build.yml`) — your OS will warn
on first open; the guides below cover the safe open-anyway steps.

- **[Quickstart](desktop/docs/QUICKSTART.md)** — download, install (unsigned),
  first-run setup, and connecting Claude Desktop.
- **[User Guide](desktop/docs/USER-GUIDE.md)** — full walkthrough: concepts,
  the wizard, settings, quick-connect snippets, privacy/safety FAQ, and
  troubleshooting.

The desktop frontend logic (snippet generation, settings validation) is
type-checked and unit-tested (`node --test`) in CI. On Windows, the native lane
also exercises those advertised commands against the acquired, checksum-pinned
packaged REST sidecar before running native Tauri tests. Rust/Tauri compilation
and the `.dmg`/`.exe` bundles are produced exclusively on the CI matrix; the
presence of these jobs is not itself a successful hosted qualification.

## Relationship to the rest of the GKOS family

| | GKOS-Engine-Lite (this repo) | GKOS-Engine (full) | Kosmos-Oden-Lite (frozen) |
|---|---|---|---|
| Interface | Command-line, any folder of notes | Command-line, any folder of notes | Obsidian plugin |
| Audience | Everyday vaults, individuals | Governed knowledge work, agentic systems | Everyday Obsidian vaults |
| Note formats documented | GKX Notes (2.2) + Agent-Ready flat 2.3 | Same, plus Machine Dialect and governance sidecars | GKX Notes (2.2) + Agent-Ready flat 2.3 |

Notes formatted by any of these are fully readable by the others — the
schema is shared, only the audience-facing documentation and surface area
differ.

## Attribution and license

- Engine: [gkos-engine](https://github.com/Odenknight/GKOS-Engine) by
  **Shaun "Oden" Marshall** ([Odenknight](https://github.com/Odenknight)).
- Note-format profiles: **GKX** (Governed Knowledge Exchange) under the
  **GKOS** (Governed Knowledge Operations Standard) governance model — see
  [gkos-standard](https://github.com/Odenknight/gkos-standard).
- First-party software license: Apache-2.0. Documentation and original graphics
  use CC BY 4.0 as described in [LICENSE](LICENSE). See [NOTICE](NOTICE),
  [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md), and
  [TRADEMARKS.md](TRADEMARKS.md).
