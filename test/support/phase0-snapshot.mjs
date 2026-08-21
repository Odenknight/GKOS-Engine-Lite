import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import * as engine from "gkos-engine";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

const NOTE_V1 = `---
okf_version: "2.3"
uid: "019b2d14-4230-7db7-87d4-7d81cfaec901"
title: "Policy v1"
type: "policy"
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T12:00:00Z"
epistemic_state: "accepted"
sensitivity: "internal"
authorship_origin: "authored"
tags: ["policy"]
supersedes: []
superseded_by: ["019b2d14-4230-7db7-87d4-7d81cfaec902"]
---
# Policy v1

First policy. See [[Policy v2]].
`;

const NOTE_V2 = `---
okf_version: "2.3"
uid: "019b2d14-4230-7db7-87d4-7d81cfaec902"
title: "Policy v2"
type: "policy"
created_at: "2026-01-02T00:00:00Z"
updated_at: "2026-01-02T12:00:00Z"
epistemic_state: "accepted"
sensitivity: "internal"
authorship_origin: "authored"
tags: ["policy"]
supersedes: ["019b2d14-4230-7db7-87d4-7d81cfaec901"]
superseded_by: []
---
# Policy v2

Second policy.
`;

export const PHASE0_FILES = Object.freeze([
  Object.freeze({
    relativePath: "policy/v1.md",
    name: "v1.md",
    extension: "md",
    size: Buffer.byteLength(NOTE_V1),
    modifiedTime: Date.parse("2026-01-01T12:00:00Z"),
    createdTime: Date.parse("2026-01-01T00:00:00Z"),
    content: NOTE_V1,
    kind: "note",
  }),
  Object.freeze({
    relativePath: "policy/v2.md",
    name: "v2.md",
    extension: "md",
    size: Buffer.byteLength(NOTE_V2),
    modifiedTime: Date.parse("2026-01-02T12:00:00Z"),
    createdTime: Date.parse("2026-01-02T00:00:00Z"),
    content: NOTE_V2,
    kind: "note",
  }),
]);

function withoutHostTiming(graph) {
  const stable = JSON.parse(JSON.stringify(graph));
  delete stable.stats.indexedAt;
  delete stable.stats.durationMs;
  delete stable.diagnostics.lastFullBuildMs;
  delete stable.diagnostics.lastIncrementalUpdateMs;
  return stable;
}

export function deterministicArtifacts() {
  const index = new engine.GkxIndex({ defaultSensitivity: "secret" });
  const { graph } = index.setFiles(PHASE0_FILES, ["policy"], []);
  const stableGraph = withoutHostTiming(graph);
  const contents = new Map(PHASE0_FILES.map((file) => [
    file.relativePath,
    engine.stripFrontmatter(file.content),
  ]));
  const episodes = engine.buildGraphitiEpisodesWithContent(stableGraph, contents, {
    vault: "phase0-vault",
    vaultIdentity: "vault:phase0",
    groupId: "phase0",
    processingTime: "2026-01-03T00:00:00.000Z",
  });
  return {
    graphBytes: Buffer.from(`${JSON.stringify(stableGraph, null, 2)}\n`, "utf8"),
    graphitiBytes: Buffer.from(`${JSON.stringify(episodes, null, 2)}\n`, "utf8"),
  };
}

export async function runtimeSnapshot() {
  const pkg = JSON.parse(await readFile(resolve(root, "package.json"), "utf8"));
  const lock = JSON.parse(await readFile(resolve(root, "package-lock.json"), "utf8"));
  const resolvedEngine = lock.packages["node_modules/gkos-engine"];
  return {
    lite_package: {
      name: pkg.name,
      version: pkg.version,
      bin: pkg.bin,
      files: pkg.files,
      engines: pkg.engines,
      engine_dependency: pkg.dependencies["gkos-engine"],
      engine_resolved_version: resolvedEngine.version,
      engine_resolved_sha: resolvedEngine.resolved.split("#").at(-1),
      engine_license: resolvedEngine.license,
    },
    engine_public_exports: Object.keys(engine).sort(),
    navigation_capabilities: typeof engine.getNavigationCapabilities === "function"
      ? engine.getNavigationCapabilities()
      : { available: false, reason: "not-exported-by-pinned-engine" },
  };
}
