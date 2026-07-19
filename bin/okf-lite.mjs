#!/usr/bin/env node
/**
 * okf-lite — the GKOS-Engine-Lite CLI.
 *
 * A thin, faithful pass-through wrapper around the installed `gkos-engine`
 * package's own CLI (node_modules/gkos-engine/bin/okf.mjs). Same four
 * commands, same behavior, same output, byte for byte:
 *
 *   okf-lite validate <dir>
 *   okf-lite assess   <dir> [--json]
 *   okf-lite graph    <dir> -o graph.json [--watch]
 *   okf-lite export graphiti <dir> --episodes episodes.json [--group-id <ns>]
 *
 * GKOS-Engine-Lite does not reimplement the engine — it re-exports it. This
 * file only overrides the --help / no-args banner, so that GKOS-Engine-Lite
 * presents itself as OKF+ Notes (2.2) + Agent-Ready (flat 2.3) tooling for
 * everyday vaults, rather than describing the full engine's Machine Dialect
 * / governance-sidecar surface (which the underlying engine can still read
 * and report on honestly — GKOS-Engine-Lite just doesn't advertise it).
 *
 * If/when gkos-engine grows write-capable commands (migrate, proposals,
 * decisions, mv, serve — none exist as of gkos-engine v1.0.0), this wrapper
 * should keep exposing only validate/assess/graph/export, not those.
 */
import { createRequire } from "node:module";
import { realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);

// gkos-engine's package.json "exports" only maps "." (-> dist/kosmos-core.mjs),
// so its bin script isn't resolvable as a subpath import. Resolve the main
// entry instead and derive the package root from it (root/dist/x.mjs -> root).
const engineMainPath = require.resolve("gkos-engine");
const enginePkgRoot = dirname(dirname(engineMainPath));
const engineBinUrl = pathToFileURL(join(enginePkgRoot, "bin/okf.mjs")).href;

const engine = await import(engineBinUrl);

const BANNER = `okf-lite (GKOS-Engine-Lite) v1.0.0
GKOS-Engine-Lite — OKF+ Notes (2.2) + Agent-Ready (flat 2.3) tooling

A thin command-line wrapper around gkos-engine for individuals and small,
everyday vaults of Markdown notes — no Obsidian required.

Usage:
  okf-lite validate <dir>                                  schema/identity/lineage diagnostics; non-zero exit on error
  okf-lite assess   <dir> [--json]                         per-note documentation-quality scores/labels
  okf-lite graph    <dir> -o <graph.json> [--watch]        canonical Kosmos graph (stable serialization)
  okf-lite export graphiti <dir> --episodes <out.json> [--group-id <ns>]

See https://github.com/Odenknight/GKOS-Engine-Lite for docs, and
https://github.com/Odenknight/GKOS-Engine for the full engine this depends on.`;

export async function main(argv = process.argv.slice(2)) {
  const first = argv[0];
  if (!first || first === "--help" || first === "-h") {
    console.log(BANNER);
    return first ? 0 : 1;
  }
  return engine.main(argv);
}

// Determine whether this module was invoked directly as a CLI (as opposed to
// being imported). We compare the *realpath* of process.argv[1] against this
// module's own realpath, because Node resolves import.meta.url to the module's
// REAL path while argv[1] keeps the AS-INVOKED path. Under `npm link`, global
// bin shims, or pnpm/npm workspaces (symlinked package dirs), those raw strings
// differ — the naive comparison then fails, main() never runs, and the CLI
// exits 0 with zero output. Realpath resolution makes both sides canonical.
// If realpath throws (e.g. argv[1] no longer on disk), fall back to the raw
// URL comparison rather than crashing.
function isInvokedDirectly() {
  const invokedPath = process.argv[1];
  if (!invokedPath) return false;
  try {
    return realpathSync(invokedPath) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return pathToFileURL(invokedPath).href === import.meta.url;
  }
}

const invokedDirectly = isInvokedDirectly();
if (invokedDirectly) {
  const code = await main();
  if (typeof code === "number" && code !== 0) process.exit(code);
}
