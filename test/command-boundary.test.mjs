import { test } from "node:test";
import assert from "node:assert/strict";
import { validateLiteCommand } from "../bin/okf-lite.mjs";

test("allows the four deterministic Lite command paths", () => {
  for (const argv of [
    ["validate", "."],
    ["assess", ".", "--json"],
    ["graph", ".", "-o", "graph.json"],
    ["export", "graphiti", ".", "--episodes", "episodes.json"],
  ]) assert.deepEqual(validateLiteCommand(argv), { allowed: true });
});

test("rejects unsupported and future upstream commands before delegation", () => {
  for (const argv of [["migrate", "."], ["serve", "."], ["proposals", "apply"], ["export", "unknown", "."]]) {
    const result = validateLiteCommand(argv);
    assert.equal(result.allowed, false);
    assert.match(result.message, /Lite exposes only/);
  }
});
