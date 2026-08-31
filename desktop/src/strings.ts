/**
 * GKOS Engine Desktop — every user-visible string, in one place.
 *
 * Single source of truth for UI copy so the docs agent can quote it verbatim
 * and translators (future) have one file to touch. Tone mirrors the Kosmos
 * plugin: plain-language, privacy-first, no hype.
 */

export const APP_NAME = "GKOS Engine Desktop";

/** The seven-level sensitivity vocabulary (GKOS §11), most-open → most-private.
 *  Order and level names are authoritative and mirror the engine
 *  (SENSITIVITY_LEVELS in gkos-engine/src/desktop-agent.ts). `secret` is the
 *  fail-closed default and is preselected in the wizard. */
export const SENSITIVITY_LEVELS = [
  "public",
  "internal",
  "restricted",
  "confidential",
  "regulated",
  "phi",
  "secret",
] as const;

export type SensitivityLevel = (typeof SENSITIVITY_LEVELS)[number];

/** Fail-closed default (decision 3): unlabeled notes resolve here until changed. */
export const DEFAULT_SENSITIVITY: SensitivityLevel = "secret";

/** One-line plain-language descriptions shown beside each level in the wizard. */
export const SENSITIVITY_DESCRIPTIONS: Record<SensitivityLevel, string> = {
  public: "Anyone could see this — e.g. a published blog draft.",
  internal: "For you and your own tools — general project notes.",
  restricted: "Limited to one specific purpose — a single client engagement.",
  confidential: "Sensitive business or personal information — salary notes, unreleased plans.",
  regulated: "Covered by a specific law or industry rule — compliance-bound records.",
  phi: "Health information — medical history, therapy notes.",
  secret: "Your most private notes — passwords, deeply personal journal entries.",
};

export const DEFAULT_PORT = 4814;
export const LOOPBACK_HOST = "127.0.0.1";
export const STRINGS = {
  app: {
    name: APP_NAME,
    tagline: "Point it at a notes folder and inspect its read-only map through a local API.",
  },

  tray: {
    openSettings: "Open Settings",
    copySnippet: "Copy Agent API health command",
    open3d: "Open 3D View",
    open3dBrowser: "Open 3D View (browser)",
    quit: "Quit",
    tooltipStopped: `${APP_NAME} — stopped`,
    tooltipIndexing: `${APP_NAME} — indexing…`,
    tooltipServing: `${APP_NAME} — serving (loopback only)`,
    tooltipError: `${APP_NAME} — error (see Settings)`,
    snippetCopied: "Agent API health command copied to the clipboard.",
    snippetUnavailable: "Enable the local Agent API first, then copy the command.",
  },

  wizard: {
    // Step 1 — Welcome
    welcomeTitle: "Welcome to GKOS Engine Desktop",
    welcomeBody:
      "This app runs quietly on your own computer. It watches a notes folder you choose and builds a live map of what's inside it — including a connections view called Graphiti. The built-in server binds directly to loopback and has no upload, remote-bind, proxy, or tunnel feature. Sensitivity levels classify records but do not filter this API: an app with the bearer token can read, copy, or export every indexed note. A separately configured client, proxy, or tunnel can relay those responses beyond this computer.",
    welcomeNext: "Get started",

    // Step 2 — Notes folder
    folderTitle: "Choose your notes folder",
    folderBody:
      "Pick the folder on your computer where your notes live (an Obsidian vault, or any folder of markdown files). The app only ever looks inside this folder, and only ever reads — it never edits your notes.",
    folderPick: "Choose folder…",
    folderNone: "No folder chosen yet.",
    folderBack: "Back",
    folderNext: "Continue",

    // Step 3 — Default sensitivity
    sensitivityTitle: "Choose your default privacy level",
    sensitivityBody:
      "Every note in the map carries a sensitivity classification. Notes that declare no level of their own use the default you choose here. Secret is preselected — when in doubt, keep secret. The raise-only rule prevents automatic lowering, but classifications do not grant or deny Agent API access.",
    sensitivityConfirm: "Confirm and continue",
    sensitivityBack: "Back",

    // Step 4 — Enable toggle + network notice
    enableTitle: "Enable the local Agent API",
    enableToggle: "Allow apps on this computer with the access token to read the note map",
    // Verbatim network-notice pattern mirrored from the Kosmos plugin.
    enableNotice:
      "This is optional and starts off. The built-in server binds directly to loopback. When enabled, any app with the bearer token can read, copy, or export every indexed note and projection—not edit your notes through this API. Sensitivity labels are metadata, not an access-control filter. GKOS has no built-in remote proxy or tunnel, but separately configured software can relay responses.",
    enableBack: "Back",
    enableNext: "Continue",

    // Step 5 — Finish
    finishTitle: "You're all set",
    finishBody:
      "The app now lives in your system tray (Mac: menu bar, top right; Windows: system tray, bottom right). Click the icon any time to open Settings, see status, or copy an authenticated health-check command.",
    finishDone: "Finish",
  },

  settings: {
    title: "Settings",
    folderLabel: "Notes folder",
    folderChange: "Change…",
    sensitivityLabel: "Default privacy level",
    sensitivityHelp:
      "Used only to classify a note that has no sensitivity of its own. It does not control who can read the Agent API.",
    enableLabel: "Enable local Agent API",
    enableHelp: "Optional. Any local app given the bearer token can read every indexed note, but cannot edit your notes.",
    advancedHeading: "Advanced connection settings",
    advancedHelp: "Most people can leave these settings unchanged.",
    portLabel: "Port",
    portHelp: `Loopback port (default ${DEFAULT_PORT}). Change only if another app conflicts; the server restarts automatically.`,
    statusHeading: "Your note map",
    statusState: "State",
    statusNotesIndexed: "Notes indexed",
    statusLastScan: "Last scan",
    statusEndpoint: "Endpoint",
    tokenLabel: "Access token",
    tokenReveal: "Reveal",
    tokenHide: "Hide",
    tokenCopy: "Copy",
    tokenCopied: "Token copied.",
    tokenHidden: "•••••••• (hidden)",
    errorRestartsExhausted:
      "GKOS could not start. Check that your notes folder still exists, then turn sharing off and on. Advanced users can also check the connection settings below.",
  },

  connect: {
    heading: "Local Agent API",
    intro:
      "The current sidecar is an authenticated, GET-only REST API. It does not expose MCP. These commands include your bearer token.",
    disabled: "Turn on the local Agent API first.",
    health: "Health and status",
    healthDesc: "Check whether the local API is ready.",
    notes: "Indexed notes",
    notesDesc: "Read all indexed notes and their sensitivity metadata.",
    graph: "Canonical graph",
    graphDesc: "Read the complete current graph projection.",
    graphiti: "Graphiti episodes",
    graphitiDesc: "Read the current Graphiti export projection.",
    copy: "Copy command",
    copied: "Command copied. Treat the embedded bearer token as a secret.",
    technicalDetails: "Show technical details",
  },

  view3d: {
    // Copy for the 3D view feature (tray items + docs share this register).
    heading: "The 3D view",
    intro:
      "A read-only 3D map of your notes and their connections, opened from the tray. It uses the same bearer-protected loopback API and does not edit source notes. GKOS does not upload the response, but software holding the token can copy or relay it.",
    openInApp: "Open 3D View opens the map in its own app window.",
    openInBrowser:
      "Open 3D View (browser) opens the same map in your default web browser instead — use this if the in-app window comes up blank.",
    needsRunning:
      "If the local Agent API is off, the view still opens and shows a connect form; turn the API on in Settings to populate the map.",
  },

  state: {
    stopped: "Stopped",
    indexing: "Indexing…",
    serving: "Serving (loopback only)",
    error: "Error",
  },
} as const;
