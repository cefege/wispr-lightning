/**
 * The single typed boundary between the webview and the Rust backend.
 *
 * Everything here mirrors a Rust type by hand rather than by codegen. That is
 * deliberate: the on-disk settings keys are the ones the Swift app wrote (a
 * mix of camelCase and snake_case that nobody would choose fresh), and a
 * generator would happily rename them into something tidier and orphan every
 * existing install. Hand-written means the drift is visible in review.
 *
 * Two rules hold for every wrapper below:
 *
 * 1. A failed command throws an {@link IpcError} carrying the backend's own
 *    message. Nothing here swallows an error into a default value, because a
 *    settings pane that silently shows defaults is worse than one that says
 *    it could not load.
 * 2. Nothing here assumes the backend exists. Opened in a plain browser (or
 *    against a backend that has not finished booting) the calls reject and
 *    the event subscriptions become no-ops, so the UI degrades to an error
 *    state instead of a blank window.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { writable, type Readable } from "svelte/store";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/** A command that returned `Err(String)`, or could not be reached at all. */
export class IpcError extends Error {
  constructor(
    readonly command: string,
    message: string,
  ) {
    super(message);
    this.name = "IpcError";
  }
}

/**
 * Tauri rejects with whatever the command's error type serialized to — a bare
 * string for our `Result<T, String>` commands, but an `Error` or an arbitrary
 * object when the failure happened before the command ran.
 */
export function describe(cause: unknown): string {
  if (typeof cause === "string") return cause;
  if (cause instanceof Error) return cause.message;
  if (cause && typeof cause === "object" && "message" in cause) {
    return String((cause as { message: unknown }).message);
  }
  return String(cause);
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    throw new IpcError(command, describe(cause));
  }
}

// ---------------------------------------------------------------------------
// Hotkeys — mirrors wl_core::settings::hotkey
// ---------------------------------------------------------------------------

/** Side-specific modifier names serialized by `Modifiers`. */
export type ModifierName =
  | "ctrl_left"
  | "ctrl_right"
  | "alt_left"
  | "alt_right"
  | "meta_left"
  | "meta_right"
  | "shift_left"
  | "shift_right"
  | "fn";

/** `TriggerKey`: unit variants are snake_case strings, `F(u8)` is a wrapper. */
export type TriggerKey = "return" | "space" | "escape" | "tab" | { F: number };

export interface Hotkey {
  modifiers: ModifierName[];
  /** `null` means a bare modifier hold, which is the common configuration. */
  key: TriggerKey | null;
}

// ---------------------------------------------------------------------------
// Settings — mirrors wl_core::settings::Settings
//
// Field names are the JSON keys, which are the legacy on-disk keys. Fields
// without an explicit `#[serde(rename)]` on the Rust side stay snake_case;
// renamed ones are camelCase. Do not "fix" the inconsistency.
// ---------------------------------------------------------------------------


/** What a quick tap of the hotkey means. `wl_core::fsm::PressBehavior`. */
export type PressBehaviorValue = "hold" | "toggle" | "legacy";


export type TypingSpeed = "slow" | "normal" | "expert";
export type EmailSignatureOption = "written_with_lightning" | "spoken_with_lightning";

export interface Settings {
  hotkeys: Hotkey[];
  hotkeyKeyCodes: number[];
  hotkeyPaused: boolean;
  hotkeyTapToToggle: boolean;
  hotkeyPressBehavior: PressBehaviorValue;

  micDeviceId: string | null;
  micDeviceName: string | null;
  keepMicrophoneActive: boolean;
  enableSounds: boolean;
  selectedSoundPack: string | null;
  muteMusic: boolean;

  deepgramModel: string;
  deepgramKeytermBoost: boolean;
  commandModeEnabled: boolean;
  useScreenContext: boolean;
  useAccessibilityContext: boolean;
  autoLearnWords: boolean;
  deepgramLanguage: string;

  naturalModeEnabled: boolean;
  naturalModeSpeed: TypingSpeed;
  emailAutoSignature: boolean;
  emailSignatureOption: EmailSignatureOption;

  launchAtLogin: boolean;
  showInDock: boolean;
  shareUsageData: boolean;
  verboseLogging: boolean;
  didCompleteOnboarding: boolean;
}

// ---------------------------------------------------------------------------
// Other payload types
// ---------------------------------------------------------------------------

export interface InputDevice {
  id: string;
  name: string;
  is_default: boolean;
}

export interface DeepgramStatus {
  configured: boolean;
}

export interface DeepgramHealth {
  ok: boolean;
  message: string;
}

export interface DeepgramBalance {
  amount: number;
  units: string;
  projectName: string;
}

/**
 * The db model types below are camelCase and expose only the columns the UI
 * may see: vestigial columns (`team_dictionary_id`, `last_used`) and the
 * soft-delete flags are filtered out server-side. Timestamps are Unix epoch
 * *seconds* as floats, so `new Date(ts * 1000)`.
 */
export interface TranscriptEntry {
  id: string;
  asrText: string | null;
  formattedText: string | null;
  timestamp: number;
  appName: string;
  appBundleId: string;
  durationSecs: number;
  numWords: number;
  language: string;
}

export type DictionaryKind = "vocabulary" | "snippets";

export interface DictionaryEntry {
  id: string;
  phrase: string;
  replacement: string | null;
  isSnippet: boolean;
  manualEntry: boolean;
  source: string | null;
  frequencyUsed: number;
  createdAt: number;
  modifiedAt: number;
}

export interface NoteEntry {
  id: string;
  title: string;
  /** Derived server-side from the first 200 characters; never sent back. */
  contentPreview: string;
  content: string;
  createdAt: number;
  modifiedAt: number;
}

/**
 * Outcome of a dictionary CSV import. Rows that failed are reported rather
 * than dropped, so a partially bad file tells the user which lines it choked
 * on instead of silently importing fewer entries than the file contained.
 */
export interface CsvImport {
  imported: number;
  errors: string[];
}

/** `src-tauri::ui::OverlayState`, externally tagged. */
export type OverlayState =
  | "Hidden"
  | "Recording"
  | "Locked"
  | "Processing"
  | "Inserting"
  | { Retrying: { attempt: number; of: number } }
  | { Error: { message: string } }
  | { Recoverable: { message: string } };

export interface Elapsed {
  label: string | null;
  /** 0 none, 1 approaching the limit, 2 about to be cut off. */
  warning: number;
}

export type PermissionState = "granted" | "denied" | "not_determined" | "not_applicable";

export type OverlayAction = "retry" | "save" | "dismiss";
export type SoundCue = "start" | "stop";
export type WindowName = "settings" | "history" | "dictionary" | "notes";

/**
 * `src-tauri::commands::AccentColor`. Every field is `#rrggbb`.
 *
 * `darker` and `lighter` are the hover and pressed shades for the light and
 * dark appearances; the stylesheet picks between them on
 * `prefers-color-scheme`, which is the one thing the backend cannot know.
 */
export interface AccentColor {
  accent: string;
  text: string;
  darker: string;
  lighter: string;
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

export const settingsGet = () => call<Settings>("settings_get");
export const settingsSave = (settings: Settings) => call<Settings>("settings_save", { settings });

export const audioDevices = () => call<InputDevice[]>("audio_devices");
export const soundPreview = (pack: string | null, cue: SoundCue) =>
  call<null>("sound_preview", { pack, cue });

/**
 * Bundled sound-pack directory names.
 *
 * Additive to the original command list. Callers must tolerate a rejection:
 * a build whose backend predates this command should degrade to the single
 * `Default` pack, which is the same thing the Swift app showed when the
 * Sounds folder was missing.
 */
export const soundPacks = () => call<string[]>("sound_packs");

export const deepgramStatus = () => call<DeepgramStatus>("deepgram_status");
export const deepgramHealth = () => call<DeepgramHealth>("deepgram_health");
export const deepgramBalance = () => call<DeepgramBalance>("deepgram_balance");
export const deepgramKeySave = (key: string) => call<null>("deepgram_key_save", { key });
export const deepgramKeyClear = () => call<null>("deepgram_key_clear");


export const historyList = (limit: number, offset: number) =>
  call<TranscriptEntry[]>("history_list", { limit, offset });
export const historySearch = (query: string) =>
  call<TranscriptEntry[]>("history_search", { query });
export const historyDelete = (id: string) => call<null>("history_delete", { id });
export const historyClear = () => call<null>("history_clear");

export const dictionaryList = (kind: DictionaryKind) =>
  call<DictionaryEntry[]>("dictionary_list", { kind });
export const dictionaryAdd = (entry: DictionaryEntry) =>
  call<DictionaryEntry>("dictionary_add", { entry });
export const dictionaryUpdate = (entry: DictionaryEntry) =>
  call<null>("dictionary_update", { entry });
export const dictionaryDelete = (id: string) => call<null>("dictionary_delete", { id });
export const dictionaryImportCsv = (path: string) =>
  call<CsvImport>("dictionary_import_csv", { path });

export const notesList = (query: string | null) => call<NoteEntry[]>("notes_list", { query });
export const notesAdd = (title: string, content: string) =>
  call<NoteEntry>("notes_add", { title, content });
export const notesUpdate = (id: string, title: string, content: string) =>
  call<null>("notes_update", { id, title, content });
export const notesDelete = (id: string) => call<null>("notes_delete", { id });

export const hotkeyCaptureBegin = () => call<null>("hotkey_capture_begin");
export const hotkeyCaptureEnd = () => call<Hotkey | null>("hotkey_capture_end");
export const hotkeySetPaused = (paused: boolean) => call<null>("hotkey_set_paused", { paused });

export const permissionsStatus = () => call<Record<string, PermissionState>>("permissions_status");
export const permissionsRequest = (permission: string) =>
  call<null>("permissions_request", { permission });
export const permissionsOpenSettings = (permission: string) =>
  call<null>("permissions_open_settings", { permission });

export const overlayAction = (action: OverlayAction) => call<null>("overlay_action", { action });

export const windowOpen = (name: WindowName) => call<null>("window_open", { name });
export const appQuit = () => call<null>("app_quit");

/**
 * The OS accent colour, or `null` where the platform would not report one.
 *
 * Native rather than the CSS `AccentColor` keyword, which both engines answer
 * with a hardcoded blue while still claiming support for it. Applied by
 * `main.ts`; no component should need to call this.
 */
export const accentColor = () => call<AccentColor | null>("accent_color");

/**
 * Copy through the clipboard plugin rather than `navigator.clipboard`, which
 * needs a secure context that the custom protocol does not reliably provide
 * on Windows.
 */
export async function copyText(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch (cause) {
    throw new IpcError("clipboard.writeText", describe(cause));
  }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/**
 * Subscribe without letting a missing backend take the window down.
 *
 * `listen` rejects when the IPC bridge is absent, and an unhandled rejection
 * inside a component's setup would leave the pane half-mounted. Returning a
 * no-op unsubscriber keeps callers' teardown code uniform.
 */
function subscribe<T>(event: string, handler: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | null = null;
  let cancelled = false;

  listen<T>(event, (e) => handler(e.payload))
    .then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    })
    .catch(() => {
      /* No bridge: the pane still works, it just never hears about changes. */
    });

  return () => {
    cancelled = true;
    unlisten?.();
    unlisten = null;
  };
}

export const onOverlayState = (h: (s: OverlayState) => void) =>
  subscribe<OverlayState>("overlay:state", h);
export const onOverlayElapsed = (h: (e: Elapsed) => void) =>
  subscribe<Elapsed>("overlay:elapsed", h);
export const onSettingsChanged = (h: (s: Settings) => void) =>
  subscribe<Settings>("settings:changed", h);
export const onDevicesChanged = (h: () => void) => subscribe<null>("devices:changed", h);
export const onHistoryChanged = (h: () => void) => subscribe<null>("history:changed", h);
export const onDictionaryChanged = (h: () => void) => subscribe<null>("dictionary:changed", h);
export const onSystemAccent = (h: (a: AccentColor) => void) =>
  subscribe<AccentColor>("system:accent", h);

// ---------------------------------------------------------------------------
// The settings store
// ---------------------------------------------------------------------------

/**
 * Loading is a three-state affair and the UI has to render all three. Folding
 * "failed" into "null" would make a backend outage look like an empty config,
 * which is exactly the confusion this app cannot afford.
 */
export type SettingsStatus =
  | { state: "loading" }
  | { state: "ready"; value: Settings }
  | { state: "error"; message: string };

const store = writable<SettingsStatus>({ state: "loading" });

/** Read-only view. Mutate through {@link updateSettings}, never by `set`. */
export const settings: Readable<SettingsStatus> = { subscribe: store.subscribe };

/** Non-fatal save failures, shown as an inline banner rather than a dialog. */
const saveErrorStore = writable<string | null>(null);
export const saveError: Readable<string | null> = { subscribe: saveErrorStore.subscribe };

/** The Swift app wrote settings.json on every keystroke. This is that, coalesced. */
const SAVE_DEBOUNCE_MS = 250;

let current: Settings | null = null;
let loadStarted = false;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
/**
 * Serialized form of the last payload exchanged with the backend. The backend
 * echoes `settings:changed` after every write, including our own; without this
 * we would round-trip our own edit back into the store and fight the user.
 */
let lastSent: string | null = null;

function publish(value: Settings): void {
  current = value;
  store.set({ state: "ready", value });
}

/** Loads once per window. Repeat calls are harmless and cheap. */
export async function loadSettings(): Promise<void> {
  if (loadStarted) return;
  loadStarted = true;
  try {
    const loaded = await settingsGet();
    lastSent = JSON.stringify(loaded);
    publish(loaded);
  } catch (err) {
    loadStarted = false; // Allow an explicit retry from the error state.
    store.set({ state: "error", message: describe(err) });
  }
}

/** Discards the cached state and reloads. Bound to the error pane's Retry. */
export async function reloadSettings(): Promise<void> {
  loadStarted = false;
  store.set({ state: "loading" });
  await loadSettings();
}

async function flush(): Promise<void> {
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  if (current === null) return;
  const payload = current;
  lastSent = JSON.stringify(payload);
  try {
    // The backend returns the settings it actually persisted (migration and
    // clamping happen there), so adopt its answer rather than assuming ours.
    const saved = await settingsSave(payload);
    lastSent = JSON.stringify(saved);
    publish(saved);
    saveErrorStore.set(null);
  } catch (err) {
    saveErrorStore.set(describe(err));
  }
}

/**
 * Apply an edit locally, then persist it after the user stops typing.
 *
 * The mutator receives a shallow-cloned draft; nested objects it intends to
 * change must be replaced, not mutated in place, so Svelte sees a new value.
 */
export function updateSettings(mutate: (draft: Settings) => void): void {
  if (current === null) return;
  const draft: Settings = { ...current };
  mutate(draft);
  publish(draft);

  if (saveTimer !== null) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    void flush();
  }, SAVE_DEBOUNCE_MS);
}

/**
 * Persist immediately, bypassing the debounce.
 *
 * Needed wherever a side effect must observe the saved file rather than the
 * in-memory draft — the sound-pack Preview button being the case that made
 * the Swift app add its own 200 ms delay.
 */
export function saveSettingsNow(): Promise<void> {
  return flush();
}

/**
 * Adopt settings changed outside this window (the tray's Natural Mode item,
 * another window, or a hand-edited file). Skipped while a save is queued, and
 * skipped for the echo of our own write.
 */
export function watchExternalSettings(): () => void {
  return onSettingsChanged((incoming) => {
    if (saveTimer !== null) return;
    const encoded = JSON.stringify(incoming);
    if (encoded === lastSent) return;
    lastSent = encoded;
    publish(incoming);
  });
}

/**
 * Mark the required first-launch walkthrough complete.
 *
 * The pending hotkey save is flushed first so the backend's onboarding flag
 * is the last write and cannot be undone by the debounce.
 *
 * The answer is adopted here rather than left to `settings:changed`, so the
 * store already reflects it by the time this resolves.
 */
export async function completeOnboarding(): Promise<void> {
  await flush();
  const saved = await call<Settings>("onboarding_complete");
  lastSent = JSON.stringify(saved);
  publish(saved);
}

/**
 * Re-arm the walkthrough, for the System pane's "Run setup again".
 *
 * Persisted immediately rather than on the debounce, so quitting straight
 * after asking for it still brings the wizard back.
 */
export function restartOnboarding(): Promise<void> {
  updateSettings((draft) => {
    draft.didCompleteOnboarding = false;
  });
  return flush();
}
