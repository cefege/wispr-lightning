<!--
  A right-click menu.

  Positioned at the pointer and clamped to the viewport, so a menu opened on
  the last row of a list does not hang off the bottom of the window. It closes
  on Escape, on any click outside it, and on scroll — the last of those because
  a menu anchored to a pointer position is meaningless once the row it belongs
  to has moved.

  There is no submenu support and no icons: every menu in this app is two items
  and a divider.
-->
<script lang="ts">
  export interface MenuItem {
    label: string;
    destructive?: boolean;
    onclick: () => void;
  }

  interface Props {
    x: number;
    y: number;
    /** `null` renders a divider. */
    items: ReadonlyArray<MenuItem | null>;
    onclose: () => void;
  }

  let { x, y, items, onclose }: Props = $props();

  let menu: HTMLDivElement | undefined = $state();
  /**
   * Null until the menu has been measured. The pointer position alone is not
   * enough — a menu opened near the right or bottom edge has to be pulled back
   * inside the window, and that needs its rendered size.
   */
  let placement = $state<{ left: number; top: number } | null>(null);

  $effect(() => {
    const el = menu;
    if (el === undefined) return;
    const { width, height } = el.getBoundingClientRect();
    placement = {
      left: Math.max(4, Math.min(x, window.innerWidth - width - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - height - 4)),
    };
    el.focus();
  });
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape") onclose();
  }}
  onresize={onclose}
/>

<!-- Captures the click that dismisses the menu so it cannot also land on
     whatever was underneath, which would open an editor on the way out. -->
<div
  class="scrim"
  role="presentation"
  onpointerdown={onclose}
  oncontextmenu={(event) => {
    event.preventDefault();
    onclose();
  }}
  onwheel={onclose}
></div>

<div
  bind:this={menu}
  class="menu"
  role="menu"
  tabindex="-1"
  class:placed={placement !== null}
  style:left="{placement?.left ?? x}px"
  style:top="{placement?.top ?? y}px"
>
  {#each items as item, index (index)}
    {#if item === null}
      <div class="divider" role="separator"></div>
    {:else}
      <button
        class="item"
        class:destructive={item.destructive}
        type="button"
        role="menuitem"
        onclick={() => {
          item.onclick();
          onclose();
        }}
      >
        {item.label}
      </button>
    {/if}
  {/each}
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
  }

  /* Laid out but not painted for the one frame it takes to measure, so the
     edge-clamped position is never visible as a jump. */
  .menu:not(.placed) {
    visibility: hidden;
  }

  .menu {
    position: fixed;
    z-index: 41;
    min-width: 140px;
    padding: var(--space-1) 0;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    outline: none;
  }

  .item {
    display: block;
    width: 100%;
    padding: 3px var(--space-3);
    font-family: inherit;
    font-size: var(--text-body);
    color: var(--text-primary);
    text-align: left;
    cursor: default;
    background: none;
    border: 0;
  }

  .item:hover {
    color: var(--text-on-accent);
    background: var(--accent);
  }

  .item.destructive {
    color: var(--danger);
  }

  .item.destructive:hover {
    color: var(--text-on-accent);
    background: var(--danger);
  }

  .divider {
    height: 1px;
    margin: var(--space-1) 0;
    background: var(--border);
  }
</style>
