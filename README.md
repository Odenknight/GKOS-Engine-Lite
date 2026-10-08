# GKOS-Engine-Lite

> **Portfolio position** · Product: GKOS-Engine-Lite · Repository: `Odenknight/GKOS-Engine-Lite`
> Tier: TF (dependency class: T2 embedding) · GKX schema: GKX Notes 2.2 + flat 2.3
> Engine dependency: embeds GKOS-Engine at frozen pin `v1.0.4`
> Lifecycle: Frozen · Relationships: none
> Authority for this block: GKOS-REGISTRY-001

GKOS-Engine-Lite is a frozen lighter legacy Engine line. It provides
command-line tooling for GKX Notes 2.2 with the optional Agent-Ready flat 2.3
profile. It embeds GKOS-Engine at the pinned historical `v1.0.4` version; it
does not track current Engine upgrades.

It is a thin wrapper, not a reimplementation; it does not independently
implement deterministic semantics. Its pinned embedded Engine supplies the
parser, validation, assessment scoring, and output. This frozen repository
receives documentation, security, and repository-maintenance corrections only.

## Why "Lite"

The full [gkos-engine](https://github.com/Odenknight/GKOS-Engine) can read
and report on both OKF+ 2.3 dialects (the human/agent-editable **Agent-Ready
flat** profile, and the nested **Machine Dialect** used by heavier
governance workflows), and diagnostic commands are always honest about what
they find in a vault regardless of dialect — GKOS-Engine-Lite never hides or
misreports a note just because it's outside its intended audience.

What "Lite" historically narrowed is documentation and positioning, not behavior: this
README and this package describe and support the everyday, individual-vault
workflow — OKF+ Notes (2.2) and Agent-Ready (flat 2.3) — and don't document
Machine-Dialect-specific workflows, sidecar governance, or proposal/decision
records. If you need current Engine capabilities, use GKOS-Engine directly.

## Install

Requires Node >=22 <25.

```sh
npm install gkos-engine-lite
```

`gkos-engine` has no npm registry publish; it's installed as a pinned git
dependency (`github:Odenknight/GKOS-Engine#v1.0.4`). Its own package doesn't
ship a prebuilt bundle for git installs, so a `postinstall` script
(`scripts/build-engine.mjs`) bundles it locally with esbuild the first time
you install — this is transparent and only runs once.

## CLI: `okf-lite`

```sh
node bin/okf-lite.mjs validate ./my-notes
node bin/okf-lite.mjs assess   ./my-notes --json
node bin/okf-lite.mjs graph    ./my-notes -o graph.json
node bin/okf-lite.mjs export graphiti ./my-notes --episodes episodes.json
```

### `okf-lite validate <dir>`

Runs the deterministic parser/projection/validation over every note in
`<dir>` and prints a summary plus per-note diagnostics. Exits non-zero if any
`error` or `critical` diagnostics are found.

### `okf-lite assess <dir> [--json]`

Runs the assessment engine over every note and prints per-note
documentation-quality scores and labels. `--json` emits a
stable-key-ordered JSON array instead of the human-readable table.

### `okf-lite graph <dir> -o <graph.json> [--watch]`

Builds the canonical Kosmos graph (nodes, links, stats, diagnostics) with
stable serialization. `--watch` rebuilds on change.

### `okf-lite export graphiti <dir> --episodes <out.json> [--group-id <ns>]`

Exports Graphiti episodes for the corpus.

Every command embeds a deterministic `build:` block
(`engine_version`, `policy_hash`, `corpus_hash`, `generated_at`) so output is
reproducible and auditable.

## Relationship to the rest of the GKOS family

| | GKOS-Engine-Lite (this repo) | GKOS-Engine (full) | Kosmos-Oden-Lite |
|---|---|---|---|
| Interface | Command-line, any folder of notes | Command-line, any folder of notes | Obsidian plugin |
| Audience | Everyday vaults, individuals | Governed knowledge work, agentic systems | Everyday Obsidian vaults |
| Note formats documented | OKF+ Notes (2.2) + Agent-Ready flat 2.3 | Same, plus Machine Dialect and governance sidecars | OKF+ Notes (2.2) + Agent-Ready flat 2.3 |

Notes formatted by any of these are fully readable by the others — the
schema is shared, only the audience-facing documentation and surface area
differ.

## Attribution and license

- Engine: [gkos-engine](https://github.com/Odenknight/GKOS-Engine) by
  **Shaun "Oden" Marshall** ([Odenknight](https://github.com/Odenknight)).
- Note-format profiles: **OKF+** (Open Knowledge Format Plus) under the
  **GKOS** (Governed Knowledge Operations Standard) governance model — see
  [gkos-standard](https://github.com/Odenknight/gkos-standard).
- License: [MIT](LICENSE).
