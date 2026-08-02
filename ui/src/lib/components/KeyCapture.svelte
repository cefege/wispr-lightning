<!--
  The hotkey list editor: one keycap per binding, a remove button when there
  is more than one, and a button that records a new binding.

  Capture happens in the backend, not here. On macOS the Fn key never reaches
  a webview `keydown`, so webview-side capture cannot express every valid
  dictation binding. The webview
  therefore asks the platform hotkey backend to record the next press and
  polls for the result: `hotkey_capture_begin` is idempotent and
  `hotkey_capture_end` leaves the capture armed until it actually has
  something, so re-arming between polls cannot drop a press.

  Two invariants the Swift version had and this keeps:
  - a press that duplicates an existing binding silently cancels the capture;
  - a capture that yields nothing leaves the existing bindings untouched. It
    is never a way to clear a binding.
-->
<script lang="ts">
  import Button from "./Button.svelte";
  import { hotkeyCaptureBegin, hotkeyCaptureEnd, describe, type Hotkey } from "../ipc";
  import { hotkeyLabel, sameHotkey } from "../hotkey";

  interface Props {
    hotkeys: readonly Hotkey[];
    addLabel: string;
    removeTooltip: string;
    /** Names the list for assistive tech, since the keycaps are not a form. */
    ariaLabel: string;
    onchange: (hotkeys: Hotkey[]) => void;
    /**
     * Raised when recording starts and stops. A host that gives Return a
     * meaning of its own — the onboarding wizard's default button — has to
     * stand down while a key is being recorded, because Return is itself a
     * bindable trigger key.
     */
    oncapturingchange?: (capturing: boolean) => void;
  }

  let {
    hotkeys,
    addLabel,
    removeTooltip,
    ariaLabel,
    onchange,
    oncapturingchange,
  }: Props = $props();

  let capturing = $state(false);
  let error = $state<string | null>(null);

  function setCapturing(next: boolean) {
    capturing = next;
    oncapturingchange?.(next);
  }

  /** How often to ask the backend whether a press has landed. */
  const POLL_MS = 120;
  /**
   * Give up rather than leave the hotkey backend suppressed forever. The
   * backend deliberately has no timer of its own, so this one is the only one
   * and cannot race a second.
   */
  const TIMEOUT_MS = 15_000;

  let cancelRequested = false;

  function remove(index: number) {
    onchange(hotkeys.filter((_, i) => i !== index));
  }

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  async function capture() {
    if (capturing) {
      // Second click on the button cancels: end the capture, drop the result.
      cancelRequested = true;
      return;
    }

    setCapturing(true);
    cancelRequested = false;
    error = null;

    const deadline = Date.now() + TIMEOUT_MS;
    try {
      await hotkeyCaptureBegin();
      while (!cancelRequested && Date.now() < deadline) {
        await sleep(POLL_MS);
        const captured = await hotkeyCaptureEnd();
        if (cancelRequested) break;
        if (captured !== null) {
          // Silently cancel on a duplicate, exactly as the Swift version did:
          // the user pressed a key that is already bound, so nothing changes.
          if (!hotkeys.some((existing) => sameHotkey(existing, captured))) {
            onchange([...hotkeys, captured]);
          }
          return;
        }
        // Nothing recorded yet, and the capture is still armed. Re-arming is a
        // no-op that keeps us honest if the backend ever disarms early.
        await hotkeyCaptureBegin();
      }
      // Cancelled or timed out. One final end() disarms the backend so it
      // stops suppressing the real hotkey handler; anything it returns at that
      // moment is discarded, because the user has already walked away.
      await hotkeyCaptureEnd();
    } catch (cause) {
      error = describe(cause);
    } finally {
      setCapturing(false);
      cancelRequested = false;
    }
  }
</script>

<div class="capture">
  <ul class="keys" aria-label={ariaLabel}>
    {#each hotkeys as hotkey, index (hotkeyLabel(hotkey) + index)}
      <li class="key">
        <span class="cap">{hotkeyLabel(hotkey)}</span>
        {#if hotkeys.length > 1}
          <Button
            variant="danger"
            title={removeTooltip}
            ariaLabel="{removeTooltip}: {hotkeyLabel(hotkey)}"
            onclick={() => remove(index)}
          >
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
              <circle cx="8" cy="8" r="6.4" />
              <path d="M5.2 8h5.6" />
            </svg>
          </Button>
        {/if}
      </li>
    {/each}
  </ul>

  <div>
    <Button onclick={capture}>{capturing ? "Press a key…" : addLabel}</Button>
  </div>

  {#if capturing}
    <p class="hint" role="status">
      Press the key you want. For a combination, hold the modifiers and press the other key. Click
      again to cancel.
    </p>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
</div>

<style>
  .capture {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
  }

  .keys {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .key {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .cap {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 40px;
    padding: 6px 12px;
    font-family: var(--font-mono);
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
    background: var(--bg-control);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .key svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
  }

  .hint {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .error {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--danger);
  }
</style>
