import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import test from "node:test";

const execFileAsync = promisify(execFile);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const checker = await readFile(join(root, "scripts/check-metadata.mjs"), "utf8");

async function runFixture({ dependency, version, resolvedVersion, resolvedSha, documentedReference }) {
  const directory = await mkdtemp(join(tmpdir(), "gkos-lite-metadata-"));
  try {
    await mkdir(join(directory, "scripts"));
    await Promise.all([
      writeFile(join(directory, "scripts/check-metadata.mjs"), checker, "utf8"),
      writeFile(join(directory, "package.json"), `${JSON.stringify({
        name: "gkos-engine-lite",
        version,
        license: "Apache-2.0",
        dependencies: { "gkos-engine": dependency },
      }, null, 2)}\n`, "utf8"),
      writeFile(join(directory, "package-lock.json"), `${JSON.stringify({
        packages: {
          "": { license: "Apache-2.0" },
          "node_modules/gkos-engine": {
            version: resolvedVersion,
            resolved: `git+ssh://git@github.com/Odenknight/GKOS-Engine.git#${resolvedSha}`,
          },
        },
      }, null, 2)}\n`, "utf8"),
      writeFile(join(directory, "README.md"), `Engine ${documentedReference}, package ${version}\n\n## Attribution and license\n\nApache-2.0\n`, "utf8"),
      writeFile(join(directory, "VERSIONING.md"), "engine-verbatim\n", "utf8"),
    ]);
    try {
      const result = await execFileAsync(process.execPath, [join(directory, "scripts/check-metadata.mjs")]);
      return { code: 0, stdout: result.stdout, stderr: result.stderr };
    } catch (error) {
      return { code: error.code ?? 1, stdout: error.stdout ?? "", stderr: error.stderr ?? "" };
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("metadata policy accepts an exact reviewed commit and locks it byte-for-byte", async () => {
  const sha = "bbc2ea874f4dde37e6376e46c080cb1c69ab1bb3";
  const result = await runFixture({
    dependency: `github:Odenknight/GKOS-Engine#${sha}`,
    version: "2.1.2",
    resolvedVersion: "2.1.2",
    resolvedSha: sha,
    documentedReference: sha,
  });
  assert.equal(result.code, 0, result.stderr);
});

test("metadata policy accepts a reviewed semver tag only with matching versions and immutable resolution", async () => {
  const result = await runFixture({
    dependency: "github:Odenknight/GKOS-Engine#v9.8.7",
    version: "9.8.7",
    resolvedVersion: "9.8.7",
    resolvedSha: "1111111111111111111111111111111111111111",
    documentedReference: "#v9.8.7",
  });
  assert.equal(result.code, 0, result.stderr);
});

test("metadata policy rejects mutable branches and commit/lock mismatches", async () => {
  const mutable = await runFixture({
    dependency: "github:Odenknight/GKOS-Engine#main",
    version: "2.1.2",
    resolvedVersion: "2.1.2",
    resolvedSha: "1111111111111111111111111111111111111111",
    documentedReference: "#main",
  });
  assert.notEqual(mutable.code, 0);

  const mismatch = await runFixture({
    dependency: "github:Odenknight/GKOS-Engine#2222222222222222222222222222222222222222",
    version: "2.1.2",
    resolvedVersion: "2.1.2",
    resolvedSha: "1111111111111111111111111111111111111111",
    documentedReference: "2222222222222222222222222222222222222222",
  });
  assert.notEqual(mismatch.code, 0);
});
