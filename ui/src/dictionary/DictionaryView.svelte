<!--
  The dictionary pane: vocabulary phrases and text snippets.

  Two things here look like bugs and are not. The tab selection is view-local
  and not persisted (WIN-033), and both tabs share one search query, so
  switching tabs carries the filter over and both lists are recomputed on every
  refresh (WIN-034). Both are observable behaviour of the reference app and
  changing either would be a deviation.

  `dictionary_list(kind)` takes no query, so filtering happens here. It is a
  filter and never a sort: the row order is exactly what the backend returned
  (WIN-042).
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "../lib/components/Button.svelte";
  import ConfirmDialog from "../lib/components/ConfirmDialog.svelte";
  import ContextMenu, { type MenuItem } from "../lib/components/ContextMenu.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import ListPane from "../lib/components/ListPane.svelte";
  import ListRow from "../lib/components/ListRow.svelte";
  import SearchField from "../lib/components/SearchField.svelte";
  import SegmentedControl from "../lib/components/SegmentedControl.svelte";
  import Sheet from "../lib/components/Sheet.svelte";
  import {
    describe,
    dictionaryAdd,
    dictionaryDelete,
    dictionaryImportCsv,
    dictionaryList,
    dictionaryUpdate,
    onDictionaryChanged,
    type DictionaryEntry,
  } from "../lib/ipc";
  import { keytermWarning } from "./keyterm";

  /** Row metadata shows the date only, e.g. `3/4/25` (WIN-040). */
  const shortDate = new Intl.DateTimeFormat(undefined, { dateStyle: "short" });

  /** Hard character caps the reference sheets enforced on every keystroke. */
  const MAX_PHRASE = 60;
  const MAX_REPLACEMENT = 200;
  const MAX_EXPANSION = 4000;

  const TABS = [
    { value: "vocabulary", label: "Vocabulary" },
    { value: "snippets", label: "Snippets" },
  ] as const;

  /** An open add/edit sheet. `entry` is null when adding. */
  interface Draft {
    entry: DictionaryEntry | null;
    snippet: boolean;
    phrase: string;
    replacement: string;
  }

  let tab = $state<string>("vocabulary");
  let query = $state("");
  let vocabulary = $state<DictionaryEntry[]>([]);
  let snippets = $state<DictionaryEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let draft = $state<Draft | null>(null);
  let pendingDelete = $state<DictionaryEntry | null>(null);
  let importResult = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; items: ReadonlyArray<MenuItem | null> } | null>(null);

  let generation = 0;

  const isSnippets = $derived(tab === "snippets");

  const rows = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    const source = isSnippets ? snippets : vocabulary;
    if (needle === "") return source;
    return source.filter(
      (entry) =>
        entry.phrase.toLowerCase().includes(needle) ||
        (entry.replacement ?? "").toLowerCase().includes(needle),
    );
  });

  /**
   * Advisory only — the entry saves either way. Vocabulary phrases are the
   * only rows forwarded to Deepgram as keyterms, so a snippet abbreviation is
   * never checked: warning about one would be plain misinformation.
   */
  const draftWarning = $derived(
    draft === null || draft.snippet ? null : keytermWarning(draft.phrase),
  );

  const sheetTitle = $derived.by(() => {
    if (draft === null) return "";
    if (draft.entry === null) return draft.snippet ? "Add Snippet" : "Add Vocabulary Word";
    return draft.snippet ? "Edit Snippet" : "Edit Vocabulary Word";
  });

  function newDraft(): Draft {
    return { entry: null, snippet: isSnippets, phrase: "", replacement: "" };
  }

  function toDraft(entry: DictionaryEntry): Draft {
    return {
      entry,
      snippet: entry.isSnippet,
      phrase: entry.phrase,
      replacement: entry.replacement ?? "",
    };
  }

  async function load(): Promise<void> {
    const stamp = ++generation;
    loading = true;
    try {
      // Both lists, always: the shared query means switching tabs must not
      // trigger a fetch, and the reference recomputed both on every refresh.
      const [vocab, snips] = await Promise.all([
        dictionaryList("vocabulary"),
        dictionaryList("snippets"),
      ]);
      if (stamp !== generation) return;
      vocabulary = vocab;
      snippets = snips;
      error = null;
    } catch (cause) {
      if (stamp === generation) error = describe(cause);
    } finally {
      if (stamp === generation) loading = false;
    }
  }

  async function commit(): Promise<void> {
    const current = draft;
    draft = null;
    if (current === null) return;

    const phrase = current.phrase.trim();
    const trimmed = current.replacement.trim();
    // A blank replacement is stored as NULL, not as an empty string
    // (WIN-050, WIN-054) — the two mean different things to the matcher.
    const replacement = trimmed === "" ? null : trimmed;

    try {
      if (current.entry === null) {
        // Only phrase, replacement and isSnippet are read by the backend; it
        // mints the id, stamps both timestamps and marks the row manual.
        await dictionaryAdd({
          id: "",
          phrase,
          replacement,
          isSnippet: current.snippet,
          manualEntry: true,
          source: null,
          frequencyUsed: 0,
          createdAt: 0,
          modifiedAt: 0,
        });
      } else {
        await dictionaryUpdate({ ...current.entry, phrase, replacement });
      }
      await load();
    } catch (cause) {
      error = describe(cause);
    }
  }

  async function confirmDelete(): Promise<void> {
    const entry = pendingDelete;
    pendingDelete = null;
    if (entry === null) return;
    try {
      await dictionaryDelete(entry.id);
      await load();
    } catch (cause) {
      error = describe(cause);
    }
  }

  async function importCsv(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      // Cancelling the picker is a no-op, not an error (WIN-047).
      if (typeof path !== "string") return;

      const result = await dictionaryImportCsv(path);
      await load();
      importResult =
        result.errors.length === 0
          ? `Imported ${result.imported} entries.`
          : `Imported ${result.imported} entries with ${result.errors.length} errors:\n${result.errors
              .slice(0, 5)
              .join("\n")}`;
    } catch (cause) {
      error = describe(cause);
    }
  }

  function openMenu(event: MouseEvent, entry: DictionaryEntry): void {
    event.preventDefault();
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: "Edit", onclick: () => (draft = toDraft(entry)) },
        null,
        { label: "Delete", destructive: true, onclick: () => (pendingDelete = entry) },
      ],
    };
  }

  onMount(() => {
    void load();
    const stop = onDictionaryChanged(() => void load());
    const onVisible = () => {
      if (document.visibilityState === "visible") void load();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
</script>

<div class="dictionary">
  <div class="tabs">
    <SegmentedControl
      bind:value={tab}
      options={TABS}
      name="dictionary-tab"
      ariaLabel="Dictionary section"
    />
  </div>

  <!-- With rows on screen a failure is a banner; with none it owns the pane, so
       the list never says "No vocabulary words yet" when the query failed. -->
  <ListPane
    error={rows.length > 0 ? error : null}
    onretry={() => void load()}
    ondismisserror={() => (error = null)}
  >
    {#snippet toolbar()}
      <SearchField
        bind:value={query}
        placeholder={isSnippets ? "Search snippets…" : "Search vocabulary…"}
        ariaLabel={isSnippets ? "Search snippets" : "Search vocabulary"}
      />
      {#if isSnippets}
        <Button size="regular" onclick={() => void importCsv()}>
          <Icon name="import" />
          Import CSV
        </Button>
      {/if}
      <Button size="regular" onclick={() => (draft = newDraft())}>
        <Icon name="plus" />
        {isSnippets ? "Add Snippet" : "Add Word"}
      </Button>
    {/snippet}

    {#if loading && rows.length === 0}
      <EmptyState title="Loading…" />
    {:else if error !== null && rows.length === 0}
      <EmptyState
        icon="warning"
        title="Couldn't load the dictionary"
        description={error}
        action={{ label: "Retry", onclick: () => void load() }}
      />
    {:else if rows.length === 0 && query.trim() !== ""}
      <EmptyState title={`No results for "${query}"`} />
    {:else if rows.length === 0}
      <EmptyState
        icon={isSnippets ? "snippet" : "book"}
        title={isSnippets ? "No snippets yet" : "No vocabulary words yet"}
        action={{
          label: isSnippets ? "Add Snippet" : "Add Word",
          onclick: () => (draft = newDraft()),
        }}
      />
    {:else}
      {#each rows as entry, index (entry.id)}
        <ListRow
          {index}
          onclick={() => (draft = toDraft(entry))}
          oncontextmenu={(event) => openMenu(event, entry)}
        >
          <div class="entry">
            <span class="phrase" class:accent={isSnippets}>{entry.phrase}</span>
            {#if entry.replacement}
              <p class="replacement" class:two-line={isSnippets}>{entry.replacement}</p>
            {/if}
          </div>

          <!-- Snippet rows carry no badge, no usage count and no date (WIN-046). -->
          {#if !isSnippets}
            {#if entry.source}
              <span class="badge">{entry.source}</span>
            {/if}
            {#if entry.frequencyUsed > 0}
              <span class="uses">{entry.frequencyUsed}x</span>
            {/if}
            <span class="modified">{shortDate.format(new Date(entry.modifiedAt * 1000))}</span>
          {/if}
        </ListRow>
      {/each}
    {/if}
  </ListPane>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

{#if draft}
  <Sheet
    open
    title={sheetTitle}
    width={draft.snippet ? 420 : 380}
    confirmLabel={draft.entry === null ? "Add" : "Save"}
    confirmDisabled={draft.phrase.trim() === "" ||
      // Adding a snippet requires an expansion; editing one does not, so an
      // existing snippet can have its expansion cleared (WIN-052 vs WIN-054).
      (draft.entry === null && draft.snippet && draft.replacement.trim() === "")}
    onconfirm={() => void commit()}
    oncancel={() => (draft = null)}
  >
    <input
      class="field selectable"
      type="text"
      maxlength={MAX_PHRASE}
      placeholder={draft.snippet
        ? draft.entry === null
          ? "Abbreviation (max 60 chars)"
          : "Abbreviation"
        : draft.entry === null
          ? "Word or phrase (max 60 chars)"
          : "Word or phrase"}
      aria-label={draft.snippet ? "Abbreviation" : "Word or phrase"}
      bind:value={draft.phrase}
    />

    {#if draft.snippet}
      <label class="caption" for="dictionary-expansion">Expansion</label>
      <textarea
        id="dictionary-expansion"
        class="expansion selectable"
        maxlength={MAX_EXPANSION}
        bind:value={draft.replacement}
      ></textarea>
    {:else}
      <input
        class="field selectable"
        type="text"
        maxlength={MAX_REPLACEMENT}
        placeholder="Replacement (optional)"
        aria-label="Replacement"
        bind:value={draft.replacement}
      />
    {/if}

    {#if draftWarning}
      <!-- Advisory: the phrase may still have a local replacement. -->
      <p class="keyterm-warning">
        <Icon name="warning" />
        <span>{draftWarning}</span>
      </p>
    {/if}
  </Sheet>
{/if}

<ConfirmDialog
  open={pendingDelete !== null}
  message="Delete this entry?"
  informative="This action cannot be undone."
  confirmLabel="Delete"
  destructive
  onconfirm={() => void confirmDelete()}
  oncancel={() => (pendingDelete = null)}
/>

<ConfirmDialog
  open={importResult !== null}
  message="Import Complete"
  informative={importResult ?? ""}
  confirmLabel="OK"
  showCancel={false}
  onconfirm={() => (importResult = null)}
  oncancel={() => (importResult = null)}
/>

<style>
  .dictionary {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg-content);
  }

  .tabs {
    flex: none;
    padding: var(--space-2);
  }

  .entry {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .phrase {
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .phrase.accent {
    color: var(--accent);
  }

  .replacement {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 1;
    line-clamp: 1;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .replacement.two-line {
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  /* Accent at 10% as a separate layer rather than `color-mix`, which the
     oldest WebKit we support (macOS 13.0, Safari 16.1) does not have. */
  .badge {
    position: relative;
    flex: none;
    isolation: isolate;
    padding: 2px 6px;
    font-size: var(--text-caption);
    color: var(--accent);
  }

  .badge::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    background: var(--accent);
    border-radius: 4px;
    opacity: 0.1;
  }

  .uses {
    flex: none;
    font-size: var(--text-caption);
    color: var(--text-secondary);
  }

  .modified {
    flex: none;
    font-size: var(--text-caption);
    color: var(--text-tertiary);
  }

  .field {
    padding: var(--space-1) var(--space-2);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    background: var(--bg-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .caption {
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .expansion {
    height: 100px;
    padding: var(--space-2);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    resize: none;
    background: var(--bg-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .keyterm-warning {
    display: flex;
    gap: var(--space-1);
    align-items: flex-start;
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--warning);
  }
</style>
