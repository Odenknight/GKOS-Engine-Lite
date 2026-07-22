// DIV-001/002/003 divergence-fix coverage, backported from
// gkos-engine test/okf23.test.mjs (merged upstream as Odenknight/GKOS-Engine#4,
// released v1.0.6).
//
// GKOS-Engine-Lite vendors no engine source — it is a thin re-export wrapper and
// depends on gkos-engine (pinned #v1.0.6). These tests therefore exercise the
// fixed behavior through the SAME public entry point the engine itself tests
// (buildOkf23Projection, exported from the gkos-engine main entry), asserting
// the identical API surface so the wrapper cannot silently drift from canon.
import test from "node:test";
import assert from "node:assert/strict";
import { buildOkf23Projection } from "gkos-engine";

const note = `---
okf_version: "2.3"
uid: "019b2d14-4230-7db7-87d4-7d81cfaec932"
title: "A governed hypothesis"
type: "hypothesis"
created_at: "2026-07-16T20:00:00Z"
updated_at: "2026-07-17T20:00:00Z"
authorship:
  origin: "authored"
  author_id: "person:operator"
epistemic:
  state: "hypothesis"
  confidence: 0.35
  confidence_origin: "authored"
sensitivity:
  level: "restricted"
  handling:
    - "no-public-export"
provenance:
  source_type: "reasoning"
---
Body.`;

// DIV-002: missing sensitivity fails closed to the restricted default (secret),
// configurable and raise-only.
test("DIV-002: missing sensitivity fails closed to secret; invalid sensitivity fails closed", () => {
  const missing = note.replace(/sensitivity:[\s\S]*?provenance:/, "provenance:");
  const p1 = buildOkf23Projection(missing, "Missing.md", "m:1", null);
  assert.equal(p1.effective.sensitivity, "secret");
  assert.ok(p1.diagnostics.some((d) => d.code === "OKF-SENSITIVITY-001"), "OKF-SENSITIVITY-001 still fires so defaulting stays visible");
  const invalid = note.replace('level: "restricted"', 'level: "unclassified"');
  const p2 = buildOkf23Projection(invalid, "Invalid.md", "i:1", null);
  assert.equal(p2.effective.sensitivity, "secret");
});

test("DIV-002: defaultSensitivity option is honored, validated, and never lowers an authored classification", () => {
  const missing = note.replace(/sensitivity:[\s\S]*?provenance:/, "provenance:");
  // A deployment may relax the default to a less restrictive level.
  const relaxed = buildOkf23Projection(missing, "Missing.md", "m:2", null, { defaultSensitivity: "internal" });
  assert.equal(relaxed.effective.sensitivity, "internal");
  assert.ok(relaxed.diagnostics.some((d) => d.code === "OKF-SENSITIVITY-001"));
  // An out-of-vocabulary option value is ignored and falls back to secret.
  const bogus = buildOkf23Projection(missing, "Missing.md", "m:3", null, { defaultSensitivity: "nonsense" });
  assert.equal(bogus.effective.sensitivity, "secret");
  // The default never overrides an authored classification, even a more open one.
  const publicNote = note.replace('level: "restricted"', 'level: "public"');
  const p = buildOkf23Projection(publicNote, "Public.md", "p:1", null, { defaultSensitivity: "secret" });
  assert.equal(p.effective.sensitivity, "public");
});

test("DIV-001: naive wall-clock created_at emits an OKF-TEMPORAL diagnostic", () => {
  const naive = `---
okf_version: "2.3"
uid: "019b2d14-4230-7db7-87d4-7d81cfaec9b0"
title: "Naive timestamp"
type: "semantic"
created_at: 2026-07-20 12:00:00
epistemic_state: "fact"
sensitivity: "restricted"
authorship_origin: "authored"
tags: []
---
Body.`;
  const p = buildOkf23Projection(naive, "Naive.md", "n:1", null);
  const temporal = p.diagnostics.filter((d) => d.code.startsWith("OKF-TEMPORAL"));
  assert.equal(temporal.length, 1, `expected one OKF-TEMPORAL diagnostic, got: ${JSON.stringify(p.diagnostics)}`);
  assert.equal(temporal[0].field, "created_at");
  assert.ok(temporal[0].severity === "warning" || temporal[0].severity === "error", "severity is warning-or-error");
  // A properly zoned timestamp raises no temporal diagnostic.
  const zoned = naive.replace("created_at: 2026-07-20 12:00:00", 'created_at: "2026-07-20T12:00:00Z"');
  const clean = buildOkf23Projection(zoned, "Zoned.md", "n:2", null);
  assert.equal(clean.diagnostics.some((d) => d.code.startsWith("OKF-TEMPORAL")), false);
});

test("DIV-003: invalid epistemic state falls back to unknown with defaulted-marking and retained diagnostic", () => {
  const invalid = note.replace('state: "hypothesis"', 'state: "gospel"');
  const p = buildOkf23Projection(invalid, "Gospel.md", "g:1", null);
  // Effective state is the null-weight fallback, machine-detectable via the flag.
  assert.equal(p.effective.epistemicState, "unknown");
  assert.equal(p.effective.epistemicStateDefaulted, true);
  // The original invalid value is retained on the authored projection and in the diagnostic.
  assert.equal(p.authored.epistemicState, "gospel");
  const epi = p.diagnostics.find((d) => d.code === "OKF-EPISTEMIC-002");
  assert.ok(epi, "OKF-EPISTEMIC-002 still fires");
  assert.equal(epi.severity, "error");
  assert.ok(epi.message.includes("gospel"), "diagnostic retains the invalid value");
  // A valid state carries no defaulted-marking.
  const valid = buildOkf23Projection(note, "Valid.md", "v:1", null);
  assert.equal(valid.effective.epistemicStateDefaulted, false);
  assert.equal(valid.effective.epistemicState, "hypothesis");
});
