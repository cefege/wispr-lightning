<!--
  A titled box, matching SwiftUI's `GroupBox`: the title sits outside and above
  the frame, the content inside gets 8pt of padding and an 8pt stack gap.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    /** Groups that need a tighter inner rhythm, e.g. the language list. */
    gap?: "medium" | "small";
    children: Snippet;
  }

  let { title, gap = "medium", children }: Props = $props();

  // A stable id so a control inside the group can point at the group heading
  // when it has no visible label of its own.
  const uid = $props.id();
  const headingId = `group-${uid}`;
</script>

<section class="group" aria-labelledby={headingId}>
  <h2 class="title" id={headingId}>{title}</h2>
  <div class="box" class:tight={gap === "small"}>
    {@render children()}
  </div>
</section>

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: 100%;
  }

  .title {
    margin: 0;
    font-size: var(--text-title);
    font-weight: var(--weight-semibold);
    color: var(--text-primary);
  }

  .box {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .box.tight {
    gap: var(--space-1);
  }
</style>
