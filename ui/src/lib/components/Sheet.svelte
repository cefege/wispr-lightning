<!--
  A modal editing sheet: title, body, then a Cancel / confirm footer.

  Escape maps to Cancel and Return to the confirm action, as the SwiftUI
  `.cancelAction` / `.defaultAction` keyboard shortcuts did. Return is bound at
  the form level so it fires from any single-line field, but a `<textarea>`
  keeps its own newline behaviour — a note body and a snippet expansion are
  multi-line by definition.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";

  interface Props {
    open: boolean;
    /** Accessible name, and the visible heading unless `headingHidden`. */
    title: string;
    /**
     * The note editor has no visible heading — it opens straight onto the
     * title field — while every dictionary sheet does.
     */
    headingHidden?: boolean;
    width: number;
    height?: number;
    /**
     * Gap between the sheet's rows, in px. The dictionary sheets are
     * `VStack(spacing: 16)` and the note editor is `VStack(spacing: 8)`.
     */
    spacing?: number;
    /** "Add" or "Save"; the sheets differ. */
    confirmLabel: string;
    confirmDisabled?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
    children: Snippet;
  }

  let {
    open,
    title,
    headingHidden = false,
    width,
    height,
    spacing = 16,
    confirmLabel,
    confirmDisabled = false,
    onconfirm,
    oncancel,
    children,
  }: Props = $props();
</script>

<Modal {open} {width} {height} ariaLabel={title} {oncancel}>
  <!-- Not a <form>: there is nothing to submit, the buttons are all
       type="button", and implicit submission would not fire anyway in a sheet
       with two text fields. Return is bound explicitly instead. -->
  <div
    class="sheet"
    style:gap="{spacing}px"
    role="presentation"
    onkeydown={(event) => {
      // A textarea keeps its own newline: a note body and a snippet expansion
      // are multi-line by definition.
      if (event.key !== "Enter" || event.target instanceof HTMLTextAreaElement) return;
      event.preventDefault();
      if (!confirmDisabled) onconfirm();
    }}
  >
    {#if !headingHidden}
      <h2 class="title">{title}</h2>
    {/if}
    <div class="body">{@render children()}</div>
    <div class="footer">
      <Button size="regular" onclick={oncancel}>Cancel</Button>
      <span class="spacer"></span>
      <Button variant="accent" size="regular" disabled={confirmDisabled} onclick={onconfirm}>
        {confirmLabel}
      </Button>
    </div>
  </div>
</Modal>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    /* Overridden per sheet; see the `spacing` prop. */
    gap: var(--space-3);
    height: 100%;
    padding: var(--space-4);
  }

  .title {
    margin: 0;
    font-size: var(--text-title);
    font-weight: var(--weight-semibold);
  }

  .body {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: inherit;
    min-height: 0;
  }

  .footer {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .spacer {
    flex: 1 1 auto;
  }
</style>
