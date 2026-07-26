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
/** Ecosystem-consistent MCP server name (matches Kosmos-Oden). */
export const MCP_SERVER_NAME = "kosmos-oden";

export const STRINGS = {
  app: {
    name: APP_NAME,
    tagline: "Point it at a notes folder; talk to your notes with local AI — nothing leaves this computer.",
  },

  tray: {
    openSettings: "Open Settings",
    copySnippet: "Copy MCP connect snippet",
    open3d: "Open 3D View",
    open3dBrowser: "Open 3D View (browser)",
    quit: "Quit",
    tooltipStopped: `${APP_NAME} — stopped`,
    tooltipIndexing: `${APP_NAME} — indexing…`,
    tooltipServing: `${APP_NAME} — serving (loopback only)`,
    tooltipError: `${APP_NAME} — error (see Settings)`,
    snippetCopied: "MCP connect snippet copied to the clipboard.",
    snippetUnavailable: "Enable the local Agent API first, then copy the snippet.",
  },

  wizard: {
    // Step 1 — Welcome
    welcomeTitle: "Welcome to GKOS Engine Desktop",
    welcomeBody:
      "This app runs quietly on your own computer. It watches a notes folder you choose and builds a live map of what's inside it — including a connections view called Graphiti. AI assistants that also run on your computer, such as Claude Desktop, can then look at that map through a small local doorway the app opens for them. Before any assistant can see anything, you choose a default privacy level; any note without an explicit label is treated as your most private setting by default. The app physically cannot accept a connection from anywhere except your own computer — no cloud, no remote access, no tunnel of any kind.",
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
      "Every note in the map carries a privacy level. Notes that declare no level of their own use the default you choose here. Secret is preselected — when in doubt, keep secret. Remember the raise-only rule: the app can mark a note as more private than your default automatically, but never less.",
    sensitivityConfirm: "Confirm and continue",
    sensitivityBack: "Back",

    // Step 4 — Enable toggle + network notice
    enableTitle: "Connect local AI apps",
    enableToggle: "Allow AI apps on this computer to read the protected note map",
    // Verbatim network-notice pattern mirrored from the Kosmos plugin.
    enableNotice:
      "This is optional and starts off. When enabled, approved AI apps on this computer can read the protected note map—not edit your notes. It cannot connect from the internet, another device, or your phone. Leave it off until you want to connect an AI app.",
    enableBack: "Back",
    enableNext: "Continue",

    // Step 5 — Finish
    finishTitle: "You're all set",
    finishBody:
      "The app now lives in your system tray (Mac: menu bar, top right; Windows: system tray, bottom right). Click the icon any time to open Settings, see status, or copy a connect snippet.",
    finishDone: "Finish",
  },

  settings: {
    title: "Settings",
    folderLabel: "Notes folder",
    folderChange: "Change…",
    sensitivityLabel: "Default privacy level",
    sensitivityHelp:
      "Used only when a note has no privacy level of its own. GKOS may make a note more private, never less.",
    enableLabel: "Share with local AI apps",
    enableHelp: "Optional. AI apps on this computer can read the protected map, but cannot edit your notes.",
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
    heading: "Connect an AI app",
    intro:
      "Choose your AI app and copy its ready-to-paste setup. Your private access key is included automatically.",
    disabled: "Turn on “Share with local AI apps” first.",
    claudeCode: "Claude Desktop or Claude Code",
    claudeCodeDesc: "Copy the ready-to-paste setup command.",
    claudeJson: "Claude project",
    claudeJsonDesc: "Copy setup for a single Claude project.",
    cursor: "Cursor",
    cursorDesc: "Copy setup for Cursor.",
    toml: "Other AI apps",
    tomlDesc: "Advanced setup for compatible local AI apps.",
    copy: "Copy setup",
    copied: "Setup copied. Paste it into your AI app.",
    technicalDetails: "Show technical details",
  },

  view3d: {
    // Copy for the 3D view feature (tray items + docs share this register).
    heading: "The 3D view",
    intro:
      "A read-only 3D map of your notes and their connections, opened from the tray. It talks to the same loopback engine your assistants use — nothing leaves this computer, and it never edits your notes.",
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
