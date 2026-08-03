# GKOS-Engine-Lite

**Command-line GKX Notes (2.2) + Agent-Ready (flat 2.3) tooling for any
folder of Markdown notes.**

GKX (Governed Knowledge Exchange) is the current name for the model formerly
published as OKF+. Existing OKF+ notes and the stable `okf-lite` command remain
fully supported compatibility surfaces; the naming change alone requires no
document migration.

GKOS-Engine-Lite is the standalone, non-Obsidian counterpart to
[Kosmos Research Studio Lite (KRS-Lite)](https://github.com/Odenknight/Kosmos-Oden-Lite): it gives
individuals and small vaults the same GKOS-Engine-Lite schema — GKX Notes
(2.2) with the optional Agent-Ready flat 2.3 profile — as a command-line tool
you can point at any folder of Markdown notes. The CLI package has no Obsidian dependency, plugin, or GUI: just `okf-lite validate`, `assess`, `graph`, and `export` over a
directory.

It is a thin wrapper, not a reimplementation. Under the hood it depends
directly on [gkos-engine](https://github.com/Odenknight/GKOS-Engine) — the
canonical, deterministic engine that also powers KRS and KRS-Lite — and
re-exports its CLI commands unchanged. Same parser,
same validation and assessment behavior, through the same upstream execution
path. CI verifies the restricted command boundary and compatibility fixtures;
Lite does not independently implement deterministic semantics.

Version 1.1 also offers an optional `assist` command backed by a separately
configured local intelligence sidecar. It never modifies a note and prints
only engine-validated candidate proposals. Python, DSPy, a model, and provider
credentials are optional; `validate`, `assess`, `graph`, and `export` work
unchanged without them.

## Why "Lite"

The full [gkos-engine](https://github.com/Odenknight/GKOS-Engine) can read
and report on both OKF+ 2.3 dialects (the human/agent-editable **Agent-Ready
flat** profile, and the nested **Machine Dialect** used by heavier
governance workflows), and diagnostic commands are always honest about what
they find in a vault regardless of dialect — GKOS-Engine-Lite never hides or
misreports a note just because it's outside its intended audience.

What "Lite" narrows is documentation and positioning, not behavior: this
README and this package describe and support the everyday, individual-vault
workflow — OKF+ Notes (2.2) and Agent-Ready (flat 2.3) — and don't document
Machine-Dialect-specific workflows, sidecar governance, or proposal/decision
records. If you need those, use gkos-engine directly. If gkos-engine later
grows write-capable commands (migrate, proposals, decisions, mv, serve — none
exist yet as of the pinned gkos-engine release), GKOS-Engine-Lite's CLI surface will stay
limited to the four deterministic read-only commands below plus the separate,
proposal-only `assist` surface.

## Install

Requires Node >=22 <25.

```sh
npm install gkos-engine-lite
```

`gkos-engine` has no npm registry publish; it's installed as a pinned git
dependency (`github:Odenknight/GKOS-Engine#v1.1.3`, resolved to commit
`72c4a3268c9db132f2f9dd5aaa7eb7075e6bab2a`). Git installation runs the
engine package's standard `prepare` build.

## CLI: `okf-lite`

```sh
node bin/okf-lite.mjs validate ./my-notes
node bin/okf-lite.mjs assess   ./my-notes --json
node bin/okf-lite.mjs graph    ./my-notes -o graph.json
node bin/okf-lite.mjs export graphiti ./my-notes --episodes episodes.json
```

Optional assistance, when the loopback sidecar is running:

```sh
node bin/okf-lite.mjs assist explain ./my-notes/example.md
node bin/okf-lite.mjs assist improve ./my-notes/example.md
node bin/okf-lite.mjs assist repair ./my-notes/example.md
node bin/okf-lite.mjs assist find-links ./my-notes/example.md
node bin/okf-lite.mjs assist check-privacy ./my-notes/example.md
```

`GKOS_INTELLIGENCE_URL` may only name a loopback URL (default
`http://127.0.0.1:8765`). `GKOS_INTELLIGENCE_TOKEN` supplies an optional bearer
token. Every response is validated by the canonical engine and output is
labeled `authoritative: false`; there is no automatic write path.

The short action names let users choose what they want help with without
learning DSPy or internal contract vocabulary. Running `okf-lite assist` with
no other arguments prints friendly examples.

See [ROADMAP.md](ROADMAP.md) for completed work and the next usability,
evaluation, installer, and signing milestones.

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

## Desktop app — GKOS Engine Desktop

`desktop/` contains the separate **GKOS Engine Desktop** presentation package, a Tauri 2 tray app for macOS and
Windows that wraps the engine's headless sidecar (`kosmos-agent`, from
[gkos-engine v1.1.3](https://github.com/Odenknight/GKOS-Engine/releases/tag/v1.1.0)).
Point it at a notes folder; it watches and projects (OKF+ 2.3 + Graphiti) and
serves a **loopback-only, read-only, token-gated** agent API for local AI
assistants (Claude Desktop, Cursor, …). No cloud, no remote access, no tunnel.

A mandatory first-run wizard makes you choose a default sensitivity **before**
the API can ever be enabled (fail-closed to `secret`). Installer workflows are implemented and build **unsigned** artifacts. The
current Engine v1.1.3 configuration requires a fresh installer-matrix and
clean-machine verification before availability is claimed (`.github/workflows/desktop-build.yml`) — your OS will warn
on first open; the guides below cover the safe open-anyway steps.

- **[Quickstart](desktop/docs/QUICKSTART.md)** — download, install (unsigned),
  first-run setup, and connecting Claude Desktop.
- **[User Guide](desktop/docs/USER-GUIDE.md)** — full walkthrough: concepts,
  the wizard, settings, quick-connect snippets, privacy/safety FAQ, and
  troubleshooting.

The desktop frontend logic (snippet generation, settings validation) is
type-checked and unit-tested (`node --test`) in CI; Rust/Tauri compilation and
the `.dmg`/`.exe` bundles are produced exclusively on the CI matrix.

## Relationship to the rest of the GKOS family

| | GKOS-Engine-Lite (this repo) | GKOS-Engine (full) | Kosmos-Oden-Lite (frozen) |
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
- Note-format profiles: **GKX** (Governed Knowledge Exchange; formerly OKF+) under the
  **GKOS** (Governed Knowledge Operations Standard) governance model — see
  [gkos-standard](https://github.com/Odenknight/gkos-standard).
- First-party software license: Apache-2.0. Documentation and original graphics
  use CC BY 4.0 as described in [LICENSE](LICENSE). See [NOTICE](NOTICE),
  [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md), and
  [TRADEMARKS.md](TRADEMARKS.md).
