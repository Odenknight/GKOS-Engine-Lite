// GkxIndex projection-options threading, exercised through the installed
// gkos-engine package. Upstream fix: parseSourceFile/buildGraph/
// GkxIndex accept and forward Gkx23ProjectionOptions (defaultSensitivity)
// — Odenknight/GKOS-Engine#7, closing Odenknight/GKOS-Engine#6.
//
// GKOS-Engine-Lite vendors no engine source; these tests assert the option flows
// from the GkxIndex constructor all the way into a parsed record's effective
// projection, so the wrapper cannot silently drift from canon.
import test from "node:test";
import assert from "node:assert/strict";
import { GkxIndex } from "gkos-engine";

// A GKX note with NO sensitivity block — effective sensitivity is decided by
// the fail-closed default (or the configured defaultSensitivity option).
const unlabeled = `---
gkx_version: "2.3"
uid: "019b2d14-4230-7db7-87d4-7d81cfaec111"
title: "Unlabeled note"
type: "semantic"
created_at: "2026-07-20T12:00:00Z"
updated_at: "2026-07-20T12:00:00Z"
epistemic:
  state: "fact"
  confidence: 0.9
  confidence_origin: "authored"
authorship:
  origin: "authored"
  author_id: "person:operator"
provenance:
  source_type: "reasoning"
---
Body.`;

// Extension is derived from the path (engine PARSEABLE keys carry no leading dot).
const file = { relativePath: "Unlabeled.md", content: unlabeled };

function effectiveSensitivity(index) {
  index.setFiles([file]);
  const rec = index.getRecords().get("Unlabeled.md");
  return rec.gkx.projection.effective.sensitivity;
}

test("GkxIndex threads defaultSensitivity into the effective projection", () => {
  const configured = new GkxIndex({ defaultSensitivity: "internal" });
  assert.equal(effectiveSensitivity(configured), "internal");
});

test("GkxIndex without options fails closed to secret for an unlabeled note", () => {
  const bare = new GkxIndex();
  assert.equal(effectiveSensitivity(bare), "secret");
});
