import { test } from "node:test";
import assert from "node:assert/strict";
import { prepareDelegatedCommand, validateLiteCommand } from "../bin/okf-lite.mjs";

test("allows the eight delegated Lite command paths", () => {
  for (const argv of [
    ["validate", "."],
    ["index", "--kb-path", ".", "--strict", "--schema", "gkos:frontmatter-profile/current"],
    ["assess", ".", "--json"],
    ["search", "canonical policy", "--kb-path", ".", "--limit", "5"],
    ["retrieval", "eval", "--fixture", "golden-fixture.toml", "--json"],
    ["retrieval", "tune", "--fixture", "golden-fixture.toml", "--output", "candidate.toml"],
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

test("preserves Phase-3 validate and index arguments byte-for-byte without interpreting schema", () => {
  for (const argv of [
    ["validate", "--kb-path", "vault", "--schema", "profiles/x-tight.toml", "--format", "json"],
    ["index", "--kb-path", "vault", "--schema", "gkos:frontmatter-profile/current"],
  ]) {
    const prepared = prepareDelegatedCommand(argv);
    assert.equal(prepared.allowed, true);
    assert.strictEqual(prepared.argv, argv, "the wrapper must delegate the original argv object");
    assert.deepEqual(prepared.argv, argv);
  }
});

test("preserves the pinned search as-of flag and value byte-for-byte at the delegation boundary", () => {
  const asOf = "2026-07-15T00:00-04:00";
  const argv = ["search", "historical policy", "--kb-path", ".", "--as-of", asOf, "--limit", "5"];
  const prepared = prepareDelegatedCommand(argv);

  assert.equal(prepared.allowed, true);
  assert.strictEqual(prepared.argv, argv, "the Lite boundary must pass the original argv object");
  assert.deepEqual(prepared.argv, [
    "search", "historical policy", "--kb-path", ".", "--as-of", asOf, "--limit", "5",
  ]);
  assert.equal(prepared.argv[prepared.argv.indexOf("--as-of") + 1], asOf);
});

test("preserves the pinned Full retrieval namespace and original argv object byte-for-byte", () => {
  for (const argv of [
    ["retrieval", "eval", "--fixture", "golden-fixture.toml", "--json"],
    ["retrieval", "tune", "--fixture", "golden-fixture.toml", "--output", "candidate.toml"],
    ["retrieval", "unknown"],
    ["retrieval"],
  ]) {
    const prepared = prepareDelegatedCommand(argv);
    assert.equal(prepared.allowed, true);
    assert.strictEqual(prepared.argv, argv, "the pinned Full parser must receive the original argv object");
  }
});
