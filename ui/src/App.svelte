<!--
  The root of the main document.

  One HTML file serves four windows: the settings window and the three
  standalone list windows, chosen by `?window=`. That keeps the build to two
  entry points (this and the overlay, which must stay tiny because it paints
  on every dictation) instead of five near-identical documents.

  In the settings window the three list views are embedded in the detail pane
  edge to edge — no scroll wrapper and no padding — exactly as ui-spec 3.2
  specifies; standalone they fill the window. Same components either way.
-->
<script lang="ts">
  import Sidebar from "./lib/components/Sidebar.svelte";
  import ErrorBanner from "./lib/components/ErrorBanner.svelte";
  import General from "./settings/panes/General.svelte";
  import Dictation from "./settings/panes/Dictation.svelte";
  import Transcription from "./settings/panes/Transcription.svelte";
  import Privacy from "./settings/panes/Privacy.svelte";
  import System from "./settings/panes/System.svelte";
  import HistoryView from "./history/HistoryView.svelte";
  import DictionaryView from "./dictionary/DictionaryView.svelte";
  import NotesView from "./notes/NotesView.svelte";
  import Onboarding from "./onboarding/Onboarding.svelte";
  import {
    DEFAULT_SECTION,
    SECTIONS,
    isEdgeToEdge,
    isSectionId,
    type SectionId,
  } from "./settings/sections";
  import {
    loadSettings,
    reloadSettings,
    saveError,
    settings,
    watchExternalSettings,
  } from "./lib/ipc";

  interface Props {
    /** `settings`, or one of the three list views shown in its own window. */
    route: string;
  }

  let { route }: Props = $props();

  let section = $state<SectionId>(DEFAULT_SECTION);

  // A standalone list window is still routed through the section ids, so the
  // sidebar and the window share one vocabulary.
  const standalone = $derived(route !== "settings" && isSectionId(route) ? route : null);

  $effect(() => {
    void loadSettings();
    return watchExternalSettings();
  });
</script>

<!-- The required walkthrough owns the whole settings window until complete.
     Standalone history, dictionary, and notes windows remain independent. -->
{#if standalone === null && $settings.state === "ready" && !$settings.value.didCompleteOnboarding}
  <Onboarding value={$settings.value} />
{:else if standalone !== null}
  <main class="standalone">
    {#if standalone === "history"}
      <HistoryView />
    {:else if standalone === "dictionary"}
      <DictionaryView />
    {:else if standalone === "notes"}
      <NotesView />
    {/if}
  </main>
{:else}
  <div class="window">
    <Sidebar selected={section} onselect={(id) => (section = id)} />

    <main class="detail">
      <header class="titlebar">
        <h1>{SECTIONS[section].title}</h1>
      </header>

      {#if isEdgeToEdge(section)}
        <div class="edge">
          {#if section === "history"}
            <HistoryView />
          {:else if section === "dictionary"}
            <DictionaryView />
          {:else}
            <NotesView />
          {/if}
        </div>
      {:else if $settings.state === "loading"}
        <div class="pane">
          <p class="status">Loading settings…</p>
        </div>
      {:else if $settings.state === "error"}
        <div class="pane">
          <ErrorBanner message="Could not load settings: {$settings.message}" onretry={reloadSettings} />
        </div>
      {:else}
        <div class="pane">
          {#if $saveError}
            <ErrorBanner message="Could not save settings: {$saveError}" />
          {/if}

          {#if section === "general"}
            <General value={$settings.value} />
          {:else if section === "dictation"}
            <Dictation value={$settings.value} />
          {:else if section === "transcription"}
            <Transcription value={$settings.value} />
          {:else if section === "privacy"}
            <Privacy value={$settings.value} />
          {:else if section === "system"}
            <System value={$settings.value} />
          {/if}
        </div>
      {/if}
    </main>
  </div>
{/if}

<style>
  /* The mount point has to carry the window height for the flex layout below
     to have anything to fill; app.css owns html and body, not this div. */
  :global(#app) {
    height: 100%;
  }

  .window {
    display: flex;
    height: 100%;
    min-height: 0;
  }

  .standalone {
    height: 100%;
    background: var(--bg-content);
  }

  .detail {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-window);
  }

  .titlebar {
    flex: none;
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--border);
  }

  .titlebar h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: var(--weight-semibold);
  }

  /* Windows 11 Settings draws no rule under the page title: the header, the
     content and the navigation pane all sit on one backdrop, so a hairline
     spanning only the detail column would read as an unfinished divider. */
  :global(:root[data-platform="windows"]) .titlebar {
    border-bottom: none;
  }

  /* Every scrolling section: leading-aligned stack, spacing 16, padding 28. */
  .pane {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
    min-height: 0;
    padding: 28px;
    overflow-y: auto;
  }

  /* History, Dictionary and Notes fill the pane with no wrapper. */
  .edge {
    flex: 1 1 auto;
    min-height: 0;
    background: var(--bg-content);
  }

  .status {
    margin: 0;
    color: var(--text-secondary);
  }
</style>
