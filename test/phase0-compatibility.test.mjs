import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { promisify } from "node:util";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import assert from "node:assert/strict";

import { validateLiteCommand } from "../bin/okf-lite.mjs";
import { deterministicArtifacts, runtimeSnapshot } from "./support/phase0-snapshot.mjs";

const execFileAsync = promisify(execFile);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = resolve(root, "test/fixtures/compatibility");
const fixture = JSON.parse(await readFile(resolve(fixtureRoot, "phase0-lite.json"), "utf8"));

async function runCli(args) {
  try {
    const result = await execFileAsync(process.execPath, [resolve(root, "bin/okf-lite.mjs"), ...args], {
      encoding: null,
    });
    return { code: 0, stdout: result.stdout, stderr: result.stderr };
  } catch (error) {
    return {
      code: typeof error.code === "number" ? error.code : 1,
      stdout: error.stdout ?? Buffer.alloc(0),
      stderr: error.stderr ?? Buffer.alloc(0),
    };
  }
}

function assertCompatibilitySnapshot(actual) {
  assert.deepEqual(actual, fixture.runtime_snapshot);
}

test("Phase 0 runtime/package/export snapshot matches the pinned baseline", async () => {
  assertCompatibilitySnapshot(await runtimeSnapshot());
});

test("Phase 0 delegated and local proposal-only boundaries match the compatibility fixture", () => {
  for (const argv of fixture.cli.delegated_allowed_argv) {
    assert.deepEqual(validateLiteCommand(argv), { allowed: true }, `expected allowed: ${argv.join(" ")}`);
  }
  for (const argv of fixture.cli.local_proposal_only_argv) {
    assert.equal(argv[0], "assist", `expected a local proposal command: ${argv.join(" ")}`);
    assert.equal(
      validateLiteCommand(argv).allowed,
      false,
      `local proposal command must not cross the delegated Engine boundary: ${argv.join(" ")}`,
    );
  }
  for (const argv of fixture.cli.blocked_argv) {
    assert.equal(validateLiteCommand(argv).allowed, false, `expected blocked: ${argv.join(" ")}`);
  }
});

test("Phase 0 CLI help and no-argument behavior are byte-compatible", async () => {
  const expected = await readFile(resolve(fixtureRoot, fixture.cli.help_stdout_file));
  const help = await runCli(["--help"]);
  assert.equal(help.code, fixture.cli.help_exit);
  assert.deepEqual(help.stdout, expected);
  assert.equal(help.stderr.length, 0);

  const noArgs = await runCli([]);
  assert.equal(noArgs.code, fixture.cli.no_args_exit);
  assert.deepEqual(noArgs.stdout, expected);
  assert.equal(noArgs.stderr.length, 0);
});

test("Phase 0 deterministic graph and Graphiti bytes match their goldens", async () => {
  const actual = deterministicArtifacts();
  const [graph, graphiti] = await Promise.all([
    readFile(resolve(fixtureRoot, fixture.deterministic.graph_file)),
    readFile(resolve(fixtureRoot, fixture.deterministic.graphiti_file)),
  ]);
  assert.deepEqual(actual.graphBytes, graph);
  assert.deepEqual(actual.graphitiBytes, graphiti);
});

test("Phase 0 deterministic golden comparison rejects simulated CRLF checkout bytes", async () => {
  const actual = deterministicArtifacts().graphBytes;
  const golden = await readFile(resolve(fixtureRoot, fixture.deterministic.graph_file));
  const simulatedCrlfCheckout = Buffer.from(golden.toString("utf8").replace(/(?<!\r)\n/g, "\r\n"), "utf8");
  assert.notDeepEqual(simulatedCrlfCheckout, golden);
  assert.throws(() => assert.deepEqual(actual, simulatedCrlfCheckout));
});

test("Phase 0 compatibility comparison rejects a deliberate interface perturbation", async () => {
  const perturbed = structuredClone(await runtimeSnapshot());
  perturbed.lite_package.bin = { "gkx-lite": "./bin/okf-lite.mjs" };
  assert.throws(() => assertCompatibilitySnapshot(perturbed));
});
