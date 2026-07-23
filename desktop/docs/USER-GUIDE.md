# GKOS Engine Desktop — User Guide

## Why this app exists

You write notes because thinking on paper (or in Obsidian, or a plain folder of markdown files) helps you think. Increasingly, you also want to talk to an AI assistant about those notes — ask it questions, have it find connections, let it help you write. The usual way to do that means uploading your notes to someone else's server. GKOS Engine Desktop is a different way.

This app runs quietly on your own computer. It watches a notes folder you choose, and builds a live "map" of what's inside it — including a connections view called Graphiti, which shows how your notes relate to each other, like a web of related ideas. AI assistants that also run on your computer, such as Claude Desktop, can then look at that map through a small local doorway the app opens for them.

The key difference is privacy. Before any assistant can see anything, you choose a default privacy level for your notes, and any note without an explicit label is treated as your most private setting by default — not the most open. The app also physically cannot accept a connection from anywhere except your own computer: there is no cloud feature, no remote access, and no tunnel of any kind. Your notes never leave the machine they're on.

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

**Fail-closed** — if a note has no privacy label at all, the app never guesses something open. It treats the note as being at your chosen default level, and that default starts at **secret** until you change it. Nothing is exposed by accident.

**Raise-only** — the app can decide a note deserves a stricter (more private) label than your default, but it can never loosen a label to make a note more open than what you or the note itself declared. Privacy only ever tightens automatically, never relaxes.

**Loopback / local-only** — the app's connection to AI assistants only works between programs on the same computer (this is what "loopback" means). It cannot accept a connection from the internet, another computer, or your phone. Your notes never leave this computer.

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

The wizard runs once, the first time you open the app, and enforces one important order: you must choose a privacy default **before** the app can ever talk to an assistant.

1. **Welcome.** A one-paragraph explanation of why the app exists (the same pitch as above).
2. **Notes-folder picker.** A native file dialog. Choose the folder your notes live in. You can change this later in Settings.
3. **Default-sensitivity chooser.** The seven levels appear as a list, each with a one-line description (see the table above). **secret** is preselected. You must actively confirm your choice to move on — nothing proceeds silently. Remember the raise-only rule: this is a floor for unlabeled notes, and the app can only make a note *more* private automatically, never less.
4. **Enable the local agent connection.** Only now does this toggle appear, and it starts **off**. Turning it on shows a network notice: your notes become reachable to other programs running on this same computer (never the internet), and the default sensitivity you just set governs any unlabeled note. Leave it off if you're not ready to connect an assistant yet — you can turn it on anytime from Settings.
5. **Finish.** The wizard closes and the app moves into your system tray (Mac: menu bar; Windows: system tray).

You can revisit every one of these choices later in the Settings window.

## The Settings window

Open Settings from the tray icon.

- **Notes folder** — shows your current folder; click to change it.
- **Sensitivity dropdown** — your default privacy level. Changing it **re-scans your notes** and restarts the local connection, because the new default has to be applied from scratch.
- **Enable toggle** — turns the local agent connection on or off.
- **Port** — the local network port the app listens on (default `4814`). Leave it alone unless another app complains about a conflict.
- **Status panel** — shows how many notes are indexed, when the last scan happened, and the local address assistants connect to.
- **Token** — a long random code the app generates for itself, like a password it uses to prove a connecting program is allowed in. You never need to type it yourself — the connect snippets below include it automatically.

## Connecting agents

### Claude Desktop (recommended path)

1. Open the tray icon and choose **Copy MCP connect snippet**.
2. In Claude Desktop, open its settings and add a new MCP server using the snippet you copied. It looks like:
   ```
   claude mcp add kosmos-oden http://127.0.0.1:4814/mcp
   ```
3. Restart Claude Desktop if it asks you to.

### Cursor

The tray menu's connect panel also offers a Cursor-formatted snippet — copy it into Cursor's MCP settings the same way.

### Other tools (generic)

A generic TOML snippet is also available from the same panel, for any other MCP-compatible tool that accepts a local server address and token.

## The 3D view

The tray menu can open a **3D view** of your notes: a read-only, rotatable map of your notes and the connections between them, the same Graphiti projection an assistant would follow. It reads from the same local engine your assistants use — nothing leaves this computer, and, like everything else in the app, it can only look at your notes, never change them.

There are two ways to open it from the tray:

- **Open 3D View** shows the map in its own app window.
- **Open 3D View (browser)** opens the same map in your default web browser instead. Use this if the in-app window comes up blank.

Either way, if the local Agent API is switched off, the view still opens — it just shows a small connect form instead of a map. Turn the API on in Settings (and pick a notes folder) to populate it. The view connects to the loopback address automatically and carries the access token for you, exactly like the connect snippets do.

## Privacy & safety FAQ

**Is anything uploaded anywhere?**
No. The app only listens on "loopback" — a technical term for connections that stay inside your own computer. It cannot accept a connection from the internet, another device, or anywhere else. There is no cloud feature in this app at all.

**What can agents see?**
Only what your sensitivity settings allow. Each note has an effective privacy level — either the level it declares, or your chosen default if it declares none — and an assistant only sees notes at or below the ceiling your setup allows.

**What can agents change?**
Nothing. The connection is read-only. Assistants can look at your notes and the map built from them, but nothing they do can edit, delete, or create files in your notes folder.

**What if I have private notes I don't want any assistant to see?**
Label them with a stricter sensitivity level yourself, or simply rely on fail-closed: any note with no label at all defaults to your chosen default, which starts at the strictest setting (secret) until you change it.

**Why is the app unsigned right now?**
Digital signing is a separate step the developer is completing later. An unsigned app still runs the same code — the warnings you see are your operating system's standard caution for any app that hasn't been through that process yet, not a sign of a problem with this specific app.

## Troubleshooting

**The app won't open on Mac.**
Make sure you used right-click → Open (not double-click) the first time, or check System Settings → Privacy & Security for an "Open Anyway" button. If neither works, try the `xattr` command in the installation section above.

**The app won't open on Windows.**
Make sure you clicked "More info" then "Run anyway" on the SmartScreen warning. If Windows blocks it entirely, check whether antivirus software is quarantining the file.

**An assistant can't connect.**
Check three things in order: is the enable toggle switched on in Settings; did you paste the current connect snippet (regenerate it from the tray if you're not sure); and have you restarted the assistant app after connecting it.

**My notes aren't showing up.**
First check that the notes folder path in Settings actually points to where your notes live. If the folder is correct but notes still seem to be missing, check your default sensitivity setting: if it's set very strict (like secret) and your notes have no privacy labels of their own, they'll all resolve to that strict level — and an assistant's request may not be allowed to see anything at that level. This is by design, not a bug: the app would rather hide too much than expose too much. Lower your default sensitivity in Settings if you want more unlabeled notes visible.

## Uninstall

**macOS:** Quit the app from the tray, then drag it from Applications to the Trash.

**Windows:** Quit the app from the tray, then use **Settings → Apps → Installed apps**, find GKOS Engine Desktop, and click **Uninstall**.

## For the curious

GKOS Engine Desktop is built on the **Governed Knowledge Operations Standard (GKOS)** — an open standard for keeping AI-assisted knowledge trustworthy: originals preserved untouched, every claim traceable to its evidence, and every consequential action recorded. Read more in the GKOS standard repository and its Illustrated Edition.
