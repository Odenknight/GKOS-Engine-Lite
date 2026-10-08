import { test } from "node:test";
import assert from "node:assert/strict";
import {
  AGENT_API_ROUTES,
  baseUrl,
  commandPlatform,
  curlAgentApi,
  allSnippets,
  viewerQuery,
  viewerAppUrl,
} from "../dist-test/snippets.js";

const info = { port: 4814, token: "deadbeefcafe" };

test("Agent API examples expose only exact implemented GET routes", () => {
  assert.equal(baseUrl(4814), "http://127.0.0.1:4814");
  assert.deepEqual(AGENT_API_ROUTES, {
    health: "/health",
    notes: "/notes",
    graph: "/graph",
    graphiti: "/graphiti/episodes",
  });
  assert.equal(
    curlAgentApi(info, "health"),
    'curl -H "Authorization: Bearer deadbeefcafe" "http://127.0.0.1:4814/health"',
  );
});

test("Windows examples use curl.exe instead of PowerShell's curl alias", () => {
  assert.equal(commandPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), "windows");
  assert.equal(commandPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X)"), "posix");
  assert.equal(
    curlAgentApi(info, "health", "windows"),
    'curl.exe -H "Authorization: Bearer deadbeefcafe" "http://127.0.0.1:4814/health"',
  );
  for (const command of Object.values(allSnippets(info, "windows"))) {
    assert.match(command, /^curl\.exe /);
  }
});

test("all snippets use bearer auth and never advertise the absent MCP route", () => {
  const snippets = allSnippets(info);
  assert.deepEqual(Object.keys(snippets).sort(), ["graph", "graphiti", "health", "notes"]);
  for (const [name, command] of Object.entries(snippets)) {
    assert.match(command, /Authorization: Bearer deadbeefcafe/, name);
    assert.doesNotMatch(command, /\/mcp(?:\b|\/)/i, name);
  }
  assert.match(snippets.notes, /\/notes"$/);
  assert.match(snippets.graph, /\/graph"$/);
  assert.match(snippets.graphiti, /\/graphiti\/episodes"$/);
});

test("viewerQuery percent-encodes the api base and carries the token", () => {
  assert.equal(
    viewerQuery(info),
    "api=http%3A%2F%2F127.0.0.1%3A4814&token=deadbeefcafe",
  );
});

test("viewerQuery round-trips through URLSearchParams the way the viewer reads it", () => {
  const params = new URLSearchParams(viewerQuery({ port: 5000, token: "ab+cd/ef=" }));
  assert.equal(params.get("api"), "http://127.0.0.1:5000");
  assert.equal(params.get("token"), "ab+cd/ef=", "a token with URL-special chars survives intact");
});

test("viewerQuery emits an empty token when none is available (viewer shows connect form)", () => {
  assert.equal(viewerQuery({ port: 4814, token: "" }), "api=http%3A%2F%2F127.0.0.1%3A4814&token=");
});

test("viewerAppUrl targets the bundled HTML at the frontend root with the query", () => {
  assert.equal(
    viewerAppUrl(info),
    "vault-kosmos.html?api=http%3A%2F%2F127.0.0.1%3A4814&token=deadbeefcafe",
  );
});
