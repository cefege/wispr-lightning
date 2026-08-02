<!--
  The glyph set.

  SF Symbols are not licensed for redistribution off Apple platforms, so every
  symbol the Swift UI used is redrawn here as a stroked 16x16 path. They are
  inline rather than a sprite sheet or an icon font: there are eleven of them,
  they must inherit `currentColor` and the surrounding font size, and a font
  would add a network request and a flash of unstyled glyph to a window that is
  otherwise instant.

  Names are the app's own, with the SF Symbol they replace noted alongside.
-->
<script module lang="ts">
  export type IconName =
    | "search" // magnifyingglass
    | "plus" // plus
    | "copy" // doc.on.doc
    | "trash" // trash
    | "history-empty" // text.badge.minus
    | "note" // note.text
    | "book" // character.book.closed
    | "snippet" // text.snippet
    | "import" // square.and.arrow.down
    | "warning" // exclamationmark.triangle
    | "close"; // xmark
</script>

<script lang="ts">
  interface Props {
    name: IconName;
    /** Edge length in px. 36 is the size the empty-state glyphs use. */
    size?: number;
  }

  let { name, size = 16 }: Props = $props();

  // Stroke widths scale down as the glyph grows so a 36 px empty-state icon
  // does not read as a heavier weight than the same shape at 16 px.
  const stroke = $derived(size >= 28 ? 1.25 : 1.5);
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === "search"}
    <circle cx="7" cy="7" r="4.5" />
    <path d="M10.4 10.4 14 14" />
  {:else if name === "plus"}
    <path d="M8 3v10M3 8h10" />
  {:else if name === "copy"}
    <rect x="5.75" y="5.75" width="7.5" height="8.5" rx="1.5" />
    <path d="M10.25 3.75H4.25a1.5 1.5 0 0 0-1.5 1.5v6" />
  {:else if name === "trash"}
    <path d="M2.75 4.5h10.5" />
    <path d="M6.5 4.5V3.25a.75.75 0 0 1 .75-.75h1.5a.75.75 0 0 1 .75.75V4.5" />
    <path d="M4.25 4.5l.55 8.3a1 1 0 0 0 1 .95h4.4a1 1 0 0 0 1-.95l.55-8.3" />
  {:else if name === "history-empty"}
    <path d="M2 3.75h12M2 7.5h6.5M2 11.25h5" />
    <circle cx="11.75" cy="11.5" r="3.25" />
    <path d="M10.25 11.5h3" />
  {:else if name === "note"}
    <rect x="2.75" y="2" width="10.5" height="12" rx="1.75" />
    <path d="M5.5 5.5h5M5.5 8h5M5.5 10.5h3" />
  {:else if name === "book"}
    <path d="M4 2.25h7.25a1 1 0 0 1 1 1v9.5a1 1 0 0 1-1 1H4z" />
    <path d="M4 2.25a1.25 1.25 0 0 0 0 11.5" />
    <path d="M6.5 5.75h3.25" />
  {:else if name === "snippet"}
    <path d="M5.5 3.5 2.5 8l3 4.5" />
    <path d="M10.5 3.5 13.5 8l-3 4.5" />
  {:else if name === "import"}
    <path d="M8 2.25v7.5M5.25 7l2.75 2.75L10.75 7" />
    <path d="M2.75 11.5v1.25a1 1 0 0 0 1 1h8.5a1 1 0 0 0 1-1V11.5" />
  {:else if name === "warning"}
    <path d="M7.13 2.6 1.4 12.5a1 1 0 0 0 .87 1.5h11.46a1 1 0 0 0 .87-1.5L8.87 2.6a1 1 0 0 0-1.74 0Z" />
    <path d="M8 6.25v3.25" />
    <circle cx="8" cy="11.6" r="0.6" fill="currentColor" stroke="none" />
  {:else}
    <path d="M4 4l8 8M12 4l-8 8" />
  {/if}
</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>
