<!--
  The in-webview stand-in for `NSAlert`.

  Button order follows the macOS original — the action first, Cancel second —
  even on Windows, where convention is the reverse. That is a deliberate call
  recorded in MATRIX WIN-016: the wording and the order are both part of the
  parity contract, and one app behaving the same way on both platforms beats
  two subtly different confirmation flows.
-->
<script lang="ts">
  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";

  interface Props {
    open: boolean;
    /** `NSAlert.messageText` — the bold first line. */
    message: string;
    /** `NSAlert.informativeText`. */
    informative?: string;
    /** Title of the first, default button. */
    confirmLabel: string;
    /** `.critical` alerts get the destructive treatment; `.warning` does not. */
    destructive?: boolean;
    /**
     * False for a plain acknowledgement such as the CSV import report, which
     * has nothing to cancel — an alert with one button is `NSAlert` with one
     * button, not a confirmation with a redundant escape hatch.
     */
    showCancel?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let {
    open,
    message,
    informative,
    confirmLabel,
    destructive = false,
    showCancel = true,
    onconfirm,
    oncancel,
  }: Props = $props();
</script>

<Modal {open} width={340} ariaLabel={message} {oncancel}>
  <div class="alert">
    <p class="message">{message}</p>
    {#if informative}
      <p class="informative">{informative}</p>
    {/if}
    <div class="buttons">
      <!-- Filled, not borderless: this is the default button, and it has to
           read as more prominent than Cancel beside it. -->
      <Button variant={destructive ? "destructive" : "accent"} size="regular" onclick={onconfirm}>
        {confirmLabel}
      </Button>
      {#if showCancel}
        <Button size="regular" onclick={oncancel}>Cancel</Button>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .alert {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
  }

  .message {
    margin: 0;
    font-size: var(--text-headline);
    font-weight: var(--weight-semibold);
  }

  .informative {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
    /* The import report is several lines: "Imported N entries with K errors:"
       followed by up to five error lines (WIN-048). */
    white-space: pre-line;
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
    margin-top: var(--space-2);
  }
</style>
