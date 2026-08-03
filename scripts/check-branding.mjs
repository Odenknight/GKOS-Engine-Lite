import { readFileSync } from "node:fs";

const productFacingFiles = ["package.json", "README.md", "bin/okf-lite.mjs", "desktop/src-tauri/tauri.conf.json"];
const failures = [];

for (const path of productFacingFiles) {
  const text = readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    if (!line.includes("OKF+")) continue;
    if (/formerly|previously|legacy|compatib|historical|existing/i.test(line)) continue;
    failures.push(`${path}:${index + 1}: ${line.trim()}`);
  }
}

if (failures.length) {
  console.error("check-branding: product-facing copy must use GKX; OKF+ is permitted only in explicit compatibility or historical context");
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("check-branding: OK — product-facing copy uses GKX with bounded legacy compatibility references");
