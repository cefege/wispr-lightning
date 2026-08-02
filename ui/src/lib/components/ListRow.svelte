<!--
  One row of an inset list with alternating row backgrounds.

  The stripe index is passed in rather than derived from `:nth-child` because
  History interleaves date headers between its rows, and the zebra has to
  ignore them and keep alternating across the whole list the way an AppKit
  `.inset(alternatesRowBackgrounds: true)` list does.

  Clickable and static rows are separate elements rather than one element with
  a conditional `role`: a row that opens an editor is a button and must be
  reachable and operable from the keyboard, and a row that does not is plain
  content that should not appear in the tab order at all.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    /** Position among data rows only, ignoring any headers. */
    index: number;
    /** Notes and Dictionary open an editor on a single click; History does not. */
    onclick?: () => void;
    oncontextmenu?: (event: MouseEvent) => void;
    children: Snippet;
  }

  let { index, onclick, oncontextmenu, children }: Props = $props();

  const classes = $derived(`row${index % 2 === 1 ? " alt" : ""}`);
</script>

{#if onclick}
  <div
    class="{classes} clickable"
    role="button"
    tabindex="0"
    {onclick}
    {oncontextmenu}
    onkeydown={(event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      onclick();
    }}
  >
    {@render children()}
  </div>
{:else}
  <!-- History rows have no context menu, only inline buttons (WIN-013), so a
       static row is inert content and takes no handlers at all. -->
  <div class={classes}>
    {@render children()}
  </div>
{/if}

<style>
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    padding: var(--space-1) var(--space-2);
  }

  .alt {
    background: var(--bg-selected);
  }

  .clickable:hover {
    background: var(--bg-control-hover);
  }
</style>
