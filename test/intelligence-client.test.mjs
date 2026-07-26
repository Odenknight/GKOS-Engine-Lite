import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { assistMain, intelligenceUrl, requestIntelligence, resolveTask } from "../bin/intelligence-client.mjs";

test("friendly assistance names hide internal task vocabulary", () => {
  assert.equal(resolveTask("explain"), "diagnostic_explanation");
  assert.equal(resolveTask("improve"), "documentation_improvement");
  assert.equal(resolveTask("check-privacy"), "classification_raise");
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
