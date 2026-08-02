<!--
  The first-launch walkthrough.

  Three required steps, each doing one job, are shown instead of the settings
  window until `didCompleteOnboarding` is true. The flow replaces the window
  rather than floating in a `<dialog>` over it: at first run there is nothing
  behind it worth seeing, and both platforms' setup flows take the whole
  window too.

  Three things make it behave like a dialog rather than a web page:

  - Return activates the primary button, unless the focused control has its own
    meaning for Return. That is the default-button behaviour of an AppKit sheet
    and a Win32 dialog, and it is what lets the whole flow be completed without
    a mouse.
  - Focus moves to the step heading on every transition, so the tab order
    restarts at the top and a screen reader announces where it landed.
  - Setup cannot be skipped. The permission step controls the primary button,
    and the backend independently refuses to persist completion while any
    required OS grant is missing.
-->
<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import ErrorBanner from "../lib/components/ErrorBanner.svelte";
  import HotkeyStep from "./HotkeyStep.svelte";
  import PermissionsStep from "./PermissionsStep.svelte";
  import DeepgramStep from "./DeepgramStep.svelte";
  import { completeOnboarding, describe, saveError, type Settings } from "../lib/ipc";

  interface Props {
    value: Settings;
  }

  let { value }: Props = $props();

  const STEPS = [
    { id: "permissions", title: "Approve access" },
    { id: "hotkey", title: "Your dictation key" },
    { id: "deepgram", title: "Connect Deepgram" },
  ] as const;

  let index = $state(0);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let heading = $state<HTMLHeadingElement>();
  /**
   * True while the hotkey step is recording a press. Return is a bindable
   * trigger key, so the wizard has to stop claiming it as its default action
   * for as long as the backend is listening for one.
   */
  let recording = $state(false);
  let permissionsReady = $state(false);
  let deepgramReady = $state(false);

  const step = $derived(STEPS[index] ?? STEPS[0]);
  const isLast = $derived(index === STEPS.length - 1);
  const canAdvance = $derived(
    step.id === "permissions"
      ? permissionsReady
      : step.id === "deepgram"
        ? deepgramReady
        : true,
  );

  $effect(() => {
    // Re-runs on every step change; `heading` is rebound by then.
    void index;
    heading?.focus({ preventScroll: true });
    // A capture abandoned by clicking Back or Continue never reports that it
    // stopped, so the flag is cleared by leaving the step rather than trusted
    // to unwind on its own.
    recording = false;
  });

  function back() {
    if (index > 0) index -= 1;
  }

  function next() {
    if (!canAdvance) return;
    if (isLast) void finish();
    else index += 1;
  }

  async function finish() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await completeOnboarding();
    } catch (cause) {
      // Left mounted on purpose: the flag did not persist, so pretending the
      // flow is over would just show the wizard again on the next launch with
      // no explanation of why.
      error = describe(cause);
      return;
    } finally {
      busy = false;
    }
  }

  /** Whether the focused element already owns the Return key. */
  function consumesReturn(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const el = target.closest("button, a[href], textarea, input, [contenteditable='true']");
    if (el === null) return false;
    if (el instanceof HTMLInputElement) return el.type !== "radio" && el.type !== "checkbox";
    return true;
  }

  function onkeydown(event: KeyboardEvent) {
    if (recording) return;
    if (event.key !== "Enter" || event.repeat || event.defaultPrevented) return;
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (consumesReturn(event.target)) return;
    event.preventDefault();
    next();
  }
</script>

<svelte:window {onkeydown} />

<div class="onboarding">
  <div class="sheet">
    <header class="head">
      <p class="counter">Step {index + 1} of {STEPS.length}</p>
      <!-- Focused programmatically on each step, hence the -1 tab stop. -->
      <h1 class="title" tabindex="-1" bind:this={heading}>{step.title}</h1>
    </header>

    <div class="body">
      {#if step.id === "permissions"}
        <PermissionsStep
          {value}
          onreadychange={(ready) => (permissionsReady = ready)}
        />
      {:else if step.id === "hotkey"}
        <HotkeyStep {value} oncapturingchange={(active) => (recording = active)} />
      {:else}
        <DeepgramStep onreadychange={(ready) => (deepgramReady = ready)} />
      {/if}
    </div>

    <footer class="foot">
      <!-- The settings window's save banner is not on screen while the wizard
           is, so a hotkey write failure has to be reported here. -->
      {#if $saveError}
        <ErrorBanner message="Could not save settings: {$saveError}" />
      {/if}

      {#if error}
        <ErrorBanner message={error} />
      {/if}

      <div class="buttons">
        <div class="spacer"></div>
        {#if index > 0}
          <Button size="regular" onclick={back}>Back</Button>
        {/if}
        <Button variant="accent" size="regular" disabled={busy || !canAdvance} onclick={next}>
          {isLast ? "Done" : "Continue"}
        </Button>
      </div>
    </footer>
  </div>
</div>

<style>
  .onboarding {
    display: flex;
    height: 100%;
    justify-content: center;
    background: var(--bg-window);
  }

  /* A single centred column, capped so the copy stays readable in a window
     the user is free to widen. */
  .sheet {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    max-width: 560px;
    min-height: 0;
    padding: 28px;
  }

  .head {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .counter {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .title {
    margin: 0;
    font-size: var(--text-title);
    font-weight: var(--weight-semibold);
  }

  /* No focus ring on the heading: the focus is a navigation aid for assistive
     tech, not something the user asked for by pressing Tab. */
  .title:focus {
    outline: none;
  }

  .body {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
    min-height: 0;
    padding: var(--space-4) 0;
    overflow-y: auto;
  }

  .foot {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  /* Default action rightmost, with Back beside it on later steps. */
  .buttons {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .spacer {
    flex: 1 1 auto;
  }
</style>
