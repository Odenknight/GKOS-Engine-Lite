import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
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
const historicalFixture = JSON.parse(await readFile(resolve(fixtureRoot, "phase0-lite.json"), "utf8"));
const fixture = JSON.parse(await readFile(resolve(fixtureRoot, "phase1-lite.json"), "utf8"));
const phase2Fixture = JSON.parse(await readFile(resolve(fixtureRoot, "phase2-lite.json"), "utf8"));
const phase3Fixture = JSON.parse(await readFile(resolve(fixtureRoot, "phase3-lite.json"), "utf8"));

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
  const expected = structuredClone(fixture.runtime_snapshot);
  expected.lite_package.engine_dependency = phase3Fixture.runtime_migration.engine_dependency;
  expected.lite_package.engine_resolved_sha = phase3Fixture.runtime_migration.engine_resolved_sha;
  assert.deepEqual(actual, expected);
}

function changedPaths(oldValue, newValue, path = "$", output = []) {
  if (Object.is(oldValue, newValue)) return output;
  if (typeof oldValue !== typeof newValue || oldValue === null || newValue === null || typeof oldValue !== "object") {
    output.push(path);
    return output;
  }
  if (Array.isArray(oldValue) !== Array.isArray(newValue)) {
    output.push(path);
    return output;
  }
  if (Array.isArray(oldValue)) {
    for (let index = 0; index < Math.max(oldValue.length, newValue.length); index += 1) {
      changedPaths(oldValue[index], newValue[index], `${path}[${index}]`, output);
    }
    return output;
  }
  const keys = [...new Set([...Object.keys(oldValue), ...Object.keys(newValue)])].sort();
  for (const key of keys) changedPaths(oldValue[key], newValue[key], `${path}.${key}`, output);
  return output;
}

test("Phase 3 runtime migration changes only the exact Full pin coordinate", async () => {
  const actual = await runtimeSnapshot();
  assertCompatibilitySnapshot(actual);
  assert.deepEqual(
    changedPaths(fixture.runtime_snapshot, actual),
    phase3Fixture.runtime_migration.expected_changed_paths,
  );
});

test("Phase 1 delegated and local proposal-only boundaries match the compatibility fixture", () => {
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
  assert.deepEqual(validateLiteCommand(phase2Fixture.cli.delegated_as_of_argv), { allowed: true });
  assert.deepEqual(validateLiteCommand(phase3Fixture.cli.delegated_validate_argv), { allowed: true });
  assert.deepEqual(validateLiteCommand(phase3Fixture.cli.delegated_index_argv), { allowed: true });
});

test("Phase 3 CLI help adds only authorized ingest lines atop the derived Phase 2 help", async () => {
  const phase1Expected = await readFile(resolve(fixtureRoot, fixture.cli.help_stdout_file));
  const phase2Expected = Buffer.from(phase1Expected.toString("utf8").replace(
    phase2Fixture.cli.phase1_help_line,
    phase2Fixture.cli.phase2_help_lines,
  ), "utf8");
  const expected = Buffer.from(phase2Expected.toString("utf8").replace(
    phase3Fixture.cli.phase2_validate_help_line,
    phase3Fixture.cli.phase3_validate_help_lines,
  ), "utf8");
  assert.notDeepEqual(phase2Expected, phase1Expected, "the Phase 1 help fixture remains immutable");
  assert.notDeepEqual(expected, phase2Expected, "the Phase 2 help is a derived immutable baseline");
  const help = await runCli(["--help"]);
  assert.equal(help.code, fixture.cli.help_exit);
  assert.deepEqual(help.stdout, expected);
  assert.equal(help.stderr.length, 0);

  const noArgs = await runCli([]);
  assert.equal(noArgs.code, fixture.cli.no_args_exit);
  assert.deepEqual(noArgs.stdout, expected);
  assert.equal(noArgs.stderr.length, 0);
});

test("Phase 2 pin preserves the separately classified Phase 1 graph and Graphiti goldens", async () => {
  const actual = deterministicArtifacts();
  const [graph, graphiti] = await Promise.all([
    readFile(resolve(fixtureRoot, fixture.deterministic.graph_file)),
    readFile(resolve(fixtureRoot, fixture.deterministic.graphiti_file)),
  ]);
  assert.deepEqual(actual.graphBytes, graph);
  assert.deepEqual(actual.graphitiBytes, graphiti);
});

test("Phase 1 deterministic golden comparison rejects simulated CRLF checkout bytes", async () => {
  const actual = deterministicArtifacts().graphBytes;
  const golden = await readFile(resolve(fixtureRoot, fixture.deterministic.graph_file));
  const simulatedCrlfCheckout = Buffer.from(golden.toString("utf8").replace(/(?<!\r)\n/g, "\r\n"), "utf8");
  assert.notDeepEqual(simulatedCrlfCheckout, golden);
  assert.throws(() => assert.deepEqual(actual, simulatedCrlfCheckout));
});

test("Phase 1 compatibility comparison rejects a deliberate interface perturbation", async () => {
  const perturbed = structuredClone(await runtimeSnapshot());
  perturbed.lite_package.bin = { "gkx-lite": "./bin/okf-lite.mjs" };
  assert.throws(() => assertCompatibilitySnapshot(perturbed));
});

test("Phase 0 fixtures remain immutable and Phase 1 records exact authorized old-to-new deltas", async () => {
  assert.equal(historicalFixture.baseline_commit, "2ebbf77583af3e83032054f1256188dc56376907");
  assert.equal(historicalFixture.runtime_snapshot.lite_package.version, "1.1.3");
  assert.equal(historicalFixture.runtime_snapshot.lite_package.engine_resolved_sha, "72c4a3268c9db132f2f9dd5aaa7eb7075e6bab2a");
  assert.equal(fixture.full_reference.commit, "bbc2ea874f4dde37e6376e46c080cb1c69ab1bb3");
  assert.equal(fixture.deterministic.source_fixture_change.startsWith("none"), true);
  assert.equal(phase2Fixture.historical_fixture, "phase1-lite.json");
  assert.equal(phase2Fixture.full_reference.commit, "6e2df27d33ede62ee0d2e3cb7610df478a7d66ce");
  assert.equal(phase2Fixture.cli.search_result_contract, "gkos-retrieval/1.0.0-draft.2");
  assert.equal(phase2Fixture.deterministic.source_fixture_change.startsWith("none"), true);
  assert.equal(phase3Fixture.historical_fixture, "phase2-lite.json");
  assert.equal(phase3Fixture.full_reference.commit, "e7cc0dd478af3d0bda216c5258dec5f77932def7");
  assert.equal(phase3Fixture.full_reference.ingest_contract, "gkos-ingest-validation/1.0.0-draft.1");
  assert.equal(phase3Fixture.deterministic.source_fixture_change.startsWith("none"), true);

  const sha256 = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  const [oldGraph, newGraph, oldGraphiti, newGraphiti] = await Promise.all([
    readFile(resolve(fixtureRoot, fixture.deterministic.historical_graph_file)),
    readFile(resolve(fixtureRoot, fixture.deterministic.graph_file)),
    readFile(resolve(fixtureRoot, fixture.deterministic.historical_graphiti_file)),
    readFile(resolve(fixtureRoot, fixture.deterministic.graphiti_file)),
  ]);
  assert.equal(sha256(oldGraph), fixture.deterministic.historical_graph_sha256);
  assert.equal(sha256(newGraph), fixture.deterministic.graph_sha256);
  assert.equal(sha256(oldGraphiti), fixture.deterministic.historical_graphiti_sha256);
  assert.equal(sha256(newGraphiti), fixture.deterministic.graphiti_sha256);
  assert.notDeepEqual(newGraph, oldGraph);
  assert.notDeepEqual(newGraphiti, oldGraphiti);

  const oldGraphValue = JSON.parse(oldGraph);
  const newGraphValue = JSON.parse(newGraph);
  assert.deepEqual(changedPaths(oldGraphValue, newGraphValue), [
    "$.gkxAssessments",
    "$.gkxDiagnostics",
    "$.gkxProfile",
    "$.gkxUidIndex",
    "$.nodes[3].gkx",
    "$.nodes[3].okf",
    "$.nodes[4].gkx",
    "$.nodes[4].okf",
    "$.okfAssessments",
    "$.okfDiagnostics",
    "$.okfProfile",
    "$.okfUidIndex",
  ], "only canonical GKX namespace/profile projection paths may change");
  for (const [oldNode, newNode] of oldGraphValue.nodes.map((node, index) => [node, newGraphValue.nodes[index]])) {
    if (!oldNode.okf) continue;
    assert.deepEqual({
      uid: newNode.gkx.uid,
      path: newNode.path,
      type: newNode.gkx.type,
      title: newNode.gkx.title,
      epistemicState: newNode.gkx.epistemicState,
      sensitivity: newNode.gkx.sensitivity,
      supersedes: newNode.gkx.supersedes,
      supersededBy: newNode.gkx.supersededBy,
      relations: newNode.gkx.relations,
      effectiveRelationships: newNode.gkx.projection.effective.relationships,
      effectiveEvidence: newNode.gkx.projection.effective.evidence,
      createdAt: newNode.gkx.projection.authored.createdAt,
      updatedAt: newNode.gkx.projection.authored.updatedAt,
      validAt: newNode.gkx.validAt,
      invalidAt: newNode.gkx.invalidAt,
      head: newNode.gkx.head,
      scores: newNode.gkx.projection.assessment.scores,
      contentHash: newNode.gkx.projection.contentHash,
    }, {
      uid: oldNode.okf.uid,
      path: oldNode.path,
      type: oldNode.okf.type,
      title: oldNode.okf.title,
      epistemicState: oldNode.okf.epistemicState,
      sensitivity: oldNode.okf.sensitivity,
      supersedes: oldNode.okf.supersedes,
      supersededBy: oldNode.okf.supersededBy,
      relations: oldNode.okf.relations,
      effectiveRelationships: oldNode.okf.projection.effective.relationships,
      effectiveEvidence: oldNode.okf.projection.effective.evidence,
      createdAt: oldNode.okf.projection.authored.createdAt,
      updatedAt: oldNode.okf.projection.authored.updatedAt,
      validAt: oldNode.okf.validAt,
      invalidAt: oldNode.okf.invalidAt,
      head: oldNode.okf.head,
      scores: oldNode.okf.projection.assessment.scores,
      contentHash: oldNode.okf.projection.contentHash,
    });
    assert.equal(newNode.gkx.projection.mode, "legacy");
    assert.equal(newNode.gkx.projection.sourceVersion, null);
    assert.equal(newNode.gkx.projection.rawFrontmatter.okf_version, "2.3");
    assert.equal(newNode.gkx.projection.extensions.okf_version, "2.3");
    assert.ok(newNode.gkx.projection.diagnostics.some(({ code }) => code === "GKX-SCHEMA-003"));
  }

  const oldEpisodes = JSON.parse(oldGraphiti);
  const newEpisodes = JSON.parse(newGraphiti);
  assert.deepEqual(changedPaths(oldEpisodes, newEpisodes), [
    "$[0].episode_body", "$[0].episode_metadata.gkx_version", "$[0].episode_metadata.okf_version", "$[0].source_description",
    "$[1].episode_body", "$[1].episode_metadata.gkx_version", "$[1].episode_metadata.okf_version", "$[1].source_description",
    "$[2].episode_body", "$[2].episode_metadata.gkx_version", "$[2].episode_metadata.okf_version", "$[2].source_description",
    "$[3].episode_body", "$[3].episode_metadata.gkx_version", "$[3].episode_metadata.okf_version", "$[3].source_description",
  ], "Graphiti may change only its canonical namespace envelope and descriptions");
  for (let index = 0; index < oldEpisodes.length; index += 1) {
    const oldBody = JSON.parse(oldEpisodes[index].episode_body);
    const newBody = JSON.parse(newEpisodes[index].episode_body);
    assert.deepEqual({
      uuid: newEpisodes[index].uuid,
      name: newEpisodes[index].name,
      source: newEpisodes[index].source,
      reference_time: newEpisodes[index].reference_time,
      group_id: newEpisodes[index].group_id,
      vault_identity: newEpisodes[index].episode_metadata.vault_identity,
      source_path_hash: newEpisodes[index].episode_metadata.source_path_hash,
      uid: newEpisodes[index].episode_metadata.uid,
      note_type: newEpisodes[index].episode_metadata.note_type,
      sensitivity: newEpisodes[index].episode_metadata.sensitivity,
      event_time: newEpisodes[index].episode_metadata.event_time,
      processing_time: newEpisodes[index].episode_metadata.processing_time,
    }, {
      uuid: oldEpisodes[index].uuid,
      name: oldEpisodes[index].name,
      source: oldEpisodes[index].source,
      reference_time: oldEpisodes[index].reference_time,
      group_id: oldEpisodes[index].group_id,
      vault_identity: oldEpisodes[index].episode_metadata.vault_identity,
      source_path_hash: oldEpisodes[index].episode_metadata.source_path_hash,
      uid: oldEpisodes[index].episode_metadata.uid,
      note_type: oldEpisodes[index].episode_metadata.note_type,
      sensitivity: oldEpisodes[index].episode_metadata.sensitivity,
      event_time: oldEpisodes[index].episode_metadata.event_time,
      processing_time: oldEpisodes[index].episode_metadata.processing_time,
    });
    const invariant = (body) => ({
      uid: body.uid,
      subject_uid: body.subject_uid,
      path: body.path,
      title: body.title,
      subject: body.subject,
      predicate: body.predicate,
      object_ref: body.object_ref,
      type: body.type,
      tags: body.tags,
      labels: body.labels,
      authority: body.authority,
      relationships: body.governance?.relationships,
      sensitivity: body.governance?.effective_sensitivity,
      score: body.governance?.assessment?.overall,
      evidence: body.evidence,
      lineage: body.lineage,
      event_time: body.event_time,
      processing_time: body.processing_time,
      content_hash: body.integrity?.content_hash,
      content: body.content,
      content_char_count: body.content_char_count,
      content_truncated: body.content_truncated,
    });
    assert.deepEqual(invariant(newBody), invariant(oldBody));
  }
});
