<!--
  The notes pane.

  Ordering comes straight from the backend query and is deliberately not
  re-sorted here (WIN-023); the store already returns notes by modified date.

  A single left click opens the editor. The reference implementation achieved
  that by way of a list selection it immediately cleared, so rows never looked
  selected; here the click simply opens the editor, which is the same observable
  behaviour without the round trip through a selection that is thrown away.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "../lib/components/Button.svelte";
  import ContextMenu, { type MenuItem } from "../lib/components/ContextMenu.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import ListPane from "../lib/components/ListPane.svelte";
  import ListRow from "../lib/components/ListRow.svelte";
  import SearchField from "../lib/components/SearchField.svelte";
  import Sheet from "../lib/components/Sheet.svelte";
  import {
    describe,
    notesAdd,
    notesDelete,
    notesList,
    notesUpdate,
    type NoteEntry,
  } from "../lib/ipc";

  /** Row timestamps use the short date *and* time style, e.g. `3/4/25, 3:42 PM`. */
  const shortDateTime = new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  });

  interface Editing {
    id: string;
    title: string;
    content: string;
  }

  let notes = $state<NoteEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");
  let editing = $state<Editing | null>(null);
  let menu = $state<{ x: number; y: number; items: ReadonlyArray<MenuItem | null> } | null>(null);

  let generation = 0;

  async function load(): Promise<void> {
    const stamp = ++generation;
    loading = true;
    try {
      const trimmed = query.trim();
      const rows = await notesList(trimmed === "" ? null : trimmed);
      if (stamp !== generation) return;
      notes = rows;
      error = null;
    } catch (cause) {
      if (stamp === generation) error = describe(cause);
    } finally {
      if (stamp === generation) loading = false;
    }
  }

  async function create(): Promise<void> {
    try {
      // The backend mints the row and hands it back, so the editor opens on the
      // real note rather than on a placeholder that has to be reconciled later.
      const note = await notesAdd("", "");
      await load();
      editing = { id: note.id, title: note.title, content: note.content };
    } catch (cause) {
      error = describe(cause);
    }
  }

  async function save(): Promise<void> {
    const draft = editing;
    editing = null;
    if (draft === null) return;
    try {
      await notesUpdate(draft.id, draft.title, draft.content);
      await load();
    } catch (cause) {
      error = describe(cause);
    }
  }

  async function remove(id: string): Promise<void> {
    try {
      // Soft delete with no confirmation, matching the reference (WIN-026).
      await notesDelete(id);
      await load();
    } catch (cause) {
      error = describe(cause);
    }
  }

  function openMenu(event: MouseEvent, note: NoteEntry): void {
    event.preventDefault();
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: "Edit",
          onclick: () => (editing = { id: note.id, title: note.title, content: note.content }),
        },
        null,
        { label: "Delete", destructive: true, onclick: () => void remove(note.id) },
      ],
    };
  }

  onMount(() => {
    void load();
    const onVisible = () => {
      if (document.visibilityState === "visible") void load();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  });
</script>

<!-- With rows on screen a failure is a banner; with none it owns the pane, so
     the list never says "No notes yet" when the query simply did not run. -->
<ListPane
  error={notes.length > 0 ? error : null}
  onretry={() => void load()}
  ondismisserror={() => (error = null)}
>
  {#snippet toolbar()}
    <SearchField
      bind:value={query}
      placeholder="Search notes…"
      ariaLabel="Search notes"
      oninput={() => void load()}
    />
    <Button size="regular" onclick={() => void create()}>
      <Icon name="plus" />
      New Note
    </Button>
  {/snippet}

  {#if loading && notes.length === 0}
    <EmptyState title="Loading…" />
  {:else if error !== null && notes.length === 0}
    <EmptyState
      icon="warning"
      title="Couldn't load notes"
      description={error}
      action={{ label: "Retry", onclick: () => void load() }}
    />
  {:else if notes.length === 0 && query.trim() !== ""}
    <EmptyState title={`No results for "${query}"`} />
  {:else if notes.length === 0}
    <EmptyState
      icon="note"
      title="No notes yet"
      action={{ label: "Create Note", onclick: () => void create() }}
    />
  {:else}
    {#each notes as note, index (note.id)}
      <ListRow
        {index}
        onclick={() => (editing = { id: note.id, title: note.title, content: note.content })}
        oncontextmenu={(event) => openMenu(event, note)}
      >
        <div class="note">
          <div class="head">
            <span class="title">{note.title === "" ? "Untitled" : note.title}</span>
            <span class="modified">
              {shortDateTime.format(new Date(note.modifiedAt * 1000))}
            </span>
          </div>
          {#if note.contentPreview !== ""}
            <p class="preview">{note.contentPreview}</p>
          {/if}
        </div>
      </ListRow>
    {/each}
  {/if}
</ListPane>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

{#if editing}
  <!-- 500 x 400, padding 24 (WIN-028). Save is never disabled: an empty note
       is a legitimate thing to save. -->
  <Sheet
    open
    title={editing.title === "" ? "New Note" : editing.title}
    headingHidden
    width={500}
    height={400}
    spacing={8}
    confirmLabel="Save"
    onconfirm={() => void save()}
    oncancel={() => (editing = null)}
  >
    <input
      class="title-field selectable"
      type="text"
      placeholder="Title"
      aria-label="Title"
      bind:value={editing.title}
    />
    <textarea class="body-field selectable" aria-label="Note" bind:value={editing.content}
    ></textarea>
  </Sheet>
{/if}

<style>
  .note {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .head {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
  }

  .title {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .modified {
    flex: none;
    font-size: var(--text-caption);
    color: var(--text-tertiary);
  }

  .preview {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .title-field {
    flex: none;
    padding: var(--space-1) var(--space-2);
    font-family: inherit;
    font-size: var(--text-title);
    color: var(--text-primary);
    background: var(--bg-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .body-field {
    /* min-height 200 in the reference; it grows to fill the 400pt sheet. */
    flex: 1 1 auto;
    min-height: 200px;
    padding: var(--space-2);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    resize: none;
    background: var(--bg-content);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
</style>
