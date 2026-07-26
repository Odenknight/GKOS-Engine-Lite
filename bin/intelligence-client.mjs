import { readFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import {
  INTELLIGENCE_CONTRACT_VERSION,
  INTELLIGENCE_PROPOSAL_TYPES,
  validateIntelligenceResponse,
} from "gkos-engine";

const DEFAULT_URL = "http://127.0.0.1:8765";
export const FRIENDLY_TASKS = Object.freeze({
  explain: "diagnostic_explanation",
  improve: "documentation_improvement",
  repair: "metadata_repair",
  "find-links": "relationship",
  "find-claims": "claim_extraction",
  "check-conflicts": "contradiction",
  "check-privacy": "classification_raise",
});

export function resolveTask(task) {
  return FRIENDLY_TASKS[task] || task;
}

export function intelligenceUrl(env = process.env) {
  const url = new URL(env.GKOS_INTELLIGENCE_URL || DEFAULT_URL);
  if (!["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)) {
    throw new Error("GKOS intelligence sidecar must use a loopback URL");
  }
  return url;
}

export async function requestIntelligence({
  task, file, targetId = file, effectiveSensitivity,
  env = process.env, fetchImpl = fetch,
}) {
  task = resolveTask(task);
  if (!INTELLIGENCE_PROPOSAL_TYPES.includes(task)) throw new Error(`Unknown assistance task: ${task}`);
  const request = {
    contractVersion: INTELLIGENCE_CONTRACT_VERSION,
    requestId: `request:${randomUUID()}`,
    task,
    targetId,
    noteText: await readFile(file, "utf8"),
    ...(effectiveSensitivity ? { effectiveSensitivity } : {}),
  };
  const headers = { "content-type": "application/json" };
  if (env.GKOS_INTELLIGENCE_TOKEN) headers.authorization = `Bearer ${env.GKOS_INTELLIGENCE_TOKEN}`;
  let response;
  try {
    response = await fetchImpl(new URL("/v1/proposals", intelligenceUrl(env)), {
      method: "POST", headers, body: JSON.stringify(request),
      signal: AbortSignal.timeout(30_000),
    });
  } catch (error) {
    throw new Error(`Optional intelligence sidecar is unavailable: ${error.message}`);
  }
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(`Optional intelligence sidecar rejected the request (${response.status}): ${body?.error ?? "unknown error"}`);
  const checked = validateIntelligenceResponse(body, request);
  if (checked.diagnostics.length) {
    const summary = checked.diagnostics.map((d) => `${d.code}: ${d.message}`).join("\n");
    if (!checked.proposals.length) throw new Error(`Sidecar returned no safe proposals:\n${summary}`);
    return { ...checked, warnings: summary };
  }
  return checked;
}

export async function assistMain(argv, options = {}) {
  const [task, file, ...rest] = argv;
  if (!task || !file || rest.length) {
    throw new Error([
      "Choose what you want help with and a note:",
      "  okf-lite assist explain <note.md>",
      "  okf-lite assist improve <note.md>",
      "  okf-lite assist repair <note.md>",
      "  okf-lite assist find-links <note.md>",
      "  okf-lite assist find-claims <note.md>",
      "  okf-lite assist check-conflicts <note.md>",
      "  okf-lite assist check-privacy <note.md>",
      "",
      "This is optional and never changes your note.",
    ].join("\n"));
  }
  return requestIntelligence({ task, file, ...options });
}
