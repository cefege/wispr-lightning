<!--
  The shell shared by the History, Notes and Dictionary panes: a toolbar row, an
  optional failure banner, and a scrolling body.

  Each of the three has to render four content states — loading, empty, populated
  and command-failed — and the rule that a pane is never blank is only worth
  anything if it is enforced in one place. The banner is deliberately separate
  from the body: a delete that fails must not wipe out the rows the user is
  looking at.

  Every pane fills its container, so the same component works both embedded in
  the settings detail pane and alone in its own window.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    toolbar: Snippet;
    children: Snippet;
    /** Last command failure, shown above the list until it is dismissed. */
    error?: string | null;
    onretry?: () => void;
    ondismisserror?: () => void;
    /** Called as the body scrolls, for the infinite-scroll pagination. */
    onscroll?: (event: Event & { currentTarget: HTMLDivElement }) => void;
  }

  let { toolbar, children, error = null, onretry, ondismisserror, onscroll }: Props = $props();
</script>

<div class="pane">
  <header class="toolbar">{@render toolbar()}</header>

  {#if error}
    <div class="banner" role="alert">
      <Icon name="warning" />
      <span class="banner-text">{error}</span>
      {#if onretry}
        <Button onclick={onretry}>Retry</Button>
      {/if}
      {#if ondismisserror}
        <Button variant="borderless" ariaLabel="Dismiss" onclick={ondismisserror}>
          <Icon name="close" />
        </Button>
      {/if}
    </div>
  {/if}

  <div class="body" {onscroll}>{@render children()}</div>
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg-content);
  }

  .toolbar {
    display: flex;
    flex: none;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-1) var(--space-2);
  }

  .banner {
    display: flex;
    flex: none;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-1) var(--space-2);
    color: var(--danger);
    background: var(--bg-window);
    border-block: 1px solid var(--border);
  }

  .banner-text {
    flex: 1 1 auto;
    min-width: 0;
    font-size: var(--text-subheadline);
  }

  .body {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
  }
</style>
