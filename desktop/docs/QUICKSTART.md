# GKOS Engine Desktop — Quickstart

## Why use this

Your notes are yours, and they stay on your computer. GKOS Engine Desktop lets AI assistants you already use — like Claude — read and understand your notes without uploading anything anywhere. It watches your notes folder, builds a live map of what's in it (including a connections view called Graphiti), and applies privacy rules you choose before any assistant can see a single word. It only talks to programs on your own machine — it cannot accept a connection from the internet.

## 1. Download

Get the installer from the project's GitHub Releases page.

- **Mac:** download the `.dmg` file.
- **Windows:** download the `.exe` file.

## 2. Install

The app is not yet digitally signed (that's coming later). Your operating system will warn you about that. These steps are the normal, safe way to run software from a developer you trust while you wait for the signed version.

**On macOS:**

1. Find the downloaded app and **right-click** it (not a regular click).
2. Choose **Open** from the menu that appears.
3. Click **Open** again on the dialog that pops up.

If you're on macOS Ventura or newer and don't see that dialog:

1. Open **System Settings**.
2. Go to **Privacy & Security**.
3. Click **Open Anyway** next to the message about GKOS Engine Desktop.

**On Windows:**

1. Double-click the downloaded `.exe` file.
2. When you see **"Windows protected your PC,"** click **More info**.
3. Click **Run anyway**.

## 3. First-run setup

The app walks you through five short steps the first time it opens.

1. **Welcome.** A short explanation of why the app exists (the same idea as above).
2. **Pick your notes folder.** Choose the folder on your computer where your notes live.
3. **Choose your default privacy level.** Every note gets a privacy level. Unlabeled notes use whichever one you pick here. From most open to most private:
   - **public** — fine for anyone to see.
   - **internal** — for your eyes and your own tools, not the outside world.
   - **restricted** — limited to a specific purpose.
   - **confidential** — sensitive business or personal information.
   - **regulated** — covered by a specific law or rule.
   - **phi** — health information.
   - **secret** — your most private notes.

   **Secret** is picked for you by default. When in doubt, keep secret.

   One rule to remember: the app can mark a note as **more** private than your default — never less.
4. **Turn on the local agent connection.** This is off until you switch it on. Turning it on means notes on your computer become reachable by other apps on this same computer — never over the internet. Your default privacy level from step 3 governs what unlabeled notes show.
5. **Finish.** The wizard closes and the app settles into your tray (Mac: menu bar, top right; Windows: system tray, bottom right).

## 4. Connect Claude Desktop

Open the tray icon and choose **Copy MCP connect snippet**. Paste it into Claude Desktop's settings. It looks like this:

```
claude mcp add kosmos-oden http://127.0.0.1:4814/mcp
```

## You're done

GKOS Engine Desktop now lives quietly in your tray (menu bar on Mac, system tray on Windows). Click the icon any time to open Settings, see status, or copy the connect snippet again.
