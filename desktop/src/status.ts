/**
 * Sidecar status document — mirrors the engine's StatusDoc (see
 * gkos-engine README "Desktop agent → Status file schema"). Read by the shell
 * from the status file and surfaced in Settings.
 */
import type { SensitivityLevel } from "./strings";

export type SidecarState = "indexing" | "serving" | "error";

export interface StatusDoc {
  pid: number;
  port: number;
  url: string;
  token_path: string;
  notes_dir: string;
  default_sensitivity: SensitivityLevel;
  notes_indexed: number;
  state: SidecarState;
  last_scan_iso: string | null;
}

/** Supervisor status reported by the Rust side (superset of engine states). */
export type SupervisorStatus = "stopped" | "indexing" | "serving" | "error";

export interface ConnectPayload {
  port: number;
  token: string;
}
