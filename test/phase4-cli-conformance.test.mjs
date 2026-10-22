import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmod,
  copyFile,
  mkdtemp,
  readFile,
  readdir,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, parse, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { prepareDelegatedCommand, validateLiteCommand } from "../bin/okf-lite.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LITE_BIN = join(ROOT, "bin/okf-lite.mjs");
const ENGINE_BIN = join(ROOT, "node_modules/gkos-engine/bin/gkx.mjs");
const PACK = join(ROOT, "rust/contracts/gkos-retrieval-evaluation-1.0.0-draft.1");
const FIXTURE_PATH = join(ROOT, "test/fixtures/retrieval-evaluation-cli-phase4.json");
const FIXTURE_BYTES = await readFile(FIXTURE_PATH);
const FIXTURE = JSON.parse(FIXTURE_BYTES.toString("utf8"));

const REVIEWED_INPUT_FILES = Object.freeze([
  "golden-fixture.toml",
  "conformance-fixture.json",
  "fixed-provider.json",
  "fixture-catalog.json",
  "source-corpus.json",
  "ndcg-discount-table.json",
  "metric-computation-fixture.json",
  "tune-priority-fixture.json",
  "reviewed-bundle.json",
]);

const EXPECTED_CASE_IDS = Object.freeze({
  argv_matrix: [
    "eval", "eval-json", "tune", "empty", "unknown-command", "eval-missing-fixture",
    "eval-missing-value", "eval-duplicate-json", "eval-output-forbidden", "tune-missing-output",
    "tune-json-forbidden", "duplicate-fixture", "duplicate-output", "dash-value",
    "retrieval-help-unapproved", "retrieval-short-help-unapproved", "eval-short-help-unapproved",
    "tune-short-help-unapproved",
  ],
  help_matrix: ["eval-help", "tune-help"],
  local_path_reject_matrix: [
    "empty", "url", "file-uri", "unc", "extended-device", "nt-device", "drive-relative", "ads",
    "reserved", "trailing-dot", "trailing-space", "double-slash", "nul", "lone-high-surrogate",
    "filesystem-root",
  ],
  error_matrix: [
    "invalid-arguments-eval", "invalid-arguments-tune", "invalid-fixture", "output-exists",
    "operational-eval", "operational-tune",
  ],
  general_execution_matrix: [
    "reviewed-active-eval", "reviewed-active-tune", "general-active-eval",
    "general-base-configuration-mismatch-eval", "general-base-configuration-mismatch-tune",
    "general-baseline-environment-splice", "general-eval-axes-mismatch", "general-disabled-eval",
    "general-reranker-only-eval", "general-embedding-only-eval",
    "general-embedding-query-failure-eval", "general-reranker-failure-eval",
    "general-disabled-tune", "general-reranker-only-tune",
  ],
  optional_companion_matrix: [
    "reviewed-object-missing", "reviewed-null-present", "reviewed-null-appeared",
    "provider-object-missing", "provider-null-present", "provider-null-appeared",
  ],
  recovery_state_matrix: [
    "fresh", "guard-stage-partial", "guard-stage-valid", "guard-stage-linked",
    "guard-stage-eexist-race", "guard-stage-ambiguous-link-race", "guard-stage-bad-digest",
    "guard-stage-coordinate-mismatch", "guard-only", "precommit", "partial-precommit", "linked",
    "finalize-only", "ordinary-final", "orphan-temporary", "guard-stage-third-link", "third-link",
    "guard-coordinate-mismatch",
  ],
  guard_mutation_matrix: [
    "owner-nonce-invalid", "guard-digest-invalid", "output-basename-substitution",
    "parent-inode-substitution", "selection-digest-substitution", "candidate-digest-substitution",
    "guard-noncanonical", "final-content-substitution",
  ],
  candidate_toml_matrix: [
    "mmr-disabled", "lambda-zero", "lambda-point-three", "lambda-point-five", "lambda-point-seven",
    "lambda-one",
  ],
});

function sha256(bytes) {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function stableJson(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
}

function exactError(row) {
  return `gkx retrieval ${row.command}: ${row.message}\n`;
}

function encodedPath(row) {
  if (row.encoding === "literal") return row.value;
  if (row.encoding === "utf16_code_units") return String.fromCharCode(...row.code_units);
  if (row.encoding === "filesystem_root") return parse(ROOT).root;
  throw new Error(`unknown path encoding: ${row.encoding}`);
}

function run(bin, args, options = {}) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(process.execPath, [bin, ...args], {
      cwd: options.cwd ?? ROOT,
      env: options.env ?? process.env,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (part) => stdout.push(part));
    child.stderr.on("data", (part) => stderr.push(part));
    child.once("error", rejectPromise);
    child.once("close", (code, signal) => resolvePromise({
      code,
      signal,
      stdout: Buffer.concat(stdout),
      stderr: Buffer.concat(stderr),
    }));
  });
}

function normalizeSqliteWarningPid(bytes) {
  return Buffer.from(bytes.toString("utf8").replace(
    /^(\(node:)\d+(\) ExperimentalWarning: SQLite is an experimental feature and might change at any time)(\r?)$/gmu,
    "$1<PID>$2$3",
  ), "utf8");
}

function assertInvocationEqual(lite, engine, label) {
  assert.equal(lite.code, engine.code,
    `${label}:exit (engine stderr=${JSON.stringify(engine.stderr.toString("utf8"))}, ` +
    `lite stderr=${JSON.stringify(lite.stderr.toString("utf8"))})`);
  assert.equal(lite.signal, engine.signal, `${label}:signal`);
  assert.deepEqual(lite.stdout, engine.stdout, `${label}:stdout`);
  assert.deepEqual(normalizeSqliteWarningPid(lite.stderr), normalizeSqliteWarningPid(engine.stderr), `${label}:stderr`);
}

async function privateDirectory(t, prefix) {
  const directory = await mkdtemp(join(tmpdir(), prefix));
  if (process.platform !== "win32") await chmod(directory, 0o700);
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

async function materializeReviewedFixture(t, prefix) {
  const root = await privateDirectory(t, prefix);
  for (const name of REVIEWED_INPUT_FILES) {
    await copyFile(join(PACK, name), join(root, name));
    if (process.platform !== "win32") await chmod(join(root, name), 0o600);
  }
  if (process.platform !== "win32") await chmod(root, 0o700);
  return { root, golden: join(root, "golden-fixture.toml") };
}

function isolatedTempEnvironment(parent) {
  return { ...process.env, TMP: parent, TEMP: parent, TMPDIR: parent };
}

async function inputDigests(root) {
  return new Map(await Promise.all(REVIEWED_INPUT_FILES.map(async (name) => [
    name,
    sha256(await readFile(join(root, name))),
  ])));
}

test("copied Full Slice-B CLI fixture has exact bytes, digest, and exhaustive case sets", () => {
  assert.equal(FIXTURE_BYTES.length, 23_770);
  assert.equal(sha256(FIXTURE_BYTES), "sha256:fce5308d252d9e693244250543f6642af1cc4a7ef9404ac604313f6f37f107be");
  assert.equal(FIXTURE.contract_version, "gkx-retrieval-evaluation-cli-conformance/1.0.0-draft.1");
  const { fixture_digest: fixtureDigest, ...material } = FIXTURE;
  assert.equal(sha256(Buffer.from(stableJson(material), "utf8")), fixtureDigest);
  assert.equal(fixtureDigest, "sha256:958c06ed5b2d063e6b9530261ed74fd17bba5e599d6326aafe5bc7f1ac6c0ff6");
  assert.deepEqual(Object.keys(FIXTURE), [
    "contract_version", "argv_matrix", "help_matrix", "local_path_reject_matrix", "presentation",
    "error_matrix", "general_execution_matrix", "optional_companion_matrix", "tune_selection",
    "recovery_state_matrix", "guard_mutation_matrix", "candidate_toml_matrix", "fixture_digest",
  ]);
  for (const [section, ids] of Object.entries(EXPECTED_CASE_IDS)) {
    assert.deepEqual(FIXTURE[section].map((row) => row.case_id), ids, section);
    assert.equal(new Set(ids).size, ids.length, `${section}: unique case IDs`);
  }
  assert.deepEqual(Object.keys(FIXTURE.presentation), [
    "usage", "eval_text", "eval_json", "eval_regression_status", "eval_needs_human_status",
    "tune_proposed", "tune_no_candidate", "tune_needs_human", "candidate_toml",
  ]);
  assert.deepEqual(Object.keys(FIXTURE.tune_selection), [
    "evaluated_candidate_count", "excluded_candidate_count", "query_evaluation_count",
    "conforming_candidate_count", "query_count", "maximum_expected_top_k", "environment_set_digest",
    "golden_digest", "base_configuration_digest", "tuning_grid_digest", "baseline_metrics_set_digest",
    "baseline_evaluation_digest", "baseline_aggregate_metrics_digest", "candidate_evaluation_set_digest",
    "candidate_config_digest", "candidate_evaluation_digest", "tune_selection_digest",
  ]);
});

test("every frozen retrieval argv/path/status row crosses Lite as the original argv object", () => {
  for (const row of [...FIXTURE.argv_matrix, ...FIXTURE.help_matrix]) {
    const argv = ["retrieval", ...row.args];
    const delegated = prepareDelegatedCommand(argv);
    assert.deepEqual(validateLiteCommand(argv), { allowed: true }, row.case_id);
    assert.strictEqual(delegated.argv, argv, row.case_id);
  }
  for (const row of FIXTURE.local_path_reject_matrix) {
    const value = encodedPath(row);
    const argv = ["retrieval", "eval", "--fixture", value];
    const delegated = prepareDelegatedCommand(argv);
    assert.strictEqual(delegated.argv, argv, row.case_id);
    assert.strictEqual(delegated.argv[3], value, row.case_id);
  }
  for (const row of FIXTURE.general_execution_matrix) {
    const argv = row.operation === "tune"
      ? ["retrieval", "tune", "--fixture", "golden-fixture.toml", "--output", "candidate.toml"]
      : ["retrieval", "eval", "--fixture", "golden-fixture.toml"];
    assert.strictEqual(prepareDelegatedCommand(argv).argv, argv, row.case_id);
  }
  for (const row of FIXTURE.error_matrix) {
    assert.equal(exactError(row), `gkx retrieval ${row.command}: ${row.message}\n`, row.case_id);
  }
  for (const row of FIXTURE.candidate_toml_matrix) {
    assert.equal(row.expected_toml.endsWith("\n"), true, row.case_id);
    assert.equal(row.expected_toml.endsWith("\n\n"), false, row.case_id);
    assert.equal(row.expected_toml.includes("mmr_lambda"), row.mmr, row.case_id);
  }
});

test("frozen invalid argv, help, paths, and ordinary errors are exact Full/Lite differentials", { timeout: 120_000 }, async (t) => {
  for (const row of FIXTURE.argv_matrix.filter((item) => !item.valid)) {
    const argv = ["retrieval", ...row.args];
    const engine = await run(ENGINE_BIN, argv);
    const lite = await run(LITE_BIN, argv);
    assertInvocationEqual(lite, engine, row.case_id);
    assert.equal(lite.code, 2, row.case_id);
    assert.equal(lite.stdout.length, 0, row.case_id);
    assert.deepEqual(lite.stderr, Buffer.from(exactError({
      command: row.command,
      message: "invalid arguments",
    }), "utf8"), row.case_id);
  }
  for (const row of FIXTURE.help_matrix) {
    const argv = ["retrieval", ...row.args];
    const engine = await run(ENGINE_BIN, argv);
    const lite = await run(LITE_BIN, argv);
    assertInvocationEqual(lite, engine, row.case_id);
    assert.equal(lite.code, 0, row.case_id);
    assert.deepEqual(lite.stdout, Buffer.from(FIXTURE.presentation.usage, "utf8"), row.case_id);
    assert.equal(lite.stderr.length, 0, row.case_id);
  }
  for (const row of FIXTURE.local_path_reject_matrix.filter((item) => item.encoding !== "utf16_code_units")) {
    const argv = ["retrieval", "eval", "--fixture", encodedPath(row)];
    const engine = await run(ENGINE_BIN, argv);
    const lite = await run(LITE_BIN, argv);
    assertInvocationEqual(lite, engine, row.case_id);
    assert.equal(lite.code, 2, row.case_id);
    assert.equal(lite.stdout.length, 0, row.case_id);
  }

  const missing = join(await privateDirectory(t, "gkos-lite-eval-missing-"), "missing.toml");
  const invalidFixture = FIXTURE.error_matrix.find((row) => row.case_id === "invalid-fixture");
  const missingArgs = ["retrieval", "eval", "--fixture", missing];
  const missingEngine = await run(ENGINE_BIN, missingArgs);
  const missingLite = await run(LITE_BIN, missingArgs);
  assertInvocationEqual(missingLite, missingEngine, invalidFixture.case_id);
  assert.deepEqual(missingLite, {
    code: invalidFixture.exit_code,
    signal: null,
    stdout: Buffer.alloc(0),
    stderr: Buffer.from(exactError(invalidFixture), "utf8"),
  });

  const fixture = await materializeReviewedFixture(t, "gkos-lite-eval-output-exists-fixture-");
  const outputRoot = await privateDirectory(t, "gkos-lite-eval-output-exists-");
  const output = join(outputRoot, "candidate.toml");
  await writeFile(output, "occupied\n", { mode: 0o600 });
  const outputExists = FIXTURE.error_matrix.find((row) => row.case_id === "output-exists");
  const outputArgs = ["retrieval", "tune", "--fixture", fixture.golden, "--output", output];
  const outputEngine = await run(ENGINE_BIN, outputArgs);
  const outputLite = await run(LITE_BIN, outputArgs);
  assertInvocationEqual(outputLite, outputEngine, outputExists.case_id);
  assert.deepEqual(outputLite, {
    code: outputExists.exit_code,
    signal: null,
    stdout: Buffer.alloc(0),
    stderr: Buffer.from(exactError(outputExists), "utf8"),
  });
});

test("reviewed eval and exhaustive tune preserve exact Full bytes without touching inputs", { timeout: 420_000 }, async (t) => {
  // Keep the host temp root deliberately short so SQLite's transient
  // `-journal` spelling remains below the Win32 legacy path ceiling too.
  const fixture = await materializeReviewedFixture(t, "glef-");
  const before = await inputDigests(fixture.root);
  const tempRoot = await privateDirectory(t, "glet-");
  const env = isolatedTempEnvironment(tempRoot);

  const textArgs = ["retrieval", "eval", "--fixture", fixture.golden];
  const engineText = await run(ENGINE_BIN, textArgs, { env });
  const liteText = await run(LITE_BIN, textArgs, { env });
  assertInvocationEqual(liteText, engineText, "reviewed eval text");
  assert.deepEqual(liteText, {
    code: 0,
    signal: null,
    stdout: Buffer.from(FIXTURE.presentation.eval_text, "utf8"),
    stderr: Buffer.alloc(0),
  });

  const jsonArgs = [...textArgs, "--json"];
  const engineJson = await run(ENGINE_BIN, jsonArgs, { env });
  const liteJson = await run(LITE_BIN, jsonArgs, { env });
  assertInvocationEqual(liteJson, engineJson, "reviewed eval json");
  assert.deepEqual(liteJson, {
    code: 0,
    signal: null,
    stdout: Buffer.from(FIXTURE.presentation.eval_json, "utf8"),
    stderr: Buffer.alloc(0),
  });

  const outputRoot = await privateDirectory(t, "gleo-");
  const output = join(outputRoot, "candidate.toml");
  const tune = await run(LITE_BIN, [
    "retrieval", "tune", "--fixture", fixture.golden, "--output", output,
  ], { env });
  assert.deepEqual(tune, {
    code: 0,
    signal: null,
    stdout: Buffer.from(FIXTURE.presentation.tune_proposed, "utf8"),
    stderr: Buffer.alloc(0),
  });
  assert.deepEqual(await readFile(output), Buffer.from(FIXTURE.presentation.candidate_toml, "utf8"));
  assert.deepEqual(await readdir(outputRoot), ["candidate.toml"]);
  assert.equal((await stat(output)).isFile(), true);
  assert.deepEqual(await readdir(tempRoot), []);
  assert.deepEqual(await inputDigests(fixture.root), before);
});
