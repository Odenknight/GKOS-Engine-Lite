import test from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { link, mkdir, mkdtemp, readFile, rm, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";

import { prepareDelegatedCommand } from "../bin/okf-lite.mjs";

const execFileAsync = promisify(execFile);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const liteBin = join(root, "bin", "okf-lite.mjs");
const engineBin = join(root, "node_modules", "gkos-engine", "bin", "gkx.mjs");
const { validatePhase3KbPath } = await import(pathToFileURL(engineBin).href);
const pack = join(root, "rust", "contracts", "gkos-ingest-validation-1.0.0-draft.1");
const cliFixture = JSON.parse(await readFile(join(pack, "cli-conformance-fixture.json"), "utf8"));
const storageFixture = JSON.parse(await readFile(join(pack, "storage-conformance-fixture.json"), "utf8"));

const trackedSections = [
  "invalid_argv_matrix",
  "presentation_matrix",
  "fixed_error_matrix",
  "search_routing_matrix",
  "local_kb_path.reject_inputs",
];
const consumed = Object.fromEntries(trackedSections.map((section) => [section, new Set()]));

function fixtureRows(section) {
  return section === "local_kb_path.reject_inputs"
    ? cliFixture.local_kb_path.reject_inputs
    : cliFixture[section];
}

function row(section, name) {
  const value = fixtureRows(section).find((candidate) => candidate.name === name);
  assert.ok(value, `${section}:${name}`);
  consumed[section].add(name);
  return value;
}

test.after(() => {
  for (const section of trackedSections) {
    const names = fixtureRows(section).map((item) => item.name);
    assert.equal(new Set(names).size, names.length, `${section} names must be unique`);
    assert.deepEqual([...consumed[section]].sort(), [...names].sort(), `${section} has unconsumed rows`);
  }
});

async function run(bin, argv, env) {
  try {
    const result = await execFileAsync(process.execPath, [bin, ...argv], {
      cwd: root,
      env,
      encoding: null,
      maxBuffer: 32 * 1024 * 1024,
      windowsHide: true,
    });
    return { code: 0, stdout: result.stdout, stderr: result.stderr };
  } catch (error) {
    return {
      code: typeof error.code === "number" ? error.code : error.code,
      stdout: error.stdout ?? Buffer.alloc(0),
      stderr: error.stderr ?? Buffer.alloc(0),
    };
  }
}

function normalizedSqlitePid(bytes) {
  return Buffer.from(bytes.toString("utf8").replace(
    /^(\(node:)\d+(\) ExperimentalWarning: SQLite is an experimental feature and might change at any time)(\r?)$/gm,
    "$1<PID>$2$3",
  ), "utf8");
}

function assertInvocationEqual(lite, engine, label) {
  assert.equal(lite.code, engine.code, `${label}: exit`);
  assert.deepEqual(lite.stdout, engine.stdout, `${label}: stdout`);
  assert.deepEqual(normalizedSqlitePid(lite.stderr), normalizedSqlitePid(engine.stderr), `${label}: stderr`);
}

function assertFrozenOutput(invoked, expected, label) {
  assert.equal(invoked.code, expected.exit_code, `${label}: exit`);
  assert.deepEqual(invoked.stdout, Buffer.from(expected.stdout ?? "", "utf8"), `${label}: stdout`);
  assert.deepEqual(invoked.stderr, Buffer.from(expected.stderr ?? "", "utf8"), `${label}: stderr`);
}

function isolatedEnv(vault) {
  return { ...process.env, XDG_CONFIG_HOME: join(vault, "isolated-config") };
}

async function writeFtsConfig(vault) {
  const path = join(vault, "isolated-config", "gkos", "gkos.toml");
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, "config_version = 1\n[retrieval]\nmode = \"fts\"\n", "utf8");
  return path;
}

async function materialize(vault, name) {
  const corpus = cliFixture.materializable_corpora[name];
  assert.ok(corpus, name);
  for (const file of corpus.files) {
    const path = join(vault, file.path);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, file.content, "utf8");
  }
  return corpus;
}

async function sourceBytes(vault, corpus) {
  return Promise.all(corpus.files.map(async (file) => [file.path, await readFile(join(vault, file.path))]));
}

function fixtureArgv(fixture, replacements) {
  return fixture.argv.map((value) => Object.hasOwn(replacements, value) ? replacements[value] : value);
}

function sha256(bytes) {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function referencedIndexResult(fixture) {
  const match = /^storage-conformance-fixture\.json#\/valid_envelopes\/index_results\/status=([^,]+),mode=(.+)$/u
    .exec(fixture.result_reference);
  assert.ok(match, fixture.result_reference);
  const result = storageFixture.valid_envelopes.index_results.find((candidate) => (
    candidate.status === match[1] && candidate.mode === match[2]
  ));
  assert.ok(result, fixture.result_reference);
  const bytes = Buffer.from(`${JSON.stringify(result, null, 2)}\n`, "utf8");
  assert.equal(sha256(bytes), fixture.expected_stdout_sha256, fixture.name);
  return result;
}

function fixtureEncodedString(fixture) {
  if (typeof fixture.input === "string") return fixture.input;
  assert.equal(fixture.encoding, "utf16_code_units");
  return String.fromCharCode(...fixture.code_units);
}

function routeRow(name, expectedRoute) {
  const selected = row("search_routing_matrix", name);
  assert.equal(selected.route, expectedRoute, `${name}: route`);
  assert.equal(selected.attempt_status_bytes_loaded, false, `${name}: attempt-status bytes`);
  assert.equal(selected.owner_bytes_loaded, false, `${name}: owner bytes`);
  return selected;
}

function assertRouteOutput(invoked, selected) {
  if (selected.exit_code === undefined) return;
  assert.equal(invoked.code, selected.exit_code, `${selected.name}: exit`);
  assert.deepEqual(invoked.stdout, Buffer.alloc(0), `${selected.name}: stdout`);
  assert.deepEqual(invoked.stderr, Buffer.from(selected.stderr, "utf8"), `${selected.name}: stderr`);
}

test("frozen Phase-3 argument and local-path matrices are exact Full/Lite differentials", async (t) => {
  const vault = await mkdtemp(join(tmpdir(), "gkos-lite-phase3-args-"));
  t.after(() => rm(vault, { recursive: true, force: true }));
  const env = isolatedEnv(vault);
  await materialize(vault, "valid");

  const helpEngine = await run(engineBin, cliFixture.help.argv, env);
  const helpLite = await run(liteBin, cliFixture.help.argv, env);
  assert.equal(helpEngine.code, cliFixture.help.exit_code);
  assert.deepEqual(helpEngine.stderr, Buffer.from(cliFixture.help.stderr, "utf8"));
  assert.equal(helpLite.code, cliFixture.help.exit_code);
  assert.deepEqual(helpLite.stderr, Buffer.from(cliFixture.help.stderr, "utf8"));
  const fullHelpLines = helpEngine.stdout.toString("utf8").split("\n");
  const liteHelpLines = helpLite.stdout.toString("utf8").split("\n");
  for (const line of cliFixture.help.required_exact_lines) {
    assert.ok(fullHelpLines.includes(line), `Full help line: ${line}`);
    const liteLine = line.replace(/^  gkx /u, "  okf-lite ");
    assert.ok(liteHelpLines.includes(liteLine), `Lite derived help line: ${liteLine}`);
  }
  for (const [label, output] of [["Full", helpEngine.stdout], ["Lite", helpLite.stdout]]) {
    for (const fragment of cliFixture.help.forbidden_fragments) {
      assert.equal(output.includes(Buffer.from(fragment, "utf8")), false, `${label} help fragment: ${fragment}`);
    }
  }
  assert.deepEqual(cliFixture.local_kb_path.non_windows_reject_inputs, ["C:/foreign-drive/vault"]);
  if (process.platform !== "win32") {
    for (const value of cliFixture.local_kb_path.non_windows_reject_inputs) {
      assert.throws(() => validatePhase3KbPath(value), /GKX_CLI_KB_PATH_INVALID/u, value);
    }
  }
  assert.deepEqual(cliFixture.local_kb_path.windows_forbidden_base_resolution, {
    base: "\\\\server\\share\\operator",
    relative: "vault",
  });
  if (process.platform === "win32") {
    const { base, relative } = cliFixture.local_kb_path.windows_forbidden_base_resolution;
    assert.throws(() => validatePhase3KbPath(relative, base), /GKX_CLI_KB_PATH_INVALID/u);
  }

  for (const fixture of cliFixture.invalid_argv_matrix) {
    const selected = row("invalid_argv_matrix", fixture.name);
    const argv = fixtureArgv(selected, { "<vault>": vault });
    const expected = row("fixed_error_matrix", selected.error);
    const engine = await run(engineBin, argv, env);
    const lite = await run(liteBin, argv, env);
    assertInvocationEqual(lite, engine, selected.name);
    assertFrozenOutput(lite, expected, selected.name);
  }

  for (const fixture of cliFixture.local_kb_path.reject_inputs) {
    const selected = row("local_kb_path.reject_inputs", fixture.name);
    const value = fixtureEncodedString(selected);
    assert.throws(() => validatePhase3KbPath(value), /GKX_CLI_KB_PATH_INVALID/u, selected.name);
    const delegated = prepareDelegatedCommand(["validate", "--kb-path", value]);
    assert.equal(delegated.allowed, true);
    assert.strictEqual(delegated.argv[2], value, `${selected.name}: original UTF-16 value`);
    if (selected.encoding === "utf16_code_units") continue;
    for (const command of ["validate", "index"]) {
      const argv = [command, "--kb-path", value];
      const expected = row("fixed_error_matrix", `${command}_${selected.name === "empty" ? "usage" : "kb_path"}`);
      const engine = await run(engineBin, argv, env);
      const lite = await run(liteBin, argv, env);
      assertInvocationEqual(lite, engine, `${selected.name}:${command}`);
      assertFrozenOutput(lite, expected, `${selected.name}:${command}`);
    }
  }
  assert.equal(await readFile(join(vault, "missing-sensitivity.md"), "utf8"),
    cliFixture.materializable_corpora.valid.files[0].content);
  await assert.rejects(readFile(join(vault, ".gkx")));
});

test("frozen Phase-3 validation and every index status are exact Full/Lite differentials", async (t) => {
  for (const corpusName of ["invalid", "valid"]) {
    const vault = await mkdtemp(join(tmpdir(), `gkos-lite-phase3-${corpusName}-`));
    t.after(() => rm(vault, { recursive: true, force: true }));
    const corpus = await materialize(vault, corpusName);
    const before = await sourceBytes(vault, corpus);
    const env = isolatedEnv(vault);
    for (const format of ["json", "text"]) {
      const selected = row("presentation_matrix", `validate_${format}_${corpusName}`);
      const argv = fixtureArgv(selected, { "<fixture-vault>": vault });
      const engine = await run(engineBin, argv, env);
      const lite = await run(liteBin, argv, env);
      assertInvocationEqual(lite, engine, selected.name);
      assert.equal(lite.code, selected.exit_code);
      assert.equal(sha256(lite.stdout), selected.expected_stdout_sha256);
      assert.deepEqual(lite.stderr, Buffer.from(selected.stderr, "utf8"));
      if (selected.expected_stdout !== undefined) {
        assert.deepEqual(lite.stdout, Buffer.from(selected.expected_stdout, "utf8"));
      }
    }
    assert.deepEqual(await sourceBytes(vault, corpus), before);
  }

  const cases = [
    ["index_published_strict", "valid"],
    ["index_published_non_strict", "valid"],
    ["index_published_with_rejections_non_strict", "invalid"],
    ["index_blocked_strict", "invalid"],
  ];
  for (const [name, corpusName] of cases) {
    const selected = row("presentation_matrix", name);
    const fixtureResult = referencedIndexResult(selected);
    assert.equal(fixtureResult.status, selected.outcome);
    assert.equal(fixtureResult.mode, selected.mode);
    const vault = await mkdtemp(join(tmpdir(), `gkos-lite-phase3-${name}-`));
    t.after(() => rm(vault, { recursive: true, force: true }));
    const corpus = await materialize(vault, corpusName);
    const before = await sourceBytes(vault, corpus);
    await writeFtsConfig(vault);
    const env = isolatedEnv(vault);
    const argv = fixtureArgv(selected, { "<fixture-vault>": vault });
    const engine = await run(engineBin, argv, env);
    await rm(join(vault, ".gkx"), { recursive: true, force: true });
    const lite = await run(liteBin, argv, env);
    assertInvocationEqual(lite, engine, selected.name);
    assert.equal(lite.code, selected.exit_code);
    const result = JSON.parse(lite.stdout.toString("utf8"));
    assert.equal(result.status, selected.outcome);
    assert.equal(result.mode, selected.mode);
    assert.deepEqual(lite.stderr, Buffer.from(selected.stderr, "utf8"));
    assert.deepEqual(await sourceBytes(vault, corpus), before);
  }

  const parent = await mkdtemp(join(tmpdir(), "gkos-lite-phase3-operational-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const missing = join(parent, "missing-vault");
  const env = isolatedEnv(parent);
  for (const name of ["index_operational_non_strict", "index_operational_strict"]) {
    const selected = row("presentation_matrix", name);
    const expectedResult = referencedIndexResult(selected);
    const argv = fixtureArgv(selected, { "<missing-vault>": missing });
    const engine = await run(engineBin, argv, env);
    const lite = await run(liteBin, argv, env);
    assertInvocationEqual(lite, engine, selected.name);
    assert.equal(lite.code, selected.exit_code);
    assert.deepEqual(lite.stdout, Buffer.from(`${JSON.stringify(expectedResult, null, 2)}\n`, "utf8"));
    assert.deepEqual(lite.stderr, Buffer.from(selected.stderr, "utf8"));
  }
});

test("profile/operational errors and every search-routing row remain exact pass-throughs", async (t) => {
  const selectorRoot = await mkdtemp(join(tmpdir(), "gkos-lite-phase3-selector-"));
  t.after(() => rm(selectorRoot, { recursive: true, force: true }));
  const missing = join(selectorRoot, "missing");
  const malformed = join(selectorRoot, "malformed.toml");
  await writeFile(malformed,
    'contract_version = "gkos-frontmatter-profile/1.0.0-draft.1"\nprofile_id = ', "utf8");
  const env = isolatedEnv(selectorRoot);
  for (const command of ["validate", "index"]) {
    const expected = row("fixed_error_matrix", `${command}_profile`);
    const argv = [command, "--kb-path", missing, "--schema", malformed];
    const engine = await run(engineBin, argv, env);
    const lite = await run(liteBin, argv, env);
    assertInvocationEqual(lite, engine, `${command}:profile`);
    assertFrozenOutput(lite, expected, `${command}:profile`);
  }
  const profile = join(selectorRoot, "profile.toml");
  const profileAlias = join(selectorRoot, "profile-alias.toml");
  await writeFile(profile,
    'contract_version = "gkos-frontmatter-profile/1.0.0-draft.1"\nprofile_id = "operator"\n', "utf8");
  await link(profile, profileAlias);
  const validateOperational = row("fixed_error_matrix", "validate_operational");
  const engineOperational = await run(engineBin,
    ["validate", "--kb-path", missing, "--schema", profileAlias], env);
  const liteOperational = await run(liteBin,
    ["validate", "--kb-path", missing, "--schema", profileAlias], env);
  assertInvocationEqual(liteOperational, engineOperational, "validate:operational");
  assertFrozenOutput(liteOperational, validateOperational, "validate:operational");
  await unlink(profileAlias);

  const note = cliFixture.materializable_corpora.invalid.files[0];
  async function searchVault(prefix) {
    const vault = await mkdtemp(join(tmpdir(), prefix));
    t.after(() => rm(vault, { recursive: true, force: true }));
    await writeFile(join(vault, note.path), note.content, "utf8");
    const config = await writeFtsConfig(vault);
    return { vault, config, env: isolatedEnv(vault) };
  }
  const searchArgv = ({ vault, config }) => [
    "search", "VISIBLE_PROVIDER_SENTINEL", "--kb-path", vault, "--config", config, "--limit", "5",
  ];

  const legacy = await searchVault("gkos-lite-phase3-search-legacy-");
  const legacyRoute = routeRow("legacy_before_phase3", "legacy_compatible_index_then_search");
  const legacyArgs = searchArgv(legacy);
  const legacyEngine = await run(engineBin, legacyArgs, legacy.env);
  await rm(join(legacy.vault, ".gkx"), { recursive: true, force: true });
  const legacyLite = await run(liteBin, legacyArgs, legacy.env);
  assertInvocationEqual(legacyLite, legacyEngine, "legacy_before_phase3");
  assertRouteOutput(legacyLite, legacyRoute);

  const prior = await searchVault("gkos-lite-phase3-search-prior-");
  const priorRoute = routeRow("attempt_status_with_legacy_prior", "metadata_only_existing_legacy_read");
  const priorArgs = searchArgv(prior);
  assert.equal((await run(engineBin, priorArgs, prior.env)).code, 0);
  const invalid = cliFixture.materializable_corpora.invalid.files[1];
  await writeFile(join(prior.vault, invalid.path), invalid.content, "utf8");
  assert.equal((await run(engineBin, ["index", "--kb-path", prior.vault, "--strict"], prior.env)).code, 1);
  const priorEngine = await run(engineBin, priorArgs, prior.env);
  const priorLite = await run(liteBin, priorArgs, prior.env);
  assertInvocationEqual(priorLite, priorEngine, "attempt_status_with_legacy_prior");
  assertRouteOutput(priorLite, priorRoute);

  const statusPath = join(prior.vault, ".gkx", "derived", "retrieval", "ingest-attempt-status.json");
  const statusAlias = join(prior.vault, "status-alias");
  await link(statusPath, statusAlias);
  const aliasEngine = await run(engineBin, priorArgs, prior.env);
  const aliasLite = await run(liteBin, priorArgs, prior.env);
  const aliasRoute = routeRow("aliased_or_case_noncanonical_evidence", "fail_closed");
  assertInvocationEqual(aliasLite, aliasEngine, "aliased_or_case_noncanonical_evidence");
  const searchAuthority = row("fixed_error_matrix", "search_authority");
  assertFrozenOutput(aliasLite, searchAuthority, "aliased_or_case_noncanonical_evidence");
  assertRouteOutput(aliasLite, aliasRoute);
  await unlink(statusAlias);

  const noPrior = await mkdtemp(join(tmpdir(), "gkos-lite-phase3-search-no-prior-"));
  t.after(() => rm(noPrior, { recursive: true, force: true }));
  await writeFile(join(noPrior, invalid.path), invalid.content, "utf8");
  const noPriorEnv = isolatedEnv(noPrior);
  assert.equal((await run(engineBin, ["index", "--kb-path", noPrior, "--strict"], noPriorEnv)).code, 1);
  const noPriorArgs = ["search", "anything", "--kb-path", noPrior];
  const noPriorEngine = await run(engineBin, noPriorArgs, noPriorEnv);
  const noPriorLite = await run(liteBin, noPriorArgs, noPriorEnv);
  const noPriorRoute = routeRow("attempt_status_without_prior", "metadata_only_unavailable");
  assertInvocationEqual(noPriorLite, noPriorEngine, "attempt_status_without_prior");
  assertFrozenOutput(noPriorLite, searchAuthority, "attempt_status_without_prior");
  assertRouteOutput(noPriorLite, noPriorRoute);

  const active = await searchVault("gkos-lite-phase3-search-active-");
  const activeRoute = routeRow("active_phase3", "public_safe_inner_only");
  assert.equal((await run(engineBin, ["index", "--kb-path", active.vault, "--strict"], active.env)).code, 0);
  const activeArgs = searchArgv(active);
  const activeEngine = await run(engineBin, activeArgs, active.env);
  const activeLite = await run(liteBin, activeArgs, active.env);
  assertInvocationEqual(activeLite, activeEngine, "active_phase3");
  assertRouteOutput(activeLite, activeRoute);
});
