# GKOS Engine Desktop — User Guide

## Why this app exists

You write notes because thinking on paper (or in Obsidian, or a plain folder of markdown files) helps you think. Increasingly, you also want to talk to an AI assistant about those notes — ask it questions, have it find connections, let it help you write. The usual way to do that means uploading your notes to someone else's server. GKOS Engine Desktop is a different way.

This app runs quietly on your own computer. It watches a notes folder you choose and builds a live "map" of what's inside it, including a connections view called Graphiti. When enabled, a bearer-token-protected, GET-only REST API lets local programs read that map.

The built-in server binds directly to loopback and GKOS provides no upload, remote-bind, proxy, or tunnel feature. Sensitivity labels still matter as governance metadata and missing labels fail closed, but they do not filter this API. Any program that receives the bearer token can read, copy, or export every indexed note and projection. Separately configured proxy or tunnel software can relay those responses beyond this computer.

## Optional intelligence assistance

A separately installed local helper can suggest plain-language explanations,
metadata repairs, or possible relationships. It is off unless configured,
never treats a suggestion as an approved fact, cannot make a note less private,
and cannot edit files automatically. The desktop app works without it.

The everyday controls use plain language:

- **Enable local Agent API** turns the bearer-protected, read-only connection on or
  off.
- **Copy command** gives you an authenticated command for an implemented REST route.
- Ports, endpoints, and access keys are hidden under **Advanced connection
  settings** and **Show technical details**. Leave them unchanged unless an
  app specifically asks for them.

## Key concepts

**Notes folder** — the folder on your computer that holds your notes (for example, an Obsidian vault, or any folder of markdown files). You pick this folder during setup; the app only ever looks inside it.

**Projection** — the map the app builds from your notes: titles, tags, connections, and privacy labels, kept up to date as you write. The app never edits your original notes to build this map — it only reads them.

**Sensitivity levels** — every note in the map carries one of seven privacy levels, from most open to most private:

| Level | Meaning | Example |
|---|---|---|
| public | Anyone could see this | A published blog draft |
| internal | For you and your own tools | General project notes |
| restricted | Limited to one specific purpose | Notes for a single client engagement |
| confidential | Sensitive business or personal information | Salary notes, unreleased plans |
| regulated | Covered by a specific law or industry rule | Financial records subject to compliance rules |
| phi | Health information | Medical history, therapy notes |
| secret | Your most private notes | Passwords, deeply personal journal entries |

**Fail-closed classification** — if a note has no sensitivity label, the app uses your chosen default, which starts at **secret**. This prevents accidental low classification; it does not hide records from a bearer-authenticated Agent API caller.

**Raise-only** — the app can decide a note deserves a stricter label than your default, but it cannot automatically loosen a label. This is a metadata rule, not caller authorization.

**Loopback binding** — GKOS binds its built-in server directly to `127.0.0.1` and does not include a remote-bind, proxy, or tunnel feature. Loopback does not prevent a token-holding client from copying responses or separately configured software from relaying them elsewhere.

**Graphiti projections** — a knowledge-graph view: a web of connections between your notes (this note cites that one, this idea contradicts that one, and so on) that an AI assistant can follow when answering your questions.

**Read-only** — the app, and any assistant connected to it, can only look at your notes. Nothing it does can change, delete, or add to your original files.

## Installation

The app is not yet digitally signed — signing is coming in a future release. Until then, your operating system will show a warning when you first try to open it. That warning is normal for any unsigned app; the steps below are the standard, safe way to proceed when you trust the developer.

### macOS

1. Find the downloaded `.dmg` file and open it.
2. Drag the app into your Applications folder (or wherever you keep apps).
3. **Right-click** the app (not a regular click) and choose **Open**.
4. Click **Open** again on the confirmation dialog.

On macOS Ventura and newer, if step 3 doesn't show a dialog:

1. Open **System Settings** → **Privacy & Security**.
2. Scroll to the security message about GKOS Engine Desktop.
3. Click **Open Anyway**, then confirm.

> **If the right-click trick doesn't work:** macOS sometimes needs one extra nudge. Open Terminal and run:
> ```
> xattr -d com.apple.quarantine /Applications/GKOS\ Engine\ Desktop.app
> ```
> Then open the app normally. This removes the "downloaded from the internet" flag that triggers the warning.

### Windows

1. Double-click the downloaded `.exe` installer.
2. Windows shows **"Windows protected your PC."** Click **More info**.
3. Click **Run anyway**.
4. Follow the installer prompts.

## The first-run wizard, step by step

The wizard runs once, the first time you open the app, and requires a default sensitivity classification before the Agent API can be enabled.

1. **Welcome.** A one-paragraph explanation of why the app exists (the same pitch as above).
2. **Notes-folder picker.** A native file dialog. Choose the folder your notes live in. You can change this later in Settings.
3. **Default-sensitivity chooser.** The seven levels appear as a list, each with a one-line description (see the table above). **secret** is preselected. You must actively confirm your choice to move on. The choice classifies unlabeled notes but does not filter authenticated API responses.
4. **Enable the local Agent API.** Only now does this toggle appear, and it starts **off**. Turning it on makes every indexed note reachable to programs on this computer that have the bearer token. Leave it off if that is not acceptable.
5. **Finish.** The wizard closes and the app moves into your system tray (Mac: menu bar; Windows: system tray).

You can revisit every one of these choices later in the Settings window.

## The Settings window

Open Settings from the tray icon.

- **Notes folder** — shows your current folder; click to change it.
- **Sensitivity dropdown** — your default classification for unlabeled notes. Changing it **re-scans your notes** and restarts the local API. It does not grant or deny API access.
- **Enable toggle** — turns the local agent connection on or off.
- **Port** — the local network port the app listens on (default `4814`). Leave it alone unless another app complains about a conflict.
- **Status panel** — shows how many notes are indexed, when the last scan happened, and the local API address.
- **Token** — a long random bearer credential. Any program with it can read every implemented Agent API route, so treat copied commands as secrets.

## Using the local Agent API

The pinned sidecar does not expose MCP. Configuring Claude Desktop, Claude Code, Cursor, or another MCP client against `/mcp` will return `404`.

Open the tray icon and choose **Copy Agent API health command**, or copy one of the commands in Settings. The implemented routes are `/health`, `/notes`, `/graph`, and `/graphiti/episodes`. For example:
   ```
   # Windows PowerShell
   curl.exe -H "Authorization: Bearer <token>" "http://127.0.0.1:4814/health"

   # macOS or Linux
   curl -H "Authorization: Bearer <token>" "http://127.0.0.1:4814/health"
   ```
All four routes are GET-only and return JSON. No source-note mutation route exists.

## The 3D view

The tray menu can open a **3D view** of your notes: a read-only, rotatable map of your notes and the connections between them, using the same loopback API. The GKOS view does not upload responses or change source notes. Software holding the bearer token can still copy or relay API responses.

There are two ways to open it from the tray:

- **Open 3D View** shows the map in its own app window.
- **Open 3D View (browser)** opens the same map in your default web browser instead. Use this if the in-app window comes up blank.

Either way, if the local Agent API is switched off, the view still opens — it just shows a small connect form instead of a map. Turn the API on in Settings (and pick a notes folder) to populate it. The view connects to the loopback address automatically and carries the access token for you.

## Privacy & safety FAQ

**Does GKOS upload anything?**
GKOS has no built-in upload feature and its server binds directly to loopback. That does not control what another token-holding program does with a response, and it does not prevent separately configured proxy or tunnel software from exposing the local service.

**What can API clients see?**
Every indexed note and projection. Sensitivity is included as metadata but is not an access-control filter. The bearer token authorizes the whole current read API.

**What can agents change?**
Nothing. The connection is read-only. Assistants can look at your notes and the map built from them, but nothing they do can edit, delete, or create files in your notes folder.

**What if I have notes I do not want an API client to see?**
Do not enable the Agent API for that corpus, do not share its bearer token, or use a separate notes directory. A stricter sensitivity label alone does not hide a note from this API.

**Why is the app unsigned right now?**
Digital signing is a separate step the developer is completing later. An unsigned app still runs the same code — the warnings you see are your operating system's standard caution for any app that hasn't been through that process yet, not a sign of a problem with this specific app.

## Troubleshooting

**The app won't open on Mac.**
Make sure you used right-click → Open (not double-click) the first time, or check System Settings → Privacy & Security for an "Open Anyway" button. If neither works, try the `xattr` command in the installation section above.

**The app won't open on Windows.**
Make sure you clicked "More info" then "Run anyway" on the SmartScreen warning. If Windows blocks it entirely, check whether antivirus software is quarantining the file.

**An API client can't connect.**
Check that the API is enabled, the client targets `127.0.0.1` and an implemented route, and the request carries the current `Authorization: Bearer <token>` header. `/mcp` is not implemented.

**My notes aren't showing up.**
First check that the notes folder path in Settings points to where your notes live and inspect `/health` for the indexed count. Changing sensitivity will not make a record appear or disappear from the current API because sensitivity is not its access-control boundary.

## Uninstall

**macOS:** Quit the app from the tray, then drag it from Applications to the Trash.

**Windows:** Quit the app from the tray, then use **Settings → Apps → Installed apps**, find GKOS Engine Desktop, and click **Uninstall**.

## For the curious

GKOS Engine Desktop is built on the **Governed Knowledge Operations Standard (GKOS)** — an open standard for keeping AI-assisted knowledge trustworthy: originals preserved untouched, every claim traceable to its evidence, and every consequential action recorded. Read more in the GKOS standard repository and its Illustrated Edition.
