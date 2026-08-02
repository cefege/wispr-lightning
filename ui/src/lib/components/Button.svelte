<!--
  A push button.

  Variants map onto the AppKit bezels the Swift app used: `standard` is the
  default rounded bezel, `accent` the default-action bezel, `borderless` the
  inline glyph buttons, `danger` the red minus-circle beside a keycap, and
  `destructive` the filled default button of a delete confirmation — filled
  because it is the default action, and red because the action is not
  reversible.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    variant?: "standard" | "accent" | "borderless" | "danger" | "destructive";
    size?: "small" | "regular";
    disabled?: boolean;
    /** Native tooltip; the spec names exact tooltip strings in places. */
    title?: string;
    ariaLabel?: string;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
  }

  let {
    variant = "standard",
    size = "small",
    disabled = false,
    title,
    ariaLabel,
    onclick,
    children,
  }: Props = $props();
</script>

<button
  type="button"
  class="btn {variant} {size}"
  {disabled}
  {title}
  aria-label={ariaLabel}
  {onclick}
>
  {@render children()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    font-family: inherit;
    font-size: var(--text-body);
    font-weight: var(--weight-regular);
    line-height: 1.2;
    color: var(--text-primary);
    background: var(--bg-control);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-sm);
    white-space: nowrap;
    transition:
      background var(--duration-fast) var(--ease),
      opacity var(--duration-fast) var(--ease);
  }

  .btn.small {
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-subheadline);
  }

  .btn.regular {
    padding: var(--space-2) var(--space-3);
  }

  .btn:hover:not(:disabled) {
    background: var(--bg-control-hover);
  }

  .btn:active:not(:disabled) {
    background: var(--bg-selected);
  }

  .btn:disabled {
    opacity: 0.45;
  }

  .btn.accent {
    color: var(--text-on-accent);
    background: var(--accent);
    border-color: transparent;
  }

  .btn.accent:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .btn.destructive {
    color: var(--text-on-accent);
    background: var(--danger);
    border-color: transparent;
  }

  /* Darkened by filter rather than by mixing in a literal colour, so the
     palette stays entirely in app.css. */
  .btn.destructive:hover:not(:disabled) {
    filter: brightness(0.92);
  }

  .btn.borderless,
  .btn.danger {
    background: none;
    border-color: transparent;
    box-shadow: none;
    padding: var(--space-1);
  }

  .btn.borderless:hover:not(:disabled),
  .btn.danger:hover:not(:disabled) {
    background: var(--bg-selected);
  }

  .btn.danger {
    color: var(--danger);
  }
</style>
