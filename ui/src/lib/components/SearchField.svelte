<!--
  The inline search box used by History, Notes and Dictionary.

  Distinct from `TextField`: the Swift app drew this one by hand — a
  magnifying-glass glyph beside a borderless field, in a filled rounded box —
  rather than using a bordered text field, and the three panes share the exact
  styling (padding 6, control background, radius 6).
-->
<script lang="ts">
  import Icon from "./Icon.svelte";

  interface Props {
    value: string;
    /** Verbatim from the spec: "Search", "Search notes…", "Search vocabulary…". */
    placeholder: string;
    ariaLabel?: string;
    oninput?: (value: string) => void;
  }

  let { value = $bindable(""), placeholder, ariaLabel, oninput }: Props = $props();
</script>

<div class="search">
  <span class="glyph"><Icon name="search" /></span>
  <input
    class="input"
    type="search"
    autocomplete="off"
    spellcheck="false"
    {placeholder}
    aria-label={ariaLabel ?? placeholder}
    {value}
    oninput={(event) => {
      value = event.currentTarget.value;
      oninput?.(value);
    }}
  />
</div>

<style>
  .search {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    gap: var(--space-1);
    align-items: center;
    padding: 6px;
    background: var(--bg-control);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .search:focus-within {
    border-color: var(--accent);
  }

  .glyph {
    color: var(--text-secondary);
  }

  .input {
    flex: 1 1 auto;
    min-width: 0;
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    background: none;
    border: 0;
    outline: none;
  }

  .input::placeholder {
    color: var(--text-tertiary);
  }

  /* WebKit draws its own clear button and magnifier inside type=search; both
     fight the hand-drawn box, and the decoration reserves width even when the
     field is empty. */
  .input::-webkit-search-decoration,
  .input::-webkit-search-cancel-button,
  .input::-webkit-search-results-button {
    appearance: none;
  }
</style>
