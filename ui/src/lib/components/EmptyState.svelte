<!--
  The centred placeholder a list pane shows instead of rows.

  One component covers all four non-list states — empty, no search results,
  loading and command-failed — because a pane must never be blank, and having
  a single shape for "there is nothing to show you, here is why" keeps the four
  from drifting apart across three windows.
-->
<script lang="ts">
  import Button from "./Button.svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    /** Omitted for the terse no-results state, which is text only. */
    icon?: IconName;
    title: string;
    description?: string;
    action?: { label: string; onclick: () => void };
  }

  let { icon, title, description, action }: Props = $props();
</script>

<div class="empty">
  {#if icon}
    <span class="glyph"><Icon name={icon} size={36} /></span>
  {/if}
  <p class="title" class:terse={icon === undefined}>{title}</p>
  {#if description}
    <p class="description">{description}</p>
  {/if}
  {#if action}
    <span class="action">
      <Button size="regular" onclick={action.onclick}>{action.label}</Button>
    </span>
  {/if}
</div>

<style>
  .empty {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: var(--space-2);
    align-items: center;
    justify-content: center;
    padding: var(--space-4);
    text-align: center;
  }

  .glyph {
    color: var(--text-tertiary);
  }

  .title {
    margin: 0;
    font-size: var(--text-title);
    color: var(--text-secondary);
  }

  /* "No results for …" is body-sized secondary text, not a title (WIN-022). */
  .title.terse {
    font-size: var(--text-body);
  }

  .description {
    max-width: 34ch;
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-tertiary);
  }

  .action {
    margin-top: var(--space-1);
  }
</style>
