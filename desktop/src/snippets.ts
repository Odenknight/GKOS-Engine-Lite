/**
 * Quick-connect snippet generation — pure functions (no Tauri, no DOM) so they
 * are unit-testable with `node --test`. Formats mirror Kosmos-Oden's
 * settings.ts exactly (server name `kosmos-oden`, MCP Streamable HTTP + bearer)
 * so the ecosystem stays consistent.
 */

import { LOOPBACK_HOST, MCP_SERVER_NAME } from "./strings";

export interface ConnectInfo {
  port: number;
  token: string;
}

/** The loopback base URL, e.g. `http://127.0.0.1:4814`. */
export function baseUrl(port: number): string {
  return `http://${LOOPBACK_HOST}:${port}`;
}

/** The MCP endpoint URL, e.g. `http://127.0.0.1:4814/mcp`. */
export function mcpUrl(port: number): string {
  return `${baseUrl(port)}/mcp`;
}

/**
 * The short tray snippet the docs illustrate for Claude Desktop. We emit the
 * fully-working variant (transport + bearer header) so the pasted command
 * actually authenticates against the token-gated loopback server.
 */
export function claudeCodeCommand(info: ConnectInfo): string {
  return `claude mcp add --transport http --header "Authorization: Bearer ${info.token}" ${MCP_SERVER_NAME} "${mcpUrl(info.port)}"`;
}

/** `.mcp.json` block for a Claude Code project (Streamable HTTP). */
export function claudeProjectJson(info: ConnectInfo): string {
  return JSON.stringify(
    {
      mcpServers: {
        [MCP_SERVER_NAME]: {
          type: "streamable-http",
          url: mcpUrl(info.port),
          headers: { Authorization: `Bearer ${info.token}` },
        },
      },
    },
    null,
    2,
  );
}

/** Cursor MCP settings block (HTTP transport + bearer header). */
export function cursorJson(info: ConnectInfo): string {
  return JSON.stringify(
    {
      mcpServers: {
        [MCP_SERVER_NAME]: {
          url: mcpUrl(info.port),
          headers: { Authorization: `Bearer ${info.token}` },
        },
      },
    },
    null,
    2,
  );
}

/** Generic TOML (Codex / universal MCP surfaces). */
export function genericToml(info: ConnectInfo): string {
  return `[mcp_servers.${MCP_SERVER_NAME}]\nurl = "${mcpUrl(info.port)}"\nhttp_headers = { Authorization = "Bearer ${info.token}" }\n`;
}

/** A cURL health check (plain HTTP, bearer). */
export function curlHealth(info: ConnectInfo): string {
  return `curl -H "Authorization: Bearer ${info.token}" "${baseUrl(info.port)}/health"`;
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

/** All snippets keyed by client, for the quick-connect panel. */
export function allSnippets(info: ConnectInfo): Record<string, string> {
  return {
    claudeCode: claudeCodeCommand(info),
    claudeJson: claudeProjectJson(info),
    cursor: cursorJson(info),
    toml: genericToml(info),
    curl: curlHealth(info),
  };
}
