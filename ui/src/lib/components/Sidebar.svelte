<!--
  The 220px source list (ui-spec 3.2).

  Three unlabeled groups separated by gaps, each row an icon plus a title, the
  app icon inset at the top. Rows are real buttons so they are in the tab
  order, and Up/Down move between them the way a native source list does — a
  `roving tabindex` would be the alternative, but a settings sidebar is short
  enough that one tab stop per section is not a burden and is easier to reason
  about.

  The selected row is drawn the way each host draws it: macOS fills it with the
  accent and flips the text to `--text-on-accent`; Windows 11 uses a subtle
  wash, keeps the label in primary ink, and puts a short accent bar on the
  leading edge. See the `[data-platform="windows"]` rules at the foot of the
  stylesheet.
-->
<script lang="ts">
  import SectionIcon from "./SectionIcon.svelte";
  import { SECTIONS, SECTION_GROUPS, type SectionId } from "../../settings/sections";

  interface Props {
    selected: SectionId;
    onselect: (id: SectionId) => void;
  }

  let { selected, onselect }: Props = $props();

  const order = SECTION_GROUPS.flat();

  function move(delta: number, event: KeyboardEvent) {
    const index = order.indexOf(selected);
    const next = order[index + delta];
    if (next === undefined) return;
    event.preventDefault();
    onselect(next);
    const el = document.getElementById(`sidebar-${next}`);
    el?.focus();
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") move(1, event);
    else if (event.key === "ArrowUp") move(-1, event);
  }
</script>

<nav class="sidebar" aria-label="Settings sections">
  <div class="brand">
    <img src="/app-icon.png" width="64" height="64" alt="Wispr Lightning" />
  </div>

  <div class="groups">
    {#each SECTION_GROUPS as group, index (index)}
      <ul class="group">
        {#each group as id (id)}
          <li>
            <button
              type="button"
              id="sidebar-{id}"
              class="row"
              class:selected={selected === id}
              aria-current={selected === id ? "page" : undefined}
              onclick={() => onselect(id)}
              {onkeydown}
            >
              <SectionIcon section={id} />
              <span class="label">{SECTIONS[id].title}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/each}
  </div>
</nav>

<style>
  .sidebar {
    display: flex;
    flex: none;
    flex-direction: column;
    width: 220px;
    height: 100%;
    overflow-y: auto;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
  }

  .brand {
    display: flex;
    justify-content: center;
    padding-top: var(--space-3);
    padding-bottom: var(--space-2);
  }

  .brand img {
    border-radius: var(--radius-icon);
  }

  .groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: 0 var(--space-2) var(--space-3);
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 1px var(--space-2);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    transition: background var(--duration-fast) var(--ease);
  }

  .row:hover:not(.selected) {
    background: var(--bg-selected);
  }

  .row.selected {
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* -- Windows 11 --------------------------------------------------------
     Its navigation pane shares one backdrop with the content (`--bg-sidebar`
     resolves to `--bg-window` there), so the divider that separates the two
     surfaces on macOS has nothing to separate and would read as a stray rule
     across half the window. */
  :global(:root[data-platform="windows"]) .sidebar {
    border-right: none;
  }

  /* WinUI's NavigationViewItem is 36px tall and inks its glyph from the row. */
  :global(:root[data-platform="windows"]) .row {
    position: relative;
    min-height: 36px;
    --glyph-ink: var(--text-primary);
  }

  :global(:root[data-platform="windows"]) .row.selected {
    color: var(--text-primary);
    background: var(--bg-selected);
    --glyph-ink: var(--accent);
  }

  /* The selection indicator: 3x16, fully rounded, leading edge, vertically
     centred — the geometry WinUI ships. `--radius-sm` exceeds half the 3px
     width, so the browser clamps it to a pill without a literal radius here. */
  :global(:root[data-platform="windows"]) .row.selected::before {
    content: "";
    position: absolute;
    top: 50%;
    left: 0;
    width: 3px;
    height: 16px;
    background: var(--accent);
    border-radius: var(--radius-sm);
    transform: translateY(-50%);
  }
</style>
