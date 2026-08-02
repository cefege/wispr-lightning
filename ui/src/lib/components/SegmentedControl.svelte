<!--
  A segmented control (AppKit `Picker(.segmented)`).

  Implemented as a radio group rather than a row of buttons: a segmented
  control is a single-choice control, so it must be one tab stop with arrow
  keys moving between the options, not N tab stops. Native radios give that
  for free, so the paint is on the labels and the inputs stay real.
-->
<script lang="ts">
  interface Option {
    value: string;
    label: string;
  }

  interface Props {
    value: string;
    options: readonly Option[];
    /** Distinguishes the radio group from any other on the page. */
    name: string;
    disabled?: boolean;
    ariaLabel?: string;
    /** Id of the visible label when there is one, per the ARIA pattern. */
    ariaLabelledby?: string;
    onchange?: (value: string) => void;
  }

  let {
    value = $bindable(""),
    options,
    name,
    disabled = false,
    ariaLabel,
    ariaLabelledby,
    onchange,
  }: Props = $props();

  function select(next: string) {
    value = next;
    onchange?.(next);
  }
</script>

<div
  class="segmented"
  role="radiogroup"
  aria-label={ariaLabel}
  aria-labelledby={ariaLabelledby}
  aria-disabled={disabled ? "true" : undefined}
>
  {#each options as option (option.value)}
    <label class="segment" class:selected={value === option.value}>
      <input
        type="radio"
        {name}
        {disabled}
        value={option.value}
        checked={value === option.value}
        onchange={() => select(option.value)}
      />
      <span>{option.label}</span>
    </label>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    padding: 2px;
    background: var(--bg-selected);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .segment {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 64px;
    padding: 3px var(--space-2);
    font-size: var(--text-body);
    color: var(--text-primary);
    border-radius: var(--radius-sm);
    transition:
      background var(--duration-fast) var(--ease),
      color var(--duration-fast) var(--ease);
  }

  .segment:hover:not(.selected) {
    background: var(--bg-control-hover);
  }

  /* Accent-tinted rather than a raised neutral pill: in dark mode the control
     background and the track differ by a couple of percent, which left the
     selection ambiguous at a glance. macOS tints the selected segment too. */
  .segment.selected {
    color: var(--text-on-accent);
    background: var(--accent);
    box-shadow: var(--shadow-sm);
  }

  /* The input stays in the layout and keeps its focus ring; it just has no
     box of its own, so the ring lands on the segment. */
  .segment input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
  }

  .segment:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .segment:has(input:disabled) {
    opacity: 0.45;
  }

  .segmented:has(input:disabled) {
    pointer-events: none;
  }
</style>
