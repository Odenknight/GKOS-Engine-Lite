import { test } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const execFileAsync = promisify(execFile);

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtures = join(root, "test/fixtures/notes");
const okfLiteBin = join(root, "bin/okf-lite.mjs");
const okfEngineBin = join(root, "node_modules/gkos-engine/bin/gkx.mjs");

const SEARCH_NOTE = `---
gkx_version: "2.3"
uid: "019b2d14-4230-7db7-87d4-7d81cfaecb01"
title: "Retrieval policy"
type: "note"
created_at: "2026-08-20T12:00:00Z"
epistemic_state: "hypothesis"
sensitivity: "public"
authorship_origin: "authored"
---
# Retrieval policy

Deterministic retrieval cites canonical policy sources.
`;

async function run(bin, args) {
  try {
    const { stdout, stderr } = await execFileAsync(process.execPath, [bin, ...args]);
    return { stdout, stderr, code: 0 };
  } catch (e) {
    return { stdout: e.stdout ?? "", stderr: e.stderr ?? "", code: e.code ?? 1 };
  }
}

function normalizeSqliteExperimentalWarningPid(stderr) {
  return stderr.replace(
    /^(\(node:)\d+(\) ExperimentalWarning: SQLite is an experimental feature and might change at any time)(\r?)$/gm,
    "$1<PID>$2$3",
  );
}

function assertEquivalentStderr(actual, expected) {
  assert.equal(
    normalizeSqliteExperimentalWarningPid(actual),
    normalizeSqliteExperimentalWarningPid(expected),
  );
}

test("okf-lite validate produces identical output to gkos-engine's own CLI", async () => {
  const lite = await run(okfLiteBin, ["validate", fixtures]);
  const engine = await run(okfEngineBin, ["validate", fixtures]);
  assert.equal(lite.code, engine.code);
  assert.equal(lite.stdout, engine.stdout);
  assertEquivalentStderr(lite.stderr, engine.stderr);
});

test("okf-lite assess --json produces identical output to gkos-engine's own CLI", async () => {
  const lite = await run(okfLiteBin, ["assess", fixtures, "--json"]);
  const engine = await run(okfEngineBin, ["assess", fixtures, "--json"]);
  assert.equal(lite.code, engine.code);
  assert.equal(lite.stdout, engine.stdout);
  assertEquivalentStderr(lite.stderr, engine.stderr);
  // Sanity: it's real JSON, not an accidental empty pass-through.
  const parsed = JSON.parse(lite.stdout);
  assert.ok(Array.isArray(parsed) && parsed.length > 0);
});

test("okf-lite search returns byte-identical contract output through the Engine CLI boundary", async () => {
  const kb = await mkdtemp(join(tmpdir(), "gkos-lite-search-"));
  const config = join(kb, "operator.toml");
  try {
    await writeFile(join(kb, "policy.md"), SEARCH_NOTE, "utf8");
    await writeFile(config, "config_version = 1\n[retrieval]\nmode = \"fts\"\n", "utf8");
    const argv = ["search", "canonical policy", "--kb-path", kb, "--config", config, "--limit", "5"];
    const lite = await run(okfLiteBin, argv);
    const engine = await run(okfEngineBin, argv);
    assert.equal(lite.code, engine.code);
    assert.equal(lite.stdout, engine.stdout);
    assertEquivalentStderr(lite.stderr, engine.stderr);
    const result = JSON.parse(lite.stdout);
    assert.equal(result.contract_version, "gkos-retrieval/1.0.0-draft.2");
    assert.equal(result.hits.length, 1);
    assert.equal(result.hits[0].citation.verified, true);
    assert.equal(await readFile(join(kb, "policy.md"), "utf8"), SEARCH_NOTE);
  } finally {
    await rm(kb, { recursive: true, force: true });
  }
});

test("okf-lite search --as-of is byte-identical to the exact pinned Full CLI", async () => {
  const kb = await mkdtemp(join(tmpdir(), "gkos-lite-search-as-of-"));
  const config = join(kb, "operator.toml");
  const asOf = "2026-08-20T08:00:00-04:00";
  try {
    await writeFile(join(kb, "policy.md"), SEARCH_NOTE, "utf8");
    await writeFile(config, "config_version = 1\n[retrieval]\nmode = \"fts\"\n", "utf8");
    const argv = [
      "search",
      "canonical policy",
      "--kb-path",
      kb,
      "--config",
      config,
      "--as-of",
      asOf,
      "--limit",
      "5",
    ];
    const lite = await run(okfLiteBin, argv);
    const engine = await run(okfEngineBin, argv);
    assert.equal(lite.code, engine.code);
    assert.equal(lite.stdout, engine.stdout);
    assertEquivalentStderr(lite.stderr, engine.stderr);
    const result = JSON.parse(lite.stdout);
    assert.equal(result.contract_version, "gkos-retrieval/1.0.0-draft.2");
    assert.equal(result.temporal.as_of, "2026-08-20T12:00:00.000Z");
    assert.equal(result.hits.length, 1);
    assert.equal(result.hits[0].citation.verified, true);
    assert.equal(result.hits[0].provenance.ledger_binding_verified, false);
    assert.equal(Object.hasOwn(result.hits[0].provenance, "ledger_hash"), false);
    assert.equal(await readFile(join(kb, "policy.md"), "utf8"), SEARCH_NOTE);
  } finally {
    await rm(kb, { recursive: true, force: true });
  }
});

test("stderr parity normalizes only the exact SQLite experimental-warning PID", () => {
  const sqliteA = "(node:123) ExperimentalWarning: SQLite is an experimental feature and might change at any time\n";
  const sqliteB = "(node:9876) ExperimentalWarning: SQLite is an experimental feature and might change at any time\n";
  assert.equal(
    normalizeSqliteExperimentalWarningPid(sqliteA),
    normalizeSqliteExperimentalWarningPid(sqliteB),
  );

  const unrelatedA = "(node:123) ExperimentalWarning: unrelated sentinel warning\n";
  const unrelatedB = "(node:9876) ExperimentalWarning: unrelated sentinel warning\n";
  assert.equal(normalizeSqliteExperimentalWarningPid(unrelatedA), unrelatedA);
  assert.equal(normalizeSqliteExperimentalWarningPid(unrelatedB), unrelatedB);
  assert.notEqual(
    normalizeSqliteExperimentalWarningPid(unrelatedA),
    normalizeSqliteExperimentalWarningPid(unrelatedB),
  );
});

test("okf-lite --help banner reflects the Lite scope framing", async () => {
  const { stdout, code } = await run(okfLiteBin, ["--help"]);
  assert.equal(code, 0);
  assert.match(stdout, /GKOS-Engine-Lite/);
  assert.match(stdout, /OKF\+ Notes \(2\.2\) \+ Agent-Ready \(flat 2\.3\)/);
  assert.doesNotMatch(stdout, /Machine Dialect/);
});

test("okf-lite with no args exits non-zero and still prints the Lite banner", async () => {
  const { stdout, code } = await run(okfLiteBin, []);
  assert.equal(code, 1);
  assert.match(stdout, /GKOS-Engine-Lite/);
});
