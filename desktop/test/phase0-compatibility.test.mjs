import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import assert from "node:assert/strict";

import { allSnippets, viewerAppUrl, viewerQuery } from "../dist-test/snippets.js";
import { defaultSettings } from "../dist-test/settings-schema.js";
import { DEFAULT_PORT, LOOPBACK_HOST } from "../dist-test/strings.js";

const desktopRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = JSON.parse(
  await readFile(resolve(desktopRoot, "test/fixtures/phase0-desktop.json"), "utf8"),
);

function interfaceFields(source, interfaceName) {
  const block = source.match(new RegExp(`export interface ${interfaceName}\\s*\\{([\\s\\S]*?)\\n\\}`));
  assert.ok(block, `interface ${interfaceName} must exist`);
  return [...block[1].matchAll(/^\s{2}([a-z_]+)(?:\?)?:/gm)]
    .map((match) => match[1])
    .sort();
}

async function desktopSnapshot() {
  const [pkg, tauri, statusSource, settingsSource] = await Promise.all([
    readFile(resolve(desktopRoot, "package.json"), "utf8").then(JSON.parse),
    readFile(resolve(desktopRoot, "src-tauri/tauri.conf.json"), "utf8").then(JSON.parse),
    readFile(resolve(desktopRoot, "src/status.ts"), "utf8"),
    readFile(resolve(desktopRoot, "src/settings-schema.ts"), "utf8"),
  ]);
  const info = { port: 4814, token: "phase0-fake-token" };
  return {
    desktop_package: {
      name: pkg.name,
      version: pkg.version,
      tauri_cli: pkg.devDependencies["@tauri-apps/cli"],
      typescript: pkg.devDependencies.typescript,
      vite: pkg.devDependencies.vite,
    },
    tauri_bundle: {
      product_name: tauri.productName,
      version: tauri.version,
      identifier: tauri.identifier,
      targets: tauri.bundle.targets,
      external_bin: tauri.bundle.externalBin,
      resources: tauri.bundle.resources,
    },
    service_boundary: {
      host: LOOPBACK_HOST,
      port: DEFAULT_PORT,
      status_fields: interfaceFields(statusSource, "StatusDoc"),
      settings_fields: interfaceFields(settingsSource, "Settings"),
      default_settings: defaultSettings(),
    },
    snippets: allSnippets(info),
    viewer_query: viewerQuery(info),
    viewer_app_url: viewerAppUrl(info),
  };
}

function assertDesktopSnapshot(actual) {
  const historical = structuredClone(fixture.runtime_snapshot);
  delete historical.service_boundary.mcp_server_name;
  delete historical.snippets;
  const current = structuredClone(actual);
  delete current.snippets;
  assert.deepEqual(current, historical);
}

test("Phase 0 desktop package, status, and settings remain stable after retiring phantom MCP snippets", async () => {
  assertDesktopSnapshot(await desktopSnapshot());
  assert.match(fixture.runtime_snapshot.snippets.claudeCode, /\/mcp/);
  assert.doesNotMatch(JSON.stringify((await desktopSnapshot()).snippets), /\/mcp(?:\\|"|\/)/i);
});

test("Phase 0 desktop fixture detects a deliberate loopback boundary perturbation", async () => {
  const perturbed = structuredClone(await desktopSnapshot());
  perturbed.service_boundary.host = "0.0.0.0";
  assert.throws(() => assertDesktopSnapshot(perturbed));
});
