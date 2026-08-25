import assert from "node:assert/strict";
import test from "node:test";

import {
  checkInstalledEngine,
  engineCompatibilityProblems,
} from "../scripts/check-engine-compat.mjs";

const proposalTypes = [
  "diagnostic_explanation",
  "metadata_repair",
  "relationship",
  "classification_raise",
  "claim_extraction",
  "contradiction",
  "documentation_improvement",
];

function compatibleFixture() {
  return {
    litePackage: { version: "2.1.2" },
    lockEntry: { version: "2.1.2", bin: { gkx: "bin/gkx.mjs" } },
    enginePackage: { version: "2.1.2", bin: { gkx: "bin/gkx.mjs" } },
    engineApi: {
      buildGkx23Projection() {},
      validateIntelligenceResponse() {},
      INTELLIGENCE_CONTRACT_VERSION: "gkos.intelligence.v1",
      INTELLIGENCE_PROPOSAL_TYPES: proposalTypes,
    },
    engineCli: { main() {} },
  };
}

test("installed exact Engine pin exposes the Lite-required 2.x boundaries", async () => {
  assert.deepEqual(await checkInstalledEngine(), { version: "2.1.2", cli: "bin/gkx.mjs" });
});

test("compatibility guard rejects the old CLI and projection names", () => {
  const fixture = compatibleFixture();
  fixture.lockEntry.bin = { okf: "bin/okf.mjs" };
  fixture.enginePackage.bin = { okf: "bin/okf.mjs" };
  delete fixture.engineApi.buildGkx23Projection;
  fixture.engineApi.buildOkf23Projection = () => {};
  const problems = engineCompatibilityProblems(fixture);
  assert.ok(problems.some((problem) => /gkx CLI/.test(problem)));
  assert.ok(problems.some((problem) => /buildGkx23Projection/.test(problem)));
});

test("compatibility guard rejects assistance vocabulary drift", () => {
  const fixture = compatibleFixture();
  fixture.engineApi.INTELLIGENCE_PROPOSAL_TYPES = proposalTypes.slice(0, -1);
  assert.ok(engineCompatibilityProblems(fixture).some((problem) => /proposal vocabulary/.test(problem)));
});

test("compatibility guard rejects intelligence contract-version drift", () => {
  const fixture = compatibleFixture();
  fixture.engineApi.INTELLIGENCE_CONTRACT_VERSION = "gkos.intelligence.v2";
  assert.ok(engineCompatibilityProblems(fixture).some((problem) => /gkos\.intelligence\.v1/.test(problem)));
});
