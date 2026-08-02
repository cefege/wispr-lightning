<!--
  A switch-style toggle.

  It is a real `<input type="checkbox">` under the paint: that is what makes it
  focusable, space-activatable and announceable without a line of ARIA
  plumbing, and it is what lets `<label for>` in SettingRow work. `role="switch"`
  is the one thing a checkbox cannot express on its own.
-->
<script lang="ts">
  interface Props {
    checked?: boolean;
    id?: string;
    disabled?: boolean;
    ariaLabel?: string;
    ariaDescribedby?: string;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked = $bindable(false),
    id,
    disabled = false,
    ariaLabel,
    ariaDescribedby,
    onchange,
  }: Props = $props();

  function handle(event: Event) {
    const next = (event.currentTarget as HTMLInputElement).checked;
    checked = next;
    onchange?.(next);
  }
</script>

<input
  class="switch"
  type="checkbox"
  role="switch"
  {id}
  {disabled}
  {checked}
  aria-label={ariaLabel}
  aria-describedby={ariaDescribedby}
  onchange={handle}
/>

<style>
  .switch {
    appearance: none;
    flex: none;
    position: relative;
    width: 30px;
    height: 18px;
    margin: 0;
    background: var(--border-strong);
    border-radius: var(--radius-lg);
    cursor: default;
    transition: background var(--duration-fast) var(--ease);
  }

  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    background: var(--bg-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
    transition: transform var(--duration-fast) var(--ease);
  }

  .switch:checked {
    background: var(--accent);
  }

  .switch:checked::after {
    transform: translateX(12px);
  }

  .switch:disabled {
    opacity: 0.45;
  }
</style>
