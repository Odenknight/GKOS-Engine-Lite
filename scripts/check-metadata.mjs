import { readFileSync } from "node:fs";
const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const lock = JSON.parse(readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"));
const readme = readFileSync(new URL("../README.md", import.meta.url), "utf8");
const versioning = readFileSync(new URL("../VERSIONING.md", import.meta.url), "utf8");
const declared = pkg.dependencies["gkos-engine"];
const match = declared.match(/#v(\d+\.\d+\.\d+)$/);
const expected = match?.[1];
const resolved = lock.packages?.["node_modules/gkos-engine"];
const problems = [];
if (pkg.license !== "Apache-2.0") problems.push("package.json must declare Apache-2.0");
if (lock.packages?.[""]?.license !== "Apache-2.0") problems.push("root lockfile package must declare Apache-2.0");
if (!expected || pkg.version !== expected) problems.push("Lite version must match the pinned Engine tag");
if (resolved?.version !== expected) problems.push("resolved Engine version must match the declared tag");
if (!resolved?.resolved?.match(/#[0-9a-f]{40}$/)) problems.push("Engine lockfile resolution must end in an immutable SHA");
if (!readme.includes(`Engine v${expected}`) || !versioning.includes("engine-verbatim")) problems.push("README/VERSIONING must describe the active Engine pin");
if (!/## Attribution and license[\s\S]*Apache-2\.0/.test(readme)) problems.push("README must declare Apache-2.0");
if (problems.length) { console.error(problems.join("\n")); process.exit(1); }
console.log(`metadata consistent: Lite ${pkg.version}, Engine ${expected}, Apache-2.0`);
