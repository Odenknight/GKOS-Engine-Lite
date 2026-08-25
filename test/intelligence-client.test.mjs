import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { INTELLIGENCE_CONTRACT_VERSION as ENGINE_INTELLIGENCE_CONTRACT_VERSION } from "gkos-engine";
import {
  assistMain,
  intelligenceUrl,
  LITE_INTELLIGENCE_CONTRACT_VERSION,
  requestIntelligence,
  resolveTask,
} from "../bin/intelligence-client.mjs";

test("Lite and the exact Engine pin share the v1 intelligence contract", () => {
  assert.equal(LITE_INTELLIGENCE_CONTRACT_VERSION, "gkos.intelligence.v1");
  assert.equal(ENGINE_INTELLIGENCE_CONTRACT_VERSION, LITE_INTELLIGENCE_CONTRACT_VERSION);
});

test("friendly assistance names hide internal task vocabulary", () => {
  assert.deepEqual(
    Object.fromEntries([
      "explain", "improve", "repair", "find-links", "find-claims", "check-conflicts", "check-privacy",
    ].map((task) => [task, resolveTask(task)])),
    {
      explain: "diagnostic_explanation",
      improve: "documentation_improvement",
      repair: "metadata_repair",
      "find-links": "relationship",
      "find-claims": "claim_extraction",
      "check-conflicts": "contradiction",
      "check-privacy": "classification_raise",
    },
  );
});

test("missing assistance arguments produce friendly examples", async () => {
  await assert.rejects(assistMain([]), /assist explain[\s\S]*never changes your note/i);
});

test("sidecar URL is restricted to loopback", () => {
  assert.equal(intelligenceUrl({}).hostname, "127.0.0.1");
  assert.throws(() => intelligenceUrl({ GKOS_INTELLIGENCE_URL: "https://example.com" }), /loopback/);
});

test("validates sidecar proposals before returning them", async () => {
  const file = join(tmpdir(), `gkos-intelligence-${process.pid}.md`);
  await writeFile(file, "# Alpha\n");
  try {
    const result = await requestIntelligence({
      task: "documentation_improvement", file, targetId: "note:alpha",
      fetchImpl: async (_url, options) => {
        const request = JSON.parse(options.body);
        assert.equal(request.contractVersion, LITE_INTELLIGENCE_CONTRACT_VERSION);
        return new Response(JSON.stringify({
          contractVersion: request.contractVersion, requestId: request.requestId,
          proposals: [{
            contractVersion: request.contractVersion, proposalId: "proposal:test-001",
            proposalType: request.task, targetId: request.targetId,
            rationale: "Add a source reference for the central claim.", confidence: 0.8,
            evidenceRefs: ["note:alpha"], generator: { system: "test", programVersion: "1.0.0" },
          }],
        }), { status: 200, headers: { "content-type": "application/json" } });
      },
    });
    assert.equal(result.proposals.length, 1);
  } finally {
    await rm(file, { force: true });
  }
});

test("fails closed when sidecar returns an authoritative patch", async () => {
  const file = join(tmpdir(), `gkos-intelligence-unsafe-${process.pid}.md`);
  await writeFile(file, "# Alpha\n");
  try {
    await assert.rejects(requestIntelligence({
      task: "metadata_repair", file, targetId: "note:alpha",
      fetchImpl: async (_url, options) => {
        const request = JSON.parse(options.body);
        return new Response(JSON.stringify({
          contractVersion: request.contractVersion, requestId: request.requestId,
          proposals: [{
            contractVersion: request.contractVersion, proposalId: "proposal:test-unsafe",
            proposalType: request.task, targetId: request.targetId,
            proposedPatch: { approved: true }, rationale: "Unsafe test.", confidence: 1,
            evidenceRefs: [], generator: { system: "test", programVersion: "1.0.0" },
          }],
        }), { status: 200, headers: { "content-type": "application/json" } });
      },
    }), /no safe proposals/);
  } finally {
    await rm(file, { force: true });
  }
});
