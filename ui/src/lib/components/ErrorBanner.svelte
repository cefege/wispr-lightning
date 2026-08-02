<!--
  An inline failure notice with an optional retry.

  Inline rather than modal on purpose: a backend that is slow to boot, or a
  command that is not wired up yet, must not block the rest of the window. The
  pane that failed says so; the panes that worked keep working.
-->
<script lang="ts">
  interface Props {
    message: string;
    /** Omit to render a notice with no action. */
    onretry?: () => void;
    retryLabel?: string;
  }

  let { message, onretry, retryLabel = "Retry" }: Props = $props();
</script>

<div class="banner" role="alert">
  <span class="message">{message}</span>
  {#if onretry}
    <button type="button" class="retry" onclick={onretry}>{retryLabel}</button>
  {/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2);
    font-size: var(--text-subheadline);
    color: var(--text-primary);
    background: var(--bg-elevated);
    border: 1px solid var(--danger);
    border-radius: var(--radius-sm);
  }

  .message {
    flex: 1 1 auto;
    min-width: 0;
    color: var(--danger);
  }

  .retry {
    flex: none;
    padding: 2px var(--space-2);
    font-family: inherit;
    font-size: var(--text-subheadline);
    color: var(--text-primary);
    background: var(--bg-control);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .retry:hover {
    background: var(--bg-control-hover);
  }
</style>
