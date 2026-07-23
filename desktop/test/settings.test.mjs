import { test } from "node:test";
import assert from "node:assert/strict";
import {
  defaultSettings,
  isValidSensitivity,
  coerceSensitivity,
  isValidPort,
  coercePort,
  normalizeSettings,
  enableBlockedReason,
  canEnable,
} from "../dist-test/settings-schema.js";
import { SENSITIVITY_LEVELS, DEFAULT_SENSITIVITY, DEFAULT_PORT } from "../dist-test/strings.js";

test("seven-level vocabulary, in most-open → most-private order", () => {
  assert.deepEqual([...SENSITIVITY_LEVELS], [
    "public", "internal", "restricted", "confidential", "regulated", "phi", "secret",
  ]);
  assert.equal(SENSITIVITY_LEVELS.length, 7);
});

test("secret is the fail-closed default", () => {
  assert.equal(DEFAULT_SENSITIVITY, "secret");
  assert.equal(defaultSettings().default_sensitivity, "secret");
  assert.equal(defaultSettings().enabled, false);
  assert.equal(defaultSettings().wizard_completed, false);
  assert.equal(defaultSettings().port, DEFAULT_PORT);
});

test("sensitivity validation + fail-closed coercion", () => {
  assert.equal(isValidSensitivity("internal"), true);
  assert.equal(isValidSensitivity("nonsense"), false);
  assert.equal(isValidSensitivity(42), false);
  assert.equal(coerceSensitivity("phi"), "phi");
  assert.equal(coerceSensitivity("bogus"), "secret");
  assert.equal(coerceSensitivity(undefined), "secret");
});

test("port validation + coercion", () => {
  assert.equal(isValidPort(4814), true);
  assert.equal(isValidPort(0), false);
  assert.equal(isValidPort(70000), false);
  assert.equal(isValidPort(4814.5), false);
  assert.equal(coercePort(9999), 9999);
  assert.equal(coercePort("nope"), DEFAULT_PORT);
});

test("normalizeSettings sanitizes untrusted disk data (fail-closed)", () => {
  const s = normalizeSettings({ notes_dir: "/x", default_sensitivity: "evil", port: -1, enabled: "yes", wizard_completed: 1 });
  assert.equal(s.notes_dir, "/x");
  assert.equal(s.default_sensitivity, "secret");
  assert.equal(s.port, DEFAULT_PORT);
  assert.equal(s.enabled, false); // only literal true enables
  assert.equal(s.wizard_completed, false);
  assert.deepEqual(normalizeSettings(null), defaultSettings());
});

test("enable is gated on folder + sensitivity (decision 3)", () => {
  const fresh = defaultSettings();
  assert.equal(canEnable(fresh), false);
  assert.match(enableBlockedReason(fresh), /notes folder/i);

  const withFolder = { ...fresh, notes_dir: "/notes" };
  // secret is a valid sensitivity, so folder is the only remaining gate
  assert.equal(canEnable(withFolder), true);
  assert.equal(enableBlockedReason(withFolder), null);
});
