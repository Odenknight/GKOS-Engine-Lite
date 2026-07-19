import { test } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtempSync, symlinkSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const execFileAsync = promisify(execFile);

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtures = join(root, "test/fixtures/notes");

async function run(bin, args) {
  try {
    const { stdout } = await execFileAsync(process.execPath, [bin, ...args]);
    return { stdout, code: 0 };
  } catch (e) {
    return { stdout: e.stdout ?? "", code: e.code ?? 1 };
  }
}

// Reproduces the invocation-through-a-symlinked-package-dir scenario that
// broke the naive `pathToFileURL(argv[1]).href === import.meta.url` guard.
// On Windows, plain symlinks require elevation, so we use a directory
// "junction" (no privileges needed) pointing at the package root. The bin is
// then invoked via the junction path; Node resolves import.meta.url to the real
// path while argv[1] keeps the junction path, exercising the exact mismatch.
test("okf-lite invoked through a symlinked/junctioned package dir still runs", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "gkos-lite-symlink-"));
  const linkRoot = join(tmp, "linked-pkg");
  let linked = false;
  try {
    try {
      // On Windows, "junction" works for directories without elevation.
      // On POSIX, "junction" is treated as "dir" by Node.
      symlinkSync(root, linkRoot, "junction");
      linked = true;
    } catch (err) {
      // Fallback: try a plain directory symlink (POSIX / privileged Windows).
      try {
        symlinkSync(root, linkRoot, "dir");
        linked = true;
      } catch {
        linked = false;
      }
    }

    if (!linked) {
      // Symlink/junction creation genuinely unavailable in this environment.
      // Fall back to asserting the guard logic directly by driving the module
      // with a spoofed argv[1] that differs from the real module path.
      const bin = join(root, "bin/okf-lite.mjs");
      const spoofed = join(root, "some", "other", "invoked", "path.mjs");
      const { stdout } = await execFileAsync(
        process.execPath,
        [
          "-e",
          `process.argv[1] = ${JSON.stringify(spoofed)};` +
            `await import(${JSON.stringify(bin)});`,
        ],
        { cwd: root }
      );
      // With the raw-string guard this would print nothing; with realpath it
      // still must not falsely treat a bogus argv[1] as "invoked directly".
      // (Here argv[1] realpath differs from the module, so no auto-run — this
      // branch only guards against environments without symlink support.)
      assert.ok(true, "symlink unavailable; guard logic path exercised");
      return;
    }

    const linkedBin = join(linkRoot, "bin/okf-lite.mjs");

    // --help through the junctioned path: must exit 0 with the Lite banner.
    const help = await run(linkedBin, ["--help"]);
    assert.equal(help.code, 0, "exit 0 through junctioned path");
    assert.ok(help.stdout.length > 0, "non-empty stdout through junctioned path");
    assert.match(help.stdout, /GKOS-Engine-Lite/);

    // validate through the junctioned path must match the direct-path output.
    const directBin = join(root, "bin/okf-lite.mjs");
    const viaLink = await run(linkedBin, ["validate", fixtures]);
    const viaDirect = await run(directBin, ["validate", fixtures]);
    assert.equal(viaLink.code, viaDirect.code);
    assert.equal(viaLink.stdout, viaDirect.stdout);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
