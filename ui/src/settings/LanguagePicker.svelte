<!--
  General > Dictation Languages (MATRIX SET-037 to SET-046).

  The selection invariants are the interesting part and they are not obvious:
  `auto` is exclusive, and the selection is never allowed to become empty —
  clearing the last language falls back to English rather than to "no
  language", because an empty list would make the server guess.
-->
<script lang="ts">
  import Divider from "../lib/components/Divider.svelte";
  import SettingRow from "../lib/components/SettingRow.svelte";
  import TextField from "../lib/components/TextField.svelte";
  import Toggle from "../lib/components/Toggle.svelte";
  import { AUTO_CODE, LANGUAGES } from "./languages";

  interface Props {
    languages: readonly string[];
    onchange: (languages: string[]) => void;
  }

  let { languages, onchange }: Props = $props();

  let search = $state("");

  const autoDetect = $derived(languages.includes(AUTO_CODE));

  /**
   * Chips follow the master table's order, not the order the user picked
   * them: the Swift implementation held the selection in a `Set` and filtered
   * the master array through it, and reproducing that keeps the chip row
   * stable instead of reshuffling on every toggle.
   */
  const selected = $derived(LANGUAGES.filter((lang) => languages.includes(lang.code)));

  /** Case-insensitive, on the NAME only — a user typing `de` wants German, not every code. */
  const visible = $derived.by(() => {
    const needle = search.trim().toLowerCase();
    if (needle === "") return LANGUAGES;
    return LANGUAGES.filter((lang) => lang.name.toLowerCase().includes(needle));
  });

  function setAutoDetect(on: boolean) {
    onchange(on ? [AUTO_CODE] : ["en"]);
  }

  function toggleLanguage(code: string) {
    const next = languages.filter((c) => c !== AUTO_CODE && c !== code);
    if (!languages.includes(code)) next.push(code);
    onchange(next.length === 0 ? ["en"] : next);
  }
</script>

<SettingRow
  title="Auto-detect"
  description="Automatically detect the spoken language"
  emphasized
>
  {#snippet control({ id, describedBy })}
    <Toggle
      {id}
      checked={autoDetect}
      ariaDescribedby={describedBy}
      onchange={setAutoDetect}
    />
  {/snippet}
</SettingRow>

<Divider />

{#if autoDetect}
  <p class="note">
    All supported languages will be recognized automatically. Specifying languages manually can
    improve accuracy.
  </p>
{:else}
  {#if selected.length > 0}
    <ul class="chips" aria-label="Selected languages">
      {#each selected as lang (lang.code)}
        <li class="chip">
          <span>{lang.flag} {lang.name}</span>
          <button
            type="button"
            class="chip-remove"
            aria-label="Remove {lang.name}"
            onclick={() => toggleLanguage(lang.code)}
          >
            <svg viewBox="0 0 14 14" width="12" height="12" aria-hidden="true">
              <circle cx="7" cy="7" r="6" />
              <path d="M4.8 4.8l4.4 4.4M9.2 4.8l-4.4 4.4" />
            </svg>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <TextField
    bind:value={search}
    type="search"
    placeholder="Search languages..."
    ariaLabel="Search languages"
  />

  <div class="list-wrap">
    <ul class="list">
      {#each visible as lang (lang.code)}
        <li>
          <label class="lang">
            <input
              type="checkbox"
              role="switch"
              class="lang-switch"
              checked={languages.includes(lang.code)}
              onchange={() => toggleLanguage(lang.code)}
            />
            <span>{lang.flag} {lang.name}</span>
          </label>
        </li>
      {/each}
      {#if visible.length === 0}
        <li class="empty">No languages match “{search}”.</li>
      {/if}
    </ul>
    <div class="fade" aria-hidden="true"></div>
  </div>
{/if}

<style>
  .note {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* The 12% accent wash is a separate layer rather than `color-mix`, which
     the oldest WebKit this app supports (macOS 13.0, Safari 16.1) does not
     have — there it would drop to no background at all and the chip would
     vanish into the pane. */
  .chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-subheadline);
    border-radius: var(--radius-lg);
  }

  .chip::before {
    content: "";
    position: absolute;
    inset: 0;
    background: var(--accent);
    border-radius: inherit;
    opacity: 0.12;
  }

  /* Keep the label and the remove button above the wash. */
  .chip > * {
    position: relative;
  }

  .chip-remove {
    display: inline-flex;
    padding: 0;
    color: var(--text-secondary);
    background: none;
    border: none;
  }

  .chip-remove svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
  }

  .list-wrap {
    position: relative;
    width: 100%;
    height: 220px;
    background: var(--bg-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .list {
    height: 100%;
    margin: 0;
    padding: var(--space-1) 0;
    overflow-y: scroll;
    list-style: none;
  }

  .lang {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 5px var(--space-2);
    font-size: var(--text-body);
  }

  .lang:hover {
    background: var(--bg-selected);
  }

  /* Divider between rows, inset 8 from the leading edge. */
  .list li + li {
    position: relative;
  }

  .list li + li::before {
    content: "";
    position: absolute;
    top: 0;
    right: 0;
    left: var(--space-2);
    height: 1px;
    background: var(--border);
  }

  .empty {
    padding: var(--space-2);
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .lang-switch {
    appearance: none;
    flex: none;
    position: relative;
    width: 26px;
    height: 16px;
    margin: 0;
    background: var(--border-strong);
    border-radius: var(--radius-lg);
    transition: background var(--duration-fast) var(--ease);
  }

  .lang-switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    background: var(--bg-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
    transition: transform var(--duration-fast) var(--ease);
  }

  .lang-switch:checked {
    background: var(--accent);
  }

  .lang-switch:checked::after {
    transform: translateX(10px);
  }

  .fade {
    position: absolute;
    right: 1px;
    bottom: 1px;
    left: 1px;
    height: 28px;
    border-radius: var(--radius-sm);
    /* Opacity on the layer instead of inside the gradient: same result, and
       no `color-mix`, which Safari 16.1 lacks. */
    background: linear-gradient(to bottom, transparent, var(--bg-content));
    opacity: 0.85;
    pointer-events: none;
  }
</style>
