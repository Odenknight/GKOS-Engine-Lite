# GKOS Engine Desktop — Quickstart

## Why use this

GKOS Engine Desktop watches your notes folder and builds a live map of what's in it, including a Graphiti projection. When enabled, the current sidecar exposes that map through a bearer-token-protected, GET-only REST API bound directly to loopback. GKOS has no built-in upload, remote-bind, proxy, or tunnel feature. A client holding the token can copy or export responses, and separately configured proxy or tunnel software can relay them beyond this computer.

Sensitivity values classify records and fail closed when missing or invalid, but they are not access-control rules for this API. Any local app that receives the bearer token can read every indexed note and projection. Keep sharing disabled or withhold the token when that is not acceptable.

Optional AI suggestions require a separately installed local helper. It is not
required for setup, never writes notes, and cannot override engine privacy or
validation rules.

The normal settings screen uses one **Enable local Agent API** switch.
Ports, access keys, and configuration text stay under **Advanced connection
settings** or **Show technical details**. Most people never need to change
them.

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
3. **Choose your default sensitivity classification.** Every note gets a classification. Unlabeled notes use whichever one you pick here. From most open to most private:
   - **public** — fine for anyone to see.
   - **internal** — for your eyes and your own tools, not the outside world.
   - **restricted** — limited to a specific purpose.
   - **confidential** — sensitive business or personal information.
   - **regulated** — covered by a specific law or rule.
   - **phi** — health information.
   - **secret** — your most private notes.

   **Secret** is picked for you by default. When in doubt, keep secret.

   One rule to remember: the app can mark a note as **more** sensitive than your default — never less. This classification does not filter Agent API responses.
4. **Turn on the local Agent API.** This is off until you switch it on. Turning it on makes every indexed note readable by apps holding the bearer token. The server itself binds to loopback; token-holding clients or separately configured proxies can still copy or relay responses.
5. **Finish.** The wizard closes and the app settles into your tray (Mac: menu bar, top right; Windows: system tray, bottom right).

## 4. Verify the local Agent API

Open the tray icon and choose **Copy Agent API health command**, then run the copied command in a terminal. It looks like this:

```
# Windows PowerShell
curl.exe -H "Authorization: Bearer <token>" "http://127.0.0.1:4814/health"

# macOS or Linux
curl -H "Authorization: Bearer <token>" "http://127.0.0.1:4814/health"
```

## You're done

GKOS Engine Desktop now lives quietly in your tray (menu bar on Mac, system tray on Windows). Click the icon any time to open Settings, see status, copy the health command again, or choose **Open 3D View** for a read-only 3D map of your notes and their connections. The pinned sidecar does not expose MCP; do not configure an MCP client against `/mcp`.
