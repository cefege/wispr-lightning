<!--
  The modal primitive behind `ConfirmDialog` and `Sheet`.

  Built on the native `<dialog>` element rather than a hand-rolled overlay: it
  gives the focus trap, the inert background, the top-layer stacking and the
  Escape handling that a desktop modal has to have, all of which are easy to
  get subtly wrong by hand. Escape is routed through `oncancel` so a sheet's
  Cancel button and the Escape key run identical code.

  Clicking the backdrop does nothing, matching NSAlert and AppKit sheets: a
  half-typed note must not be thrown away by a stray click.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    open: boolean;
    /** Fixed width in px; the spec sizes every sheet and alert explicitly. */
    width: number;
    /** Fixed height in px, for the note editor's 500 x 400. Otherwise intrinsic. */
    height?: number;
    ariaLabel: string;
    oncancel: () => void;
    children: Snippet;
  }

  let { open, width, height, ariaLabel, oncancel, children }: Props = $props();

  let dialog: HTMLDialogElement | undefined = $state();

  $effect(() => {
    const el = dialog;
    if (el === undefined) return;
    if (open && !el.open) el.showModal();
    else if (!open && el.open) el.close();
  });
</script>

<dialog
  bind:this={dialog}
  class="modal"
  style:width="{width}px"
  style:height={height === undefined ? undefined : `${height}px`}
  aria-label={ariaLabel}
  oncancel={(event) => {
    // Let the caller decide what closing means; without this the element
    // closes itself and the caller's `open` flag goes stale.
    event.preventDefault();
    oncancel();
  }}
>
  {#if open}
    {@render children()}
  {/if}
</dialog>

<style>
  .modal {
    max-width: calc(100vw - var(--space-4));
    max-height: calc(100vh - var(--space-4));
    padding: 0;
    color: var(--text-primary);
    background: var(--bg-window);
    border: 0;
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
  }

  .modal::backdrop {
    background: rgb(0 0 0 / 30%);
  }
</style>
