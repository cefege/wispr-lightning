<!--
  The 28x28 icon beside each sidebar row (ui-spec 3.3).

  SF Symbols are not licensable off Apple's platforms, so the glyphs are
  hand-drawn substitutes chosen to read the same at 13px: gear, microphone,
  waveform, sparkle, clock, book, note, raised hand, monitor. On macOS the tile
  geometry — 28x28, radius 7, vertical gradient, white glyph — is exact.

  Windows 11 Settings has no such tile: its navigation glyphs are flat and
  monochrome, taking the row's own ink and the accent when the row is selected.
  So on Windows the plate is dropped and the same paths are inked from
  `--glyph-ink`, which `Sidebar` sets per row. The geometry does not move —
  a 14px glyph stroked at 1.35px inside a 28px box is already what a Fluent
  16px nav glyph measures.

  Stroke and fill are set from CSS rather than from presentation attributes so
  the glyph colour can come from a token; `fill="var(--x)"` as an attribute is
  not resolved by WebKit.
-->
<script lang="ts">
  import { SECTIONS, type SectionId } from "../../settings/sections";

  interface Props {
    section: SectionId;
  }

  let { section }: Props = $props();

  const info = $derived(SECTIONS[section]);
  const uid = $props.id();
  const gradientId = `tile-${uid}`;
</script>

<span class="tile" aria-hidden="true">
  <svg viewBox="0 0 28 28" width="28" height="28">
    <defs>
      <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color={info.gradient[0]} />
        <stop offset="1" stop-color={info.gradient[1]} />
      </linearGradient>
    </defs>
    <rect class="plate" x="0" y="0" width="28" height="28" rx="7" fill="url(#{gradientId})" />
    <g class="glyph">
      {#if section === "general"}
        <circle cx="14" cy="14" r="4.7" />
        <circle cx="14" cy="14" r="1.9" />
        <path
          d="M14 7.3v2M14 18.7v2M20.7 14h-2M7.3 14h2M18.74 9.26l-1.42 1.42M10.68 17.32l-1.42 1.42M18.74 18.74l-1.42-1.42M10.68 10.68L9.26 9.26"
        />
      {:else if section === "system"}
        <rect x="7" y="8" width="14" height="9.6" rx="1.6" />
        <path d="M11 20.8h6M14 17.6v3.2" />
      {:else if section === "dictation"}
        <rect class="solid" x="11.4" y="7" width="5.2" height="9.6" rx="2.6" />
        <path d="M9 14.2a5 5 0 0 0 10 0M14 19.2v1.8M11.6 21h4.8" />
      {:else if section === "transcription"}
        <path d="M7.6 14h1.4M11.6 10.4v7.2M15.2 8v12M18.8 11.6v4.8M22 13.4v1.2" />
      {:else if section === "history"}
        <circle cx="14" cy="14" r="6.4" />
        <path d="M14 10.2V14l2.6 1.8" />
      {:else if section === "dictionary"}
        <path d="M8.4 8.2h8.4a2.4 2.4 0 0 1 2.4 2.4v9.2H10.8a2.4 2.4 0 0 1-2.4-2.4z" />
        <path d="M8.4 17.4a2.4 2.4 0 0 1 2.4-2.4h8.4M13 11.6h3.2" />
      {:else if section === "notes"}
        <rect x="8.2" y="7.4" width="11.6" height="13.2" rx="1.8" />
        <path d="M11 11h6M11 14h6M11 17h3.4" />
      {:else if section === "privacy"}
        <path
          d="M10.4 14.6V9.8a1.3 1.3 0 0 1 2.6 0v3.4V8.2a1.3 1.3 0 0 1 2.6 0v5V9.6a1.3 1.3 0 0 1 2.6 0v6.2a5 5 0 0 1-10 0v-1.6a1.3 1.3 0 0 1 2.2-.9z"
        />
      {/if}
    </g>
  </svg>
</span>

<style>
  .tile {
    display: inline-flex;
    flex: none;
    width: 28px;
    height: 28px;
  }

  .glyph :global(*) {
    fill: none;
    stroke: var(--text-on-accent);
    stroke-width: 1.35;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .glyph :global(.solid) {
    fill: var(--text-on-accent);
    stroke: none;
  }

  :global(:root[data-platform="windows"]) .plate {
    display: none;
  }

  :global(:root[data-platform="windows"]) .glyph :global(*) {
    stroke: var(--glyph-ink, var(--text-primary));
  }

  :global(:root[data-platform="windows"]) .glyph :global(.solid) {
    fill: var(--glyph-ink, var(--text-primary));
    stroke: none;
  }
</style>
