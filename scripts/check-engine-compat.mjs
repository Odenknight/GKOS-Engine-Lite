#!/usr/bin/env node
/**
 * Verify the installed Engine exposes every exact boundary Lite imports.
 *
 * Version agreement alone is insufficient: Engine 2.x renamed both its CLI
 * and projection API. A future pin may keep the same package version shape yet
 * remove one of Lite's required entry points. This post-install guard turns
 * that drift into an actionable failure before wrapper tests execute.
 */
import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REQUIRED_PROPOSAL_TYPES = Object.freeze([
  "diagnostic_explanation",
  "metadata_repair",
  "relationship",
  "classification_raise",
  "claim_extraction",
  "contradiction",
  "documentation_improvement",
]);
const REQUIRED_INTELLIGENCE_CONTRACT_VERSION = "gkos.intelligence.v1";

export function engineCompatibilityProblems({ litePackage, lockEntry, enginePackage, engineApi, engineCli }) {
  const problems = [];
  const expectedVersion = litePackage?.version;
  if (lockEntry?.version !== expectedVersion) {
    problems.push("lockfile Engine version must match the Lite package version");
  }
  if (enginePackage?.version !== expectedVersion) {
    problems.push("installed Engine version must match the Lite package version");
  }
  if (lockEntry?.bin?.gkx !== "bin/gkx.mjs") {
    problems.push("lockfile must expose the Engine 2.x gkx CLI at bin/gkx.mjs");
  }
  if (enginePackage?.bin?.gkx !== "bin/gkx.mjs") {
    problems.push("installed Engine must expose the gkx CLI at bin/gkx.mjs");
  }
  if (typeof engineApi?.buildGkx23Projection !== "function") {
    problems.push("installed Engine must export buildGkx23Projection");
  }
  if (typeof engineApi?.validateIntelligenceResponse !== "function") {
    problems.push("installed Engine must export validateIntelligenceResponse");
  }
  if (engineApi?.INTELLIGENCE_CONTRACT_VERSION !== REQUIRED_INTELLIGENCE_CONTRACT_VERSION) {
    problems.push(`installed Engine intelligence contract must be ${REQUIRED_INTELLIGENCE_CONTRACT_VERSION}`);
  }
  if (JSON.stringify(engineApi?.INTELLIGENCE_PROPOSAL_TYPES) !== JSON.stringify(REQUIRED_PROPOSAL_TYPES)) {
    problems.push("installed Engine assistance proposal vocabulary is incompatible with Lite");
  }
  if (typeof engineCli?.main !== "function") {
    problems.push("installed Engine gkx CLI must export main(argv)");
  }
  return problems;
}

export async function checkInstalledEngine() {
  const require = createRequire(import.meta.url);
  const liteRoot = dirname(dirname(fileURLToPath(import.meta.url)));
  const litePackage = JSON.parse(await readFile(join(liteRoot, "package.json"), "utf8"));
  const lock = JSON.parse(await readFile(join(liteRoot, "package-lock.json"), "utf8"));
  const lockEntry = lock.packages?.["node_modules/gkos-engine"];
  const engineMain = require.resolve("gkos-engine");
  const engineRoot = dirname(dirname(engineMain));
  const enginePackage = JSON.parse(await readFile(join(engineRoot, "package.json"), "utf8"));
  const engineApi = await import("gkos-engine");
  const engineCli = await import(pathToFileURL(join(engineRoot, "bin/gkx.mjs")).href);
  const problems = engineCompatibilityProblems({ litePackage, lockEntry, enginePackage, engineApi, engineCli });
  if (problems.length) throw new Error(problems.join("\n"));
  return { version: enginePackage.version, cli: enginePackage.bin.gkx };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fileURLToPath(pathToFileURL(process.argv[1]))) {
  try {
    const result = await checkInstalledEngine();
    console.log(`Engine compatibility: ${result.version}, ${result.cli}, required API surface present`);
  } catch (error) {
    console.error(`check-engine-compat: FAIL\n${error.message}`);
    process.exit(1);
  }
}
