/**
 * GKOS Engine Desktop — frontend controller (plain TS + Tauri APIs, no
 * framework). One SPA renders either the first-run wizard or the Settings
 * window, chosen by the URL hash (`#wizard` | `#settings`). All Rust bridges
 * go through `invoke`; all copy comes from strings.ts.
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { STRINGS, SENSITIVITY_LEVELS, SENSITIVITY_DESCRIPTIONS, type SensitivityLevel } from "./strings";
import { normalizeSettings, enableBlockedReason, type Settings } from "./settings-schema";
import { allSnippets } from "./snippets";
import type { StatusDoc, SupervisorStatus, ConnectPayload } from "./status";

// ---- Rust bridge (command names must match src-tauri/src/lib.rs) ----
const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (patch: Partial<Settings>) => invoke<Settings>("set_settings", { patch }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  startSidecar: () => invoke<void>("start_sidecar"),
  stopSidecar: () => invoke<void>("stop_sidecar"),
  readStatus: () => invoke<StatusDoc | null>("read_status"),
  supervisorStatus: () => invoke<SupervisorStatus>("supervisor_status"),
  connectInfo: () => invoke<ConnectPayload | null>("connect_info"),
  completeWizard: () => invoke<void>("complete_wizard"),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
};

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const el = (tag: string, cls?: string, text?: string): HTMLElement => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};
function toast(msg: string): void {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  window.setTimeout(() => t.classList.remove("show"), 2600);
}

async function copy(text: string, note: string): Promise<void> {
  try {
    await writeText(text);
    toast(note);
  } catch {
    toast("Copy failed — select and copy manually.");
  }
}

// ============================ Wizard ============================

async function renderWizard(root: HTMLElement): Promise<void> {
  let settings = normalizeSettings(await api.getSettings());
  let step = 0;
  let chosenSensitivity: SensitivityLevel = settings.default_sensitivity || "secret";

  const render = () => {
    root.innerHTML = "";
    const card = el("div", "card wizard");
    const dots = el("div", "dots");
    for (let i = 0; i < 5; i++) dots.appendChild(el("span", `dot${i === step ? " on" : ""}`));
    card.appendChild(dots);

    if (step === 0) {
      card.appendChild(el("h1", undefined, STRINGS.wizard.welcomeTitle));
      card.appendChild(el("p", undefined, STRINGS.wizard.welcomeBody));
      const next = el("button", "primary", STRINGS.wizard.welcomeNext);
      next.onclick = () => { step = 1; render(); };
      card.appendChild(footer(undefined, next));
    } else if (step === 1) {
      card.appendChild(el("h1", undefined, STRINGS.wizard.folderTitle));
      card.appendChild(el("p", undefined, STRINGS.wizard.folderBody));
      const chosen = el("p", "mono", settings.notes_dir ?? STRINGS.wizard.folderNone);
      const pick = el("button", "secondary", STRINGS.wizard.folderPick);
      pick.onclick = async () => {
        const dir = await api.pickFolder();
        if (dir) {
          settings = normalizeSettings(await api.setSettings({ notes_dir: dir }));
          chosen.textContent = settings.notes_dir ?? STRINGS.wizard.folderNone;
          next.disabled = !settings.notes_dir;
        }
      };
      card.appendChild(pick);
      card.appendChild(chosen);
      const back = el("button", "ghost", STRINGS.wizard.folderBack);
      back.onclick = () => { step = 0; render(); };
      const next = el("button", "primary", STRINGS.wizard.folderNext) as HTMLButtonElement;
      next.disabled = !settings.notes_dir;
      next.onclick = () => { step = 2; render(); };
      card.appendChild(footer(back, next));
    } else if (step === 2) {
      card.appendChild(el("h1", undefined, STRINGS.wizard.sensitivityTitle));
      card.appendChild(el("p", undefined, STRINGS.wizard.sensitivityBody));
      const list = el("div", "levels");
      for (const level of SENSITIVITY_LEVELS) {
        const row = el("label", `level${level === chosenSensitivity ? " on" : ""}`);
        const radio = el("input") as HTMLInputElement;
        radio.type = "radio";
        radio.name = "sensitivity";
        radio.value = level;
        radio.checked = level === chosenSensitivity;
        radio.onchange = () => { chosenSensitivity = level; render(); };
        const body = el("div", "level-body");
        body.appendChild(el("span", "level-name", level));
        body.appendChild(el("span", "level-desc", SENSITIVITY_DESCRIPTIONS[level]));
        row.appendChild(radio);
        row.appendChild(body);
        list.appendChild(row);
      }
      card.appendChild(list);
      const back = el("button", "ghost", STRINGS.wizard.sensitivityBack);
      back.onclick = () => { step = 1; render(); };
      const next = el("button", "primary", STRINGS.wizard.sensitivityConfirm);
      next.onclick = async () => {
        settings = normalizeSettings(await api.setSettings({ default_sensitivity: chosenSensitivity }));
        step = 3;
        render();
      };
      card.appendChild(footer(back, next));
    } else if (step === 3) {
      card.appendChild(el("h1", undefined, STRINGS.wizard.enableTitle));
      const toggleRow = el("label", "toggle-row");
      const toggle = el("input") as HTMLInputElement;
      toggle.type = "checkbox";
      toggle.checked = settings.enabled;
      const blocked = enableBlockedReason(settings);
      toggle.disabled = blocked !== null;
      toggle.onchange = async () => {
        settings = normalizeSettings(await api.setSettings({ enabled: toggle.checked }));
        if (settings.enabled) await api.startSidecar(); else await api.stopSidecar();
      };
      toggleRow.appendChild(toggle);
      toggleRow.appendChild(el("span", undefined, STRINGS.wizard.enableToggle));
      card.appendChild(toggleRow);
      card.appendChild(el("p", "notice", STRINGS.wizard.enableNotice));
      const back = el("button", "ghost", STRINGS.wizard.enableBack);
      back.onclick = () => { step = 2; render(); };
      const next = el("button", "primary", STRINGS.wizard.enableNext);
      next.onclick = () => { step = 4; render(); };
      card.appendChild(footer(back, next));
    } else {
      card.appendChild(el("h1", undefined, STRINGS.wizard.finishTitle));
      card.appendChild(el("p", undefined, STRINGS.wizard.finishBody));
      const done = el("button", "primary", STRINGS.wizard.finishDone);
      done.onclick = async () => {
        await api.completeWizard();
        await getCurrentWindow().close();
      };
      card.appendChild(footer(undefined, done));
    }
    root.appendChild(card);
  };

  const footer = (back?: HTMLElement, next?: HTMLElement): HTMLElement => {
    const f = el("div", "actions");
    if (back) f.appendChild(back);
    const spacer = el("div", "spacer");
    f.appendChild(spacer);
    if (next) f.appendChild(next);
    return f;
  };

  render();
}

// ============================ Settings ============================

async function renderSettings(root: HTMLElement): Promise<void> {
  let settings = normalizeSettings(await api.getSettings());

  const render = async () => {
    root.innerHTML = "";
    const status = await api.readStatus().catch(() => null);
    const supervisor = await api.supervisorStatus().catch(() => "stopped" as SupervisorStatus);
    const connect = await api.connectInfo().catch(() => null);

    const card = el("div", "card");
    card.appendChild(el("h1", undefined, STRINGS.settings.title));

    // Notes folder
    const folder = section(STRINGS.settings.folderLabel);
    folder.appendChild(el("p", "mono", settings.notes_dir ?? STRINGS.wizard.folderNone));
    const changeFolder = el("button", "secondary", STRINGS.settings.folderChange);
    changeFolder.onclick = async () => {
      const dir = await api.pickFolder();
      if (dir) {
        settings = normalizeSettings(await api.setSettings({ notes_dir: dir }));
        if (settings.enabled) await api.startSidecar();
        await render();
      }
    };
    folder.appendChild(changeFolder);
    card.appendChild(folder);

    // Default sensitivity
    const sens = section(STRINGS.settings.sensitivityLabel);
    sens.appendChild(el("p", "help", STRINGS.settings.sensitivityHelp));
    const select = el("select") as HTMLSelectElement;
    for (const level of SENSITIVITY_LEVELS) {
      const opt = el("option") as HTMLOptionElement;
      opt.value = level;
      opt.textContent = level + (level === "secret" ? " (fail-closed default)" : "");
      opt.selected = level === settings.default_sensitivity;
      select.appendChild(opt);
    }
    select.onchange = async () => {
      settings = normalizeSettings(await api.setSettings({ default_sensitivity: select.value as SensitivityLevel }));
      // Sensitivity is ctor-fixed in the engine: a restart IS the re-projection.
      if (settings.enabled) await api.startSidecar();
      await render();
    };
    sens.appendChild(select);
    card.appendChild(sens);

    // Enable toggle
    const enable = section(STRINGS.settings.enableLabel);
    enable.appendChild(el("p", "help", STRINGS.settings.enableHelp));
    const blocked = enableBlockedReason(settings);
    const toggleRow = el("label", "toggle-row");
    const toggle = el("input") as HTMLInputElement;
    toggle.type = "checkbox";
    toggle.checked = settings.enabled;
    toggle.disabled = blocked !== null;
    toggle.onchange = async () => {
      settings = normalizeSettings(await api.setSettings({ enabled: toggle.checked }));
      if (settings.enabled) await api.startSidecar(); else await api.stopSidecar();
      window.setTimeout(render, 400);
    };
    toggleRow.appendChild(toggle);
    toggleRow.appendChild(el("span", undefined, STRINGS.settings.enableLabel));
    enable.appendChild(toggleRow);
    if (blocked) enable.appendChild(el("p", "help", blocked));
    card.appendChild(enable);

    // Port
    const port = section(STRINGS.settings.portLabel);
    port.appendChild(el("p", "help", STRINGS.settings.portHelp));
    const portInput = el("input") as HTMLInputElement;
    portInput.type = "number";
    portInput.min = "1";
    portInput.max = "65535";
    portInput.value = String(settings.port);
    portInput.onchange = async () => {
      settings = normalizeSettings(await api.setSettings({ port: Number(portInput.value) }));
      if (settings.enabled) await api.startSidecar();
      await render();
    };
    port.appendChild(portInput);
    card.appendChild(port);

    // Status (read-only)
    const st = section(STRINGS.settings.statusHeading);
    if (supervisor === "error") st.appendChild(el("p", "notice error", STRINGS.settings.errorRestartsExhausted));
    st.appendChild(kv(STRINGS.settings.statusState, STRINGS.state[supervisor] ?? supervisor));
    st.appendChild(kv(STRINGS.settings.statusNotesIndexed, status ? String(status.notes_indexed) : "—"));
    st.appendChild(kv(STRINGS.settings.statusLastScan, status?.last_scan_iso ?? "—"));
    st.appendChild(kv(STRINGS.settings.statusEndpoint, status?.url ?? "—"));

    // Token reveal/copy
    if (connect) {
      const tokRow = el("div", "kv");
      tokRow.appendChild(el("span", "k", STRINGS.settings.tokenLabel));
      const val = el("span", "v mono", STRINGS.settings.tokenHidden);
      let revealed = false;
      const reveal = el("button", "link", STRINGS.settings.tokenReveal);
      reveal.onclick = () => {
        revealed = !revealed;
        val.textContent = revealed ? connect.token : STRINGS.settings.tokenHidden;
        reveal.textContent = revealed ? STRINGS.settings.tokenHide : STRINGS.settings.tokenReveal;
      };
      const copyTok = el("button", "link", STRINGS.settings.tokenCopy);
      copyTok.onclick = () => copy(connect.token, STRINGS.settings.tokenCopied);
      tokRow.appendChild(val);
      tokRow.appendChild(reveal);
      tokRow.appendChild(copyTok);
      st.appendChild(tokRow);
    }
    card.appendChild(st);

    // Quick connect
    const qc = section(STRINGS.connect.heading);
    qc.appendChild(el("p", "help", STRINGS.connect.intro));
    if (!connect || supervisor === "stopped" || supervisor === "error") {
      qc.appendChild(el("p", "help", STRINGS.connect.disabled));
    } else {
      const snips = allSnippets({ port: connect.port, token: connect.token });
      qc.appendChild(snippetBlock(STRINGS.connect.claudeCode, STRINGS.connect.claudeCodeDesc, snips.claudeCode));
      qc.appendChild(snippetBlock(STRINGS.connect.claudeJson, STRINGS.connect.claudeJsonDesc, snips.claudeJson));
      qc.appendChild(snippetBlock(STRINGS.connect.cursor, STRINGS.connect.cursorDesc, snips.cursor));
      qc.appendChild(snippetBlock(STRINGS.connect.toml, STRINGS.connect.tomlDesc, snips.toml));
    }
    card.appendChild(qc);

    root.appendChild(card);
  };

  const section = (title: string): HTMLElement => {
    const s = el("section", "sec");
    s.appendChild(el("h2", undefined, title));
    return s;
  };
  const kv = (k: string, v: string): HTMLElement => {
    const row = el("div", "kv");
    row.appendChild(el("span", "k", k));
    row.appendChild(el("span", "v", v));
    return row;
  };
  const snippetBlock = (title: string, desc: string, code: string): HTMLElement => {
    const b = el("div", "snippet");
    const head = el("div", "snippet-head");
    head.appendChild(el("strong", undefined, title));
    const btn = el("button", "secondary small", STRINGS.connect.copy);
    btn.onclick = () => copy(code, STRINGS.connect.copied);
    head.appendChild(btn);
    b.appendChild(head);
    b.appendChild(el("p", "help", desc));
    const pre = el("pre") as HTMLPreElement;
    pre.textContent = code;
    b.appendChild(pre);
    return b;
  };

  await render();
}

// ============================ Boot ============================

async function boot(): Promise<void> {
  const root = $("#app");
  const view = window.location.hash.replace("#", "") || "wizard";
  if (view === "settings") await renderSettings(root);
  else await renderWizard(root);
}

document.addEventListener("DOMContentLoaded", () => void boot());
