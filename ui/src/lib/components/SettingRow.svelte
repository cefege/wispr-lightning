<!--
  The reusable settings row (ui-spec 3.4): a leading title with an optional
  secondary description, and the control flush right.

  The row owns the ids and hands them to the control snippet, which is what
  makes the association real rather than decorative. Two are provided because
  not every control can be pointed at by `<label for>`:

  - `id`    — put it on a single focusable element (`<input>`, `<select>`) and
              the title becomes its `<label for>`.
  - `labelId` — for composite controls (a radio group, a row of buttons), use
              `aria-labelledby={labelId}` on the group instead.

  A description, when present, is wired through `aria-describedby` so a screen
  reader reads the same sentence a sighted user does.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface ControlIds {
    id: string;
    labelId: string;
    describedBy: string | undefined;
  }

  interface Props {
    title: string;
    description?: string;
    /** Renders the title in medium weight, as the Auto-detect row does. */
    emphasized?: boolean;
    /** Dims the text alongside a control that is itself `disabled`. */
    disabled?: boolean;
    control: Snippet<[ControlIds]>;
  }

  let { title, description, emphasized = false, disabled = false, control }: Props = $props();

  const uid = $props.id();
  const id = `row-${uid}`;
  const labelId = `row-${uid}-label`;
  const descId = `row-${uid}-desc`;
  const describedBy = $derived(description ? descId : undefined);
</script>

<div class="row" class:disabled>
  <div class="text">
    <label class="title" class:emphasized for={id} id={labelId}>{title}</label>
    {#if description}
      <span class="description" id={descId}>{description}</span>
    {/if}
  </div>
  <div class="control">
    {@render control({ id, labelId, describedBy })}
  </div>
</div>

<style>
  .row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    width: 100%;
  }

  .text {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .title {
    font-size: var(--text-body);
    color: var(--text-primary);
  }

  .title.emphasized {
    font-weight: var(--weight-medium);
  }

  .description {
    font-size: var(--text-subheadline);
    font-weight: var(--weight-regular);
    color: var(--text-secondary);
  }

  .control {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-2);
  }

  .row.disabled .title,
  .row.disabled .description {
    opacity: 0.45;
  }
</style>
