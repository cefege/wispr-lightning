<!--
  The dictation history pane.

  Reads through `history_list(limit, offset)` a page at a time, both on scroll
  and from an explicit Load More — the button is not redundant, because a pane
  whose first page does not fill the window never fires a scroll event.

  Search is a separate, unpaginated command (`history_search`), so paging is
  suppressed while a query is active. That mirrors the reference app, whose
  search returned the whole match set in one go.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "../lib/components/Button.svelte";
  import ConfirmDialog from "../lib/components/ConfirmDialog.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import ListPane from "../lib/components/ListPane.svelte";
  import ListRow from "../lib/components/ListRow.svelte";
  import SearchField from "../lib/components/SearchField.svelte";
  import {
    copyText,
    describe,
    historyClear,
    historyDelete,
    historyList,
    historySearch,
    onHistoryChanged,
    type TranscriptEntry,
  } from "../lib/ipc";
  import { groupByDay, shortTime } from "./grouping";

  /** Rows per page. Large enough that the common case is one round trip. */
  const PAGE = 50;

  let entries = $state<TranscriptEntry[]>([]);
  let loading = $state(true);
  let loadingMore = $state(false);
  let hasMore = $state(false);
  let error = $state<string | null>(null);
  let query = $state("");
  let pendingDelete = $state<TranscriptEntry | null>(null);
  let confirmingClear = $state(false);

  /**
   * Guards against an out-of-order response overwriting a newer one: every
   * load stamps this, and a response whose stamp is stale is discarded.
   */
  let generation = 0;

  const groups = $derived(groupByDay(entries));

  /**
   * Zebra striping runs across the whole list and ignores the date headers, so
   * the stripe index has to be assigned here rather than by `:nth-child`.
   */
  const rendered = $derived.by(() => {
    let index = 0;
    return groups.map((group) => ({
      title: group.title,
      rows: group.entries.map((entry) => ({ entry, index: index++ })),
    }));
  });

  async function load(): Promise<void> {
    const stamp = ++generation;
    loading = true;
    try {
      const trimmed = query.trim();
      const rows = trimmed === "" ? await historyList(PAGE, 0) : await historySearch(trimmed);
      if (stamp !== generation) return;
      entries = rows;
      hasMore = trimmed === "" && rows.length === PAGE;
      error = null;
    } catch (cause) {
      if (stamp !== generation) return;
      error = describe(cause);
    } finally {
      if (stamp === generation) loading = false;
    }
  }

  async function loadMore(): Promise<void> {
    if (loadingMore || !hasMore) return;
    const stamp = generation;
    loadingMore = true;
    try {
      const rows = await historyList(PAGE, entries.length);
      if (stamp !== generation) return;
      entries = [...entries, ...rows];
      hasMore = rows.length === PAGE;
      error = null;
    } catch (cause) {
      if (stamp === generation) error = describe(cause);
    } finally {
      if (stamp === generation) loadingMore = false;
    }
  }

  /**
   * Refresh in place after a background change. Re-requesting exactly as many
   * rows as are already on screen keeps the user's scroll position and every
   * page they have paged through, which a reset to page one would throw away.
   */
  async function refreshInPlace(): Promise<void> {
    if (query.trim() !== "") {
      await load();
      return;
    }
    const stamp = ++generation;
    try {
      const rows = await historyList(Math.max(entries.length, PAGE), 0);
      if (stamp !== generation) return;
      entries = rows;
      error = null;
    } catch (cause) {
      if (stamp === generation) error = describe(cause);
    }
  }

  async function copy(entry: TranscriptEntry): Promise<void> {
    try {
      // The reference app gave no confirmation at all (WIN-015), so success is
      // silent. A *failure* is not: a copy that quietly did nothing is worse
      // than a banner.
      await copyText(entry.formattedText ?? entry.asrText ?? "");
    } catch (cause) {
      error = `Could not copy to the clipboard: ${describe(cause)}`;
    }
  }

  async function confirmDelete(): Promise<void> {
    const entry = pendingDelete;
    pendingDelete = null;
    if (entry === null) return;
    try {
      await historyDelete(entry.id);
      await refreshInPlace();
    } catch (cause) {
      error = describe(cause);
    }
  }

  async function confirmClear(): Promise<void> {
    confirmingClear = false;
    try {
      await historyClear();
      await load();
    } catch (cause) {
      error = describe(cause);
    }
  }

  // `onMount`, not `$effect`: `load()` reads `query` synchronously before its
  // first await, so an effect would re-subscribe on every keystroke and race
  // the `oninput` handler that already reloads.
  onMount(() => {
    void load();
    const stop = onHistoryChanged(() => void refreshInPlace());

    // Windows are hidden rather than destroyed, so a reopened pane is the same
    // document it was when it was closed and would otherwise show stale rows.
    const onVisible = () => {
      if (document.visibilityState === "visible") void refreshInPlace();
    };
    document.addEventListener("visibilitychange", onVisible);

    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
</script>

<!-- A failure with rows on screen is a banner over those rows; a failure with
     nothing on screen owns the whole pane instead, so the list never claims
     "No dictations yet" when the truth is that the query did not run. -->
<ListPane
  error={entries.length > 0 ? error : null}
  onretry={() => void load()}
  ondismisserror={() => (error = null)}
  onscroll={(event) => {
    const el = event.currentTarget;
    // One viewport of slack, so the next page is in flight before the user
    // reaches the bottom.
    if (el.scrollHeight - el.scrollTop - el.clientHeight < el.clientHeight) void loadMore();
  }}
>
  {#snippet toolbar()}
    <SearchField
      bind:value={query}
      placeholder="Search"
      ariaLabel="Search history"
      oninput={() => void load()}
    />
  {/snippet}

  {#if loading && entries.length === 0}
    <EmptyState title="Loading…" />
  {:else if error !== null && entries.length === 0}
    <EmptyState
      icon="warning"
      title="Couldn't load history"
      description={error}
      action={{ label: "Retry", onclick: () => void load() }}
    />
  {:else if entries.length === 0}
    <!-- Deliberately the same empty state for "no history" and "no search
         results": History has no distinct no-results state, unlike Notes and
         Dictionary, and that inconsistency is part of the parity contract
         (WIN-004). -->
    <EmptyState icon="history-empty" title="No dictations yet" />
  {:else}
    {#each rendered as group (group.title)}
      <h2 class="group">{group.title}</h2>
      {#each group.rows as row (row.entry.id)}
        <ListRow index={row.index}>
          <div class="entry">
            <div class="meta">
              <span>{shortTime.format(new Date(row.entry.timestamp * 1000))}</span>
              <span class="dot">·</span>
              <span>{row.entry.appName}</span>
              <span class="dot">·</span>
              <span>{row.entry.durationSecs.toFixed(1)}s</span>
              <span class="dot">·</span>
              <span>{row.entry.numWords} words</span>
              <span class="spacer"></span>
              <Button
                variant="borderless"
                title="Copy"
                ariaLabel="Copy"
                onclick={() => void copy(row.entry)}
              >
                <Icon name="copy" />
              </Button>
              <Button
                variant="borderless"
                title="Delete"
                ariaLabel="Delete"
                onclick={() => (pendingDelete = row.entry)}
              >
                <Icon name="trash" />
              </Button>
            </div>
            <p class="text selectable">
              {row.entry.formattedText ?? row.entry.asrText ?? ""}
            </p>
          </div>
        </ListRow>
      {/each}
    {/each}

    {#if hasMore}
      <div class="more">
        <Button disabled={loadingMore} onclick={() => void loadMore()}>
          {loadingMore ? "Loading…" : "Load More"}
        </Button>
      </div>
    {/if}

    <!-- Only ever rendered with a non-empty list (WIN-014). -->
    <div class="footer">
      <Button variant="danger" onclick={() => (confirmingClear = true)}>Clear All</Button>
    </div>
  {/if}
</ListPane>

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
  open={confirmingClear}
  message="Clear all history?"
  informative="This will delete all transcript entries. This action cannot be undone."
  confirmLabel="Clear All"
  destructive
  onconfirm={() => void confirmClear()}
  oncancel={() => (confirmingClear = false)}
/>

<style>
  .group {
    position: sticky;
    top: 0;
    z-index: 1;
    margin: 0;
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-subheadline);
    font-weight: var(--weight-semibold);
    color: var(--text-secondary);
    background: var(--bg-content);
  }

  .entry {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .meta {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .dot {
    color: var(--text-tertiary);
  }

  .spacer {
    flex: 1 1 auto;
  }

  .text {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-body);
    color: var(--text-primary);
  }

  .more,
  .footer {
    display: flex;
    flex: none;
    justify-content: flex-end;
    padding: var(--space-2);
  }

  .more {
    justify-content: center;
  }
</style>
