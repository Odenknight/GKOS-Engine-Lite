/**
 * Authenticated local Agent API examples — pure functions (no Tauri, no DOM)
 * so they are unit-testable with `node --test`.
 *
 * The pinned sidecar is a GET-only REST service. It does not expose MCP. Keep
 * these examples limited to routes the exact Engine pin implements so the UI
 * cannot advertise a configuration that deterministically returns 404.
 */

import { LOOPBACK_HOST } from "./strings";

export interface ConnectInfo {
  port: number;
  token: string;
}

/** The loopback base URL, e.g. `http://127.0.0.1:4814`. */
export function baseUrl(port: number): string {
  return `http://${LOOPBACK_HOST}:${port}`;
}

export const AGENT_API_ROUTES = Object.freeze({
  health: "/health",
  notes: "/notes",
  graph: "/graph",
  graphiti: "/graphiti/episodes",
} as const);

export type AgentApiRoute = keyof typeof AGENT_API_ROUTES;
export type CommandPlatform = "windows" | "posix";

export function commandPlatform(userAgent: string): CommandPlatform {
  return /windows/i.test(userAgent) ? "windows" : "posix";
}

/** An exact authenticated GET example for one implemented Agent API route. */
export function curlAgentApi(
  info: ConnectInfo,
  route: AgentApiRoute,
  platform: CommandPlatform = "posix",
): string {
  const executable = platform === "windows" ? "curl.exe" : "curl";
  return `${executable} -H "Authorization: Bearer ${info.token}" "${baseUrl(info.port)}${AGENT_API_ROUTES[route]}"`;
}

/**
 * Query string the standalone 3D viewer reads from `location.search` to
 * auto-connect to the loopback sidecar: `?api=<base>&token=<bearer>`. Values
 * are percent-encoded so the sidecar URL's `:` and `/` survive intact through
 * the query. An empty token is still emitted (the viewer then falls back to its
 * manual connect form rather than erroring). Mirrored byte-for-byte by the Rust
 * side that builds the window URL; kept here as the single tested definition.
 */
export function viewerQuery(info: ConnectInfo): string {
  return `api=${encodeURIComponent(baseUrl(info.port))}&token=${encodeURIComponent(info.token)}`;
}

/**
 * App-relative URL for the viewer served over the Tauri protocol (the in-app
 * "Open 3D View" window). The bundled HTML lives at the frontend root, so its
 * origin is `tauri://localhost` / `https://tauri.localhost` — the origins the
 * engine sidecar's CORS allowlist reflects.
 */
export function viewerAppUrl(info: ConnectInfo): string {
  return `vault-kosmos.html?${viewerQuery(info)}`;
}

/** All implemented REST examples keyed by response surface. */
export function allSnippets(
  info: ConnectInfo,
  platform: CommandPlatform = "posix",
): Record<string, string> {
  return {
    health: curlAgentApi(info, "health", platform),
    notes: curlAgentApi(info, "notes", platform),
    graph: curlAgentApi(info, "graph", platform),
    graphiti: curlAgentApi(info, "graphiti", platform),
  };
}
