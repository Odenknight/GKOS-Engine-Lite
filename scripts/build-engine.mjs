/**
 * postinstall: build dist/kosmos-core.mjs inside the installed gkos-engine
 * package.
 *
 * gkos-engine's package.json "files" field is ["dist/", "bin/", "src/"], and
 * dist/ is git-ignored upstream (it's a build artifact, not committed). When
 * npm installs gkos-engine as a git dependency it packs the repo according to
 * that "files" field, so the installed copy has src/ and bin/ but no dist/
 * and no scripts/build.mjs to make one. GKOS-Engine-Lite is a thin wrapper
 * around that installed engine (see bin/okf-lite.mjs), so it needs the bundle
 * to exist — this script reproduces gkos-engine's own build step (bundle
 * src/index.ts -> dist/kosmos-core.mjs with esbuild) against the installed
 * package, using the esbuild devDependency declared here.
 *
 * If a future gkos-engine release publishes dist/ directly (or changes its
 * "files" field), this step becomes a no-op fast-path.
 */
import esbuild from "esbuild";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const enginePkgRoot = resolve(root, "node_modules/gkos-engine");
const entry = resolve(enginePkgRoot, "src/index.ts");
const outFile = resolve(enginePkgRoot, "dist/kosmos-core.mjs");

if (existsSync(outFile)) {
  console.log("gkos-engine-lite: dist/kosmos-core.mjs already present, skipping build.");
  process.exit(0);
}

if (!existsSync(entry)) {
  console.error(`gkos-engine-lite: expected ${entry} — is gkos-engine installed?`);
  process.exit(1);
}

mkdirSync(resolve(enginePkgRoot, "dist"), { recursive: true });

try {
  const res = await esbuild.build({
    entryPoints: [entry],
    bundle: true,
    write: false,
    format: "esm",
    platform: "neutral",
    target: "es2020",
    minify: false,
    sourcemap: false,
    logLevel: "silent",
  });
  writeFileSync(outFile, res.outputFiles[0].text);
  console.log("gkos-engine-lite: built node_modules/gkos-engine/dist/kosmos-core.mjs");
} catch (e) {
  console.error("gkos-engine-lite: failed to build gkos-engine bundle:", e);
  process.exit(1);
}
