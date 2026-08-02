<script lang="ts">
  import Button from "../../lib/components/Button.svelte";
  import Divider from "../../lib/components/Divider.svelte";
  import ErrorBanner from "../../lib/components/ErrorBanner.svelte";
  import GroupBox from "../../lib/components/GroupBox.svelte";
  import Select from "../../lib/components/Select.svelte";
  import SettingRow from "../../lib/components/SettingRow.svelte";
  import TextField from "../../lib/components/TextField.svelte";
  import Toggle from "../../lib/components/Toggle.svelte";
  import {
    describe,
    deepgramBalance,
    deepgramHealth,
    deepgramKeyClear,
    deepgramKeySave,
    deepgramStatus,
    updateSettings,
    type DeepgramBalance,
    type DeepgramStatus,
    type Settings,
  } from "../../lib/ipc";
  import { LANGUAGES } from "../languages";

  interface Props {
    value: Settings;
  }

  let { value }: Props = $props();

  const MODELS = [
    { value: "nova-3", label: "Nova 3 — recommended" },
    { value: "nova-2", label: "Nova 2" },
  ];

  // `__auto__` and `__multi__` both send `language=multi` to the streaming
  // endpoint, so offering both was two labels for one behaviour. Only the
  // canonical sentinel is listed; legacy `__multi__` is handled below.
  const DEEPGRAM_LANGUAGES = [
    { value: "__auto__", label: "Auto-detect (multilingual)" },
    ...LANGUAGES.map((language) => ({ value: language.code, label: language.name })),
  ];

  // A settings file written before the two options were merged still holds
  // `__multi__`. It behaves identically, so show it as the auto entry rather
  // than leaving the picker blank on a value it no longer lists.
  const selectedLanguage = $derived(
    value.deepgramLanguage === "__multi__" ? "__auto__" : value.deepgramLanguage,
  );

  let status = $state<DeepgramStatus | null>(null);
  let loadError = $state<string | null>(null);
  let apiKey = $state("");
  let keyBusy = $state(false);
  let keyError = $state<string | null>(null);
  let health = $state<{ ok: boolean; message: string } | null>(null);
  let balance = $state<DeepgramBalance | null>(null);
  let balanceError = $state<string | null>(null);
  let balanceBusy = $state(false);
  let healthBusy = $state(false);

  const configured = $derived(status?.configured ?? false);
  const supportsKeyterm = $derived(value.deepgramModel.toLowerCase().startsWith("nova-3"));

  async function refresh() {
    try {
      status = await deepgramStatus();
      loadError = null;
    } catch (cause) {
      loadError = describe(cause);
    }
  }

  $effect(() => {
    void refresh();
  });

  async function saveKey() {
    if (!apiKey.trim()) return;
    keyBusy = true;
    keyError = null;
    try {
      await deepgramKeySave(apiKey);
      apiKey = "";
      health = null;
      await refresh();
    } catch (cause) {
      keyError = describe(cause);
    } finally {
      keyBusy = false;
    }
  }

  async function clearKey() {
    keyBusy = true;
    keyError = null;
    try {
      await deepgramKeyClear();
      apiKey = "";
      health = null;
      await refresh();
    } catch (cause) {
      keyError = describe(cause);
    } finally {
      keyBusy = false;
    }
  }

  async function testConnection() {
    healthBusy = true;
    try {
      health = await deepgramHealth();
    } catch (cause) {
      health = { ok: false, message: describe(cause) };
    } finally {
      healthBusy = false;
    }
  }

  async function loadBalance() {
    balanceBusy = true;
    balanceError = null;
    try {
      balance = await deepgramBalance();
    } catch (cause) {
      balance = null;
      balanceError = describe(cause);
    } finally {
      balanceBusy = false;
    }
  }

  // Deepgram reports `usd` for pay-as-you-go credit and `hour` for committed
  // volume, so the unit decides the rendering instead of being assumed.
  const balanceLabel = $derived.by(() => {
    if (!balance) return "";
    const { amount, units } = balance;
    if (units === "usd") return `$${amount.toFixed(2)}`;
    if (units === "hour") return `${amount.toFixed(1)} hours`;
    return `${amount} ${units}`;
  });
</script>

<GroupBox title="Deepgram">
  <div class="deepgram-head">
    <div>
      <strong>Live streaming transcription</strong>
      <p>Audio is sent directly to Deepgram while you dictate.</p>
    </div>
    <span class="status" data-ok={configured}>{configured ? "API key saved" : "API key required"}</span>
  </div>

  {#if loadError}
    <ErrorBanner message={loadError} onretry={refresh} />
  {/if}

  <Divider />

  <div class="key-block">
    <label for="deepgram-key">API key</label>
    <div class="key-row">
      <TextField
        id="deepgram-key"
        type="password"
        bind:value={apiKey}
        placeholder={configured ? "Saved key ••••••••••••" : "Paste Deepgram API key"}
        autocomplete="off"
        onkeydown={(event) => {
          if (event.key === "Enter") void saveKey();
        }}
      />
      <Button variant="accent" disabled={keyBusy || apiKey.trim() === ""} onclick={() => void saveKey()}>
        {configured ? "Replace" : "Save key"}
      </Button>
      {#if configured}
        <Button disabled={keyBusy} onclick={() => void clearKey()}>Clear</Button>
      {/if}
    </div>
    <p class="caption">Stored locally in the app data folder. The saved value is never revealed.</p>
    {#if keyError}<p class="error" role="alert">{keyError}</p>{/if}
  </div>

  {#if configured}
    <Divider />

    <div class="credits">
      <div class="credits-head">
        <div>
          <strong>Credits remaining</strong>
          {#if balance}
            <p>{balance.projectName}</p>
          {:else}
            <p>Prepaid balance on your Deepgram account.</p>
          {/if}
        </div>
        <div class="credits-value">
          {#if balance}<span class="amount">{balanceLabel}</span>{/if}
          <Button disabled={balanceBusy} onclick={() => void loadBalance()}>
            {balanceBusy ? "Checking…" : balance ? "Refresh" : "Check"}
          </Button>
        </div>
      </div>
      {#if balanceError}<p class="error" role="alert">{balanceError}</p>{/if}
    </div>
  {/if}

  <Divider />

  <div class="labelled">
    <label for="deepgram-model">Model</label>
    <Select
      id="deepgram-model"
      value={value.deepgramModel}
      options={MODELS}
      onchange={(model) => updateSettings((draft) => { draft.deepgramModel = model; })}
    />
  </div>

  <div class="labelled">
    <label for="deepgram-language">Language</label>
    <Select
      id="deepgram-language"
      value={selectedLanguage}
      options={DEEPGRAM_LANGUAGES}
      onchange={(language) => updateSettings((draft) => { draft.deepgramLanguage = language; })}
    />
  </div>

  <p class="caption">Auto-detect streams Deepgram's multilingual model, which recognises several languages in one dictation; its per-language coverage is narrower than picking a fixed language.</p>

  <SettingRow
    title="Contextual recognition hints"
    description={supportsKeyterm
      ? "Send dictionary terms and distinctive words from the focused app. Screen OCR is included when enabled in Privacy."
      : "Nova 3 is required. The current model will not receive dictionary or context keyterms."}
  >
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.deepgramKeytermBoost}
        disabled={!supportsKeyterm}
        ariaDescribedby={describedBy}
        onchange={(on) => updateSettings((draft) => { draft.deepgramKeytermBoost = on; })}
      />
    {/snippet}
  </SettingRow>

  <Divider />

  <div class="test-row">
    <Button disabled={healthBusy || !configured} onclick={() => void testConnection()}>
      {healthBusy ? "Testing…" : "Test connection"}
    </Button>
    {#if health}
      <span class="health" data-ok={health.ok}>{health.message}</span>
    {/if}
  </div>
</GroupBox>

<GroupBox title="Transcript processing">
  <p class="caption">Deepgram returns recognizer text. Wispr Lightning then applies your replacements and snippets locally, followed by capitalization and terminal punctuation.</p>
</GroupBox>

<style>
  .deepgram-head,
  .credits-head,
  .key-row,
  .labelled,
  .test-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .deepgram-head, .credits-head { justify-content: space-between; }
  .deepgram-head p,
  .credits-head p,
  .caption { margin: 0; color: var(--text-secondary); font-size: var(--text-subheadline); }
  .credits { display: grid; gap: var(--space-2); }
  .credits-value { display: flex; align-items: center; gap: var(--space-2); }
  .amount { font-size: var(--text-title3); font-variant-numeric: tabular-nums; }
  .key-block { display: grid; gap: var(--space-2); }
  .key-row :global(.field) { flex: 1; }
  .labelled label { min-width: 7rem; }
  .labelled :global(select) { flex: 1; }
  .status { font-size: var(--text-caption); color: var(--warning); }
  .status[data-ok="true"], .health[data-ok="true"] { color: var(--success); }
  .health[data-ok="false"], .error { color: var(--danger); }
  .health { font-size: var(--text-subheadline); }
  .error { margin: 0; font-size: var(--text-subheadline); }
</style>
