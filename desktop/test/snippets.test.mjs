import { test } from "node:test";
import assert from "node:assert/strict";
import {
  baseUrl,
  mcpUrl,
  claudeCodeCommand,
  claudeProjectJson,
  cursorJson,
  genericToml,
  curlHealth,
  allSnippets,
  viewerQuery,
  viewerAppUrl,
} from "../dist-test/snippets.js";

const info = { port: 4814, token: "deadbeefcafe" };

test("baseUrl / mcpUrl use loopback host and given port", () => {
  assert.equal(baseUrl(4814), "http://127.0.0.1:4814");
  assert.equal(mcpUrl(9000), "http://127.0.0.1:9000/mcp");
});

test("claude code command mirrors Kosmos-Oden format with bearer + server name", () => {
  const cmd = claudeCodeCommand(info);
  assert.match(cmd, /^claude mcp add --transport http --header "Authorization: Bearer deadbeefcafe" kosmos-oden "http:\/\/127\.0\.0\.1:4814\/mcp"$/);
});

test("claude project .mcp.json is valid streamable-http block", () => {
  const obj = JSON.parse(claudeProjectJson(info));
  assert.equal(obj.mcpServers["kosmos-oden"].type, "streamable-http");
  assert.equal(obj.mcpServers["kosmos-oden"].url, "http://127.0.0.1:4814/mcp");
  assert.equal(obj.mcpServers["kosmos-oden"].headers.Authorization, "Bearer deadbeefcafe");
});

test("cursor block is valid JSON with url + header", () => {
  const obj = JSON.parse(cursorJson(info));
  assert.equal(obj.mcpServers["kosmos-oden"].url, "http://127.0.0.1:4814/mcp");
  assert.equal(obj.mcpServers["kosmos-oden"].headers.Authorization, "Bearer deadbeefcafe");
});

test("generic TOML has section header, url, and http_headers", () => {
  const toml = genericToml(info);
  assert.match(toml, /\[mcp_servers\.kosmos-oden\]/);
  assert.match(toml, /url = "http:\/\/127\.0\.0\.1:4814\/mcp"/);
  assert.match(toml, /http_headers = \{ Authorization = "Bearer deadbeefcafe" \}/);
});

test("curl health check targets /health with bearer", () => {
  assert.equal(curlHealth(info), 'curl -H "Authorization: Bearer deadbeefcafe" "http://127.0.0.1:4814/health"');
});

test("allSnippets returns every client key", () => {
  const s = allSnippets(info);
  assert.deepEqual(Object.keys(s).sort(), ["claudeCode", "claudeJson", "curl", "cursor", "toml"]);
});

// ---- 3D viewer URL building --------------------------------------------

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
