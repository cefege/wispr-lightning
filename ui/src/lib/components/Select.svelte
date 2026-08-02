<!--
  A pop-up menu.

  A native `<select>`, because the platform's own menu is what the Swift
  `Picker(.menu)` was and because nothing hand-rolled matches it for keyboard
  handling, type-ahead or screen-reader support.

  Option values are strings; a setting whose value is nullable (the microphone
  id, the sound pack) maps `null` to the empty string at the call site, which
  keeps the null-handling visible rather than hidden in here.
-->
<script lang="ts">
  export interface SelectOption {
    value: string;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    value: string;
    options: readonly SelectOption[];
    id?: string;
    disabled?: boolean;
    ariaLabel?: string;
    /** Id of the explanatory text, when a `SettingRow` supplies one. */
    ariaDescribedby?: string;
    /** Stops a long device name from stretching the whole pane. */
    maxWidth?: string;
    onchange?: (value: string) => void;
  }

  let {
    value = $bindable(""),
    options,
    id,
    disabled = false,
    ariaLabel,
    ariaDescribedby,
    maxWidth,
    onchange,
  }: Props = $props();

  function handle(event: Event) {
    const next = (event.currentTarget as HTMLSelectElement).value;
    value = next;
    onchange?.(next);
  }
</script>

<select
  class="select"
  style:max-width={maxWidth}
  {id}
  {disabled}
  aria-label={ariaLabel}
  aria-describedby={ariaDescribedby}
  {value}
  onchange={handle}
>
  {#each options as option (option.value)}
    <option value={option.value} disabled={option.disabled}>{option.label}</option>
  {/each}
</select>

<style>
  .select {
    min-width: 0;
    padding: var(--space-1) var(--space-2);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    background: var(--bg-control);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-sm);
  }

  .select:disabled {
    opacity: 0.45;
  }
</style>
