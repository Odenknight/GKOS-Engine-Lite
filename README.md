# GKOS-Engine-Lite

**Command-line OKF+ Notes (2.2) + Agent-Ready (flat 2.3) tooling for any
folder of Markdown notes.**

GKOS-Engine-Lite is the standalone, non-Obsidian counterpart to
[Kosmos-Oden-Lite](https://github.com/Odenknight/Kosmos-Oden-Lite): it gives
individuals and small vaults the same GKOS-Engine-Lite schema — OKF+ Notes
(2.2) with the optional Agent-Ready flat 2.3 profile — as a command-line tool
you can point at any folder of Markdown notes. No Obsidian, no plugin, no
GUI: just `okf-lite validate`, `assess`, `graph`, and `export` over a
directory.

It is a thin wrapper, not a reimplementation. Under the hood it depends
directly on [gkos-engine](https://github.com/Odenknight/GKOS-Engine) — the
canonical, deterministic engine that also powers Kosmos-Oden and
Kosmos-Oden-Lite — and re-exports its CLI commands unchanged. Same parser,
same validation, same assessment scoring, same output, byte for byte.

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
limited to the four read-only diagnostic commands below.

## Install

Requires Node >=22 <25.

```sh
npm install gkos-engine-lite
```

`gkos-engine` has no npm registry publish; it's installed as a pinned git
dependency (`github:Odenknight/GKOS-Engine#v1.0.8`). Its own package doesn't
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

## Desktop app — GKOS Engine Desktop

`desktop/` contains **GKOS Engine Desktop**, a Tauri 2 tray app for macOS and
Windows that wraps the engine's headless sidecar (`kosmos-agent`, from
[gkos-engine v1.0.8](https://github.com/Odenknight/GKOS-Engine/releases/tag/v1.0.8)).
Point it at a notes folder; it watches and projects (OKF+ 2.3 + Graphiti) and
serves a **loopback-only, read-only, token-gated** agent API for local AI
assistants (Claude Desktop, Cursor, …). No cloud, no remote access, no tunnel.

A mandatory first-run wizard makes you choose a default sensitivity **before**
the API can ever be enabled (fail-closed to `secret`). Installers are built
**unsigned** by CI (`.github/workflows/desktop-build.yml`) — your OS will warn
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
