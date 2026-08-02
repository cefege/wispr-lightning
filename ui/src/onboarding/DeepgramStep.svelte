<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import ErrorBanner from "../lib/components/ErrorBanner.svelte";
  import TextField from "../lib/components/TextField.svelte";
  import {
    describe,
    deepgramKeySave,
    deepgramStatus,
    type DeepgramStatus,
  } from "../lib/ipc";
  interface Props {
    onreadychange: (ready: boolean) => void;
  }

  let { onreadychange }: Props = $props();
  let status = $state<DeepgramStatus | null>(null);
  let apiKey = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  const configured = $derived(status?.configured ?? false);

  $effect(() => {
    onreadychange(configured);
  });

  async function refresh() {
    try {
      status = await deepgramStatus();
      error = null;
    } catch (cause) {
      error = describe(cause);
    }
  }

  $effect(() => {
    void refresh();
  });

  async function save() {
    if (!apiKey.trim()) return;
    busy = true;
    error = null;
    try {
      await deepgramKeySave(apiKey);
      apiKey = "";
      await refresh();
    } catch (cause) {
      error = describe(cause);
    } finally {
      busy = false;
    }
  }
</script>

<p class="lead">Wispr Lightning uses Deepgram for fast, live speech recognition.</p>
<p class="footnote">Paste a Deepgram API key to finish setup. You can replace it later in Settings › Transcription.</p>

{#if error}
  <ErrorBanner message={error} onretry={refresh} />
{/if}

<div class="key-block">
  <label for="onboarding-deepgram-key">Deepgram API key</label>
  <div class="key-row">
    <TextField
      id="onboarding-deepgram-key"
      type="password"
      bind:value={apiKey}
      placeholder={configured ? "Saved key ••••••••••••" : "Paste Deepgram API key"}
      autocomplete="off"
      onkeydown={(event) => {
        if (event.key === "Enter") void save();
      }}
    />
    <Button variant="accent" disabled={busy || apiKey.trim() === ""} onclick={() => void save()}>
      {configured ? "Replace" : "Save key"}
    </Button>
  </div>
</div>

{#if configured}
  <p class="saved" role="status">Deepgram API key saved. Setup is ready to finish.</p>
{:else}
  <p class="footnote">The key is write-only: after saving, the app shows a masked saved state instead of revealing it.</p>
{/if}

<style>
  .lead { margin: 0; color: var(--text-primary); }
  .footnote { margin: 0; font-size: var(--text-subheadline); color: var(--text-secondary); }
  .key-block { display: grid; width: 100%; gap: var(--space-2); }
  .key-row { display: flex; align-items: center; gap: var(--space-2); }
  .key-row :global(.field) { flex: 1; }
  .saved { margin: 0; color: var(--success); font-size: var(--text-subheadline); }
</style>
