/**
 * Rendering a serialized `Hotkey` as a human label.
 *
 * This duplicates `wl_core::settings::hotkey::Modifiers::label()`. It would be
 * better to render a server-provided string, but the IPC surface has no
 * label command and `hotkey_capture_end` hands back a structural `Hotkey`, so
 * the mapping has to exist on this side too. The table below is a literal copy
 * of the `modifiers!` macro invocation in `hotkey.rs`, in the same declaration
 * order — `Modifiers::label()` joins in that order and so does this.
 */

import type { Hotkey, ModifierName, TriggerKey } from "./ipc";
import { isWindows } from "./platform";

/** `[name, macOS label, Windows label]`, in `hotkey.rs` declaration order. */
const MODIFIERS: ReadonlyArray<readonly [ModifierName, string, string]> = [
  ["ctrl_left", "Left Control", "Left Ctrl"],
  ["ctrl_right", "Right Control", "Right Ctrl"],
  ["alt_left", "Left Option", "Left Alt"],
  ["alt_right", "Right Option", "Right Alt"],
  ["meta_left", "Left Command", "Left Win"],
  ["meta_right", "Right Command", "Right Win"],
  ["shift_left", "Left Shift", "Left Shift"],
  ["shift_right", "Right Shift", "Right Shift"],
  ["fn", "Fn", "Fn"],
];

function triggerKeyLabel(key: TriggerKey): string {
  if (typeof key === "string") return key.charAt(0).toUpperCase() + key.slice(1);
  return `F${key.F}`;
}

function modifierLabels(modifiers: readonly ModifierName[]): string[] {
  return MODIFIERS.filter(([name]) => modifiers.includes(name)).map(([, mac, win]) =>
    isWindows ? win : mac,
  );
}

/** Matches `Hotkey::label()`, including the `Unset` fallback. */
export function hotkeyLabel(hotkey: Hotkey): string {
  const mods = modifierLabels(hotkey.modifiers).join(" + ");
  if (hotkey.key === null) return mods === "" ? "Unset" : mods;
  const key = triggerKeyLabel(hotkey.key);
  return mods === "" ? key : `${mods} + ${key}`;
}

/**
 * Identity for duplicate detection. Two hotkeys are the same binding when they
 * carry the same modifier set and the same trigger key, so the settings pane
 * can refuse to add a key that is already bound.
 */
export function hotkeyKey(hotkey: Hotkey): string {
  const mods = MODIFIERS.filter(([name]) => hotkey.modifiers.includes(name))
    .map(([name]) => name)
    .join("+");
  const key =
    hotkey.key === null ? "" : typeof hotkey.key === "string" ? hotkey.key : `F${hotkey.key.F}`;
  return `${mods}/${key}`;
}

export function sameHotkey(a: Hotkey, b: Hotkey): boolean {
  return hotkeyKey(a) === hotkeyKey(b);
}
