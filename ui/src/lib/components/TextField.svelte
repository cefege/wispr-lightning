<!--
  A single-line text input.

  `type="password"` exists for the Deepgram API key, which is write-only: the
  backend never hands the key back, so the field is a submission box, not a
  bound value that round-trips.
-->
<script lang="ts">
  interface Props {
    value?: string;
    id?: string;
    type?: "text" | "password" | "search";
    placeholder?: string;
    disabled?: boolean;
    ariaLabel?: string;
    /** Only for fields with no visible <label>, e.g. an inline search box. */
    autocomplete?: "off" | "on";
    oninput?: (value: string) => void;
    onkeydown?: (event: KeyboardEvent) => void;
  }

  let {
    value = $bindable(""),
    id,
    type = "text",
    placeholder,
    disabled = false,
    ariaLabel,
    autocomplete = "off",
    oninput,
    onkeydown,
  }: Props = $props();

  function handle(event: Event) {
    const next = (event.currentTarget as HTMLInputElement).value;
    value = next;
    oninput?.(next);
  }
</script>

<input
  class="field"
  {id}
  {type}
  {placeholder}
  {disabled}
  {autocomplete}
  aria-label={ariaLabel}
  {value}
  oninput={handle}
  {onkeydown}
/>

<style>
  .field {
    width: 100%;
    min-width: 0;
    padding: var(--space-1) var(--space-2);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    background: var(--bg-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .field::placeholder {
    color: var(--text-tertiary);
  }

  .field:disabled {
    opacity: 0.45;
  }
</style>
