import { readFileSync } from "node:fs";
const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const lock = JSON.parse(readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"));
const readme = readFileSync(new URL("../README.md", import.meta.url), "utf8");
const versioning = readFileSync(new URL("../VERSIONING.md", import.meta.url), "utf8");
const declared = pkg.dependencies["gkos-engine"];
const commitMatch = declared.match(/#([0-9a-f]{40})$/);
const tagMatch = declared.match(/#v(\d+\.\d+\.\d+)$/);
const expectedSha = commitMatch?.[1];
const expectedVersion = tagMatch?.[1] ?? pkg.version;
const resolved = lock.packages?.["node_modules/gkos-engine"];
const resolvedSha = resolved?.resolved?.match(/#([0-9a-f]{40})$/)?.[1];
const problems = [];
if (pkg.license !== "Apache-2.0") problems.push("package.json must declare Apache-2.0");
if (lock.packages?.[""]?.license !== "Apache-2.0") problems.push("root lockfile package must declare Apache-2.0");
if (!commitMatch && !tagMatch) problems.push("Engine dependency must use a reviewed semver tag or exact 40-character commit SHA");
if (pkg.version !== expectedVersion || resolved?.version !== expectedVersion) problems.push("Lite version must match the selected Engine package version");
if (!resolvedSha) problems.push("Engine lockfile resolution must end in an immutable SHA");
if (expectedSha && resolvedSha !== expectedSha) problems.push("Engine lockfile resolution must match the declared immutable SHA");
if (resolved?.bin?.gkx !== "bin/gkx.mjs") problems.push("Engine lockfile must expose the reviewed 2.x gkx CLI at bin/gkx.mjs");
const documentedReference = expectedSha ?? `#v${expectedVersion}`;
if (!readme.includes(documentedReference) || !readme.includes(`package ${expectedVersion}`) || !versioning.includes("engine-verbatim")) {
  problems.push("README/VERSIONING must describe the active Engine commit and package version");
}
if (!/## Attribution and license[\s\S]*Apache-2\.0/.test(readme)) problems.push("README must declare Apache-2.0");
if (problems.length) { console.error(problems.join("\n")); process.exit(1); }
console.log(`metadata consistent: Lite ${pkg.version}, Engine package ${resolved.version} at ${expectedSha ?? `v${expectedVersion} (${resolvedSha})`}, Apache-2.0`);
