import { test } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const execFileAsync = promisify(execFile);

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtures = join(root, "test/fixtures/notes");
const okfLiteBin = join(root, "bin/okf-lite.mjs");
const okfEngineBin = join(root, "node_modules/gkos-engine/bin/okf.mjs");

async function run(bin, args) {
  try {
    const { stdout } = await execFileAsync(process.execPath, [bin, ...args]);
    return { stdout, code: 0 };
  } catch (e) {
    return { stdout: e.stdout ?? "", code: e.code ?? 1 };
  }
}

test("okf-lite validate produces identical output to gkos-engine's own CLI", async () => {
  const lite = await run(okfLiteBin, ["validate", fixtures]);
  const engine = await run(okfEngineBin, ["validate", fixtures]);
  assert.equal(lite.code, engine.code);
  assert.equal(lite.stdout, engine.stdout);
});

test("okf-lite assess --json produces identical output to gkos-engine's own CLI", async () => {
  const lite = await run(okfLiteBin, ["assess", fixtures, "--json"]);
  const engine = await run(okfEngineBin, ["assess", fixtures, "--json"]);
  assert.equal(lite.code, engine.code);
  assert.equal(lite.stdout, engine.stdout);
  // Sanity: it's real JSON, not an accidental empty pass-through.
  const parsed = JSON.parse(lite.stdout);
  assert.ok(Array.isArray(parsed) && parsed.length > 0);
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
