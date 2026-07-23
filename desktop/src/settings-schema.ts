/**
 * Settings shape + validation — pure functions (no Tauri, no DOM), unit-tested.
 * Mirrors the Rust `Settings` struct persisted to the app config dir as JSON:
 *   notes_dir, default_sensitivity, port, enabled, wizard_completed.
 */

import {
  SENSITIVITY_LEVELS,
  DEFAULT_SENSITIVITY,
  DEFAULT_PORT,
  type SensitivityLevel,
} from "./strings";

export interface Settings {
  notes_dir: string | null;
  default_sensitivity: SensitivityLevel;
  port: number;
  enabled: boolean;
  wizard_completed: boolean;
}

export function defaultSettings(): Settings {
  return {
    notes_dir: null,
    default_sensitivity: DEFAULT_SENSITIVITY,
    port: DEFAULT_PORT,
    enabled: false,
    wizard_completed: false,
  };
}

export function isValidSensitivity(v: unknown): v is SensitivityLevel {
  return typeof v === "string" && (SENSITIVITY_LEVELS as readonly string[]).includes(v);
}

/** Fail-closed: any invalid/missing sensitivity coerces to `secret`. */
export function coerceSensitivity(v: unknown): SensitivityLevel {
  return isValidSensitivity(v) ? v : DEFAULT_SENSITIVITY;
}

export function isValidPort(v: unknown): boolean {
  return typeof v === "number" && Number.isInteger(v) && v >= 1 && v <= 65535;
}

export function coercePort(v: unknown): number {
  return isValidPort(v) ? (v as number) : DEFAULT_PORT;
}

/** Normalize an untrusted settings object (e.g. from disk) into a valid one. */
export function normalizeSettings(raw: Partial<Settings> | null | undefined): Settings {
  const base = defaultSettings();
  if (!raw || typeof raw !== "object") return base;
  return {
    notes_dir: typeof raw.notes_dir === "string" && raw.notes_dir.length > 0 ? raw.notes_dir : null,
    default_sensitivity: coerceSensitivity(raw.default_sensitivity),
    port: coercePort(raw.port),
    enabled: raw.enabled === true,
    wizard_completed: raw.wizard_completed === true,
  };
}

/**
 * Wizard gating (decision 3): the Agent API can only ever be enabled once a
 * notes folder is chosen AND a default sensitivity is set. Returns the reason a
 * user cannot enable yet, or null when enabling is permitted.
 */
export function enableBlockedReason(s: Settings): string | null {
  if (!s.notes_dir) return "Choose a notes folder first.";
  if (!isValidSensitivity(s.default_sensitivity)) return "Choose a default sensitivity first.";
  return null;
}

export function canEnable(s: Settings): boolean {
  return enableBlockedReason(s) === null;
}
