<!--
  System (ui-spec 3.5, MATRIX SET-075 to SET-082).

  The version string below the group is deliberately the hardcoded
  `Wispr Lightning v1.0.0` the Swift app showed, not the protocol client
  version — reproducing the inconsistency rather than quietly "fixing" it,
  because anything a support conversation might quote has to match.
-->
<script lang="ts">
  import Button from "../../lib/components/Button.svelte";
  import Divider from "../../lib/components/Divider.svelte";
  import GroupBox from "../../lib/components/GroupBox.svelte";
  import Select from "../../lib/components/Select.svelte";
  import SettingRow from "../../lib/components/SettingRow.svelte";
  import Toggle from "../../lib/components/Toggle.svelte";
  import { logFilePath, showInDockLabel } from "../../lib/platform";
  import {
    describe,
    restartOnboarding,
    saveSettingsNow,
    soundPacks,
    soundPreview,
    updateSettings,
    type Settings,
  } from "../../lib/ipc";

  interface Props {
    value: Settings;
  }

  let { value }: Props = $props();

  /** The default pack's option carries `null`; the empty string is its wire form. */
  const DEFAULT_PACK = "";

  let packs = $state<string[]>([]);
  let previewError = $state<string | null>(null);

  $effect(() => {
    void (async () => {
      try {
        packs = await soundPacks();
      } catch {
        // No enumeration available: fall back to the single Default pack,
        // which is what the Swift app showed when the folder was missing.
        packs = [];
      }
    })();
  });

  const capitalize = (name: string) => name.charAt(0).toUpperCase() + name.slice(1);

  const packOptions = $derived([
    { value: DEFAULT_PACK, label: "Default" },
    ...packs
      .filter((pack) => pack !== "default")
      .map((pack) => ({ value: pack, label: capitalize(pack) })),
  ]);

  /**
   * Save before previewing so the player reads the pack the user just picked
   * rather than the one still on disk. The Swift version achieved this with a
   * 200 ms delay after posting the change notification; awaiting the write is
   * the same idea without the guess.
   */
  async function preview() {
    previewError = null;
    try {
      await saveSettingsNow();
      await soundPreview(value.selectedSoundPack, "start");
    } catch (cause) {
      previewError = describe(cause);
    }
  }

  /**
   * Re-arm the first-launch walkthrough. Clearing the flag is all it takes:
   * `App.svelte` renders the wizard whenever it is false, so there is no
   * second piece of state that could disagree with the one on disk. A failed
   * write surfaces through the window's own save banner, and the wizard runs
   * from the in-memory value regardless.
   */
  const runSetupAgain = () => void restartOnboarding();
</script>

<GroupBox title="System">
  <SettingRow title="Launch at login">
    {#snippet control({ id })}
      <Toggle
        {id}
        checked={value.launchAtLogin}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.launchAtLogin = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow title={showInDockLabel}>
    {#snippet control({ id })}
      <Toggle
        {id}
        checked={value.showInDock}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.showInDock = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow title="Sound effects">
    {#snippet control({ id })}
      <Toggle
        {id}
        checked={value.enableSounds}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.enableSounds = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow title="Mute music while dictating">
    {#snippet control({ id })}
      <Toggle
        {id}
        checked={value.muteMusic}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.muteMusic = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <Divider />

  <SettingRow
    title="Verbose logging"
    description="Log full server requests and responses to {logFilePath}"
  >
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.verboseLogging}
        ariaDescribedby={describedBy}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.verboseLogging = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <Divider />

  <div class="pack-row">
    <label for="sound-pack">Sound pack</label>
    <Select
      id="sound-pack"
      value={value.selectedSoundPack ?? DEFAULT_PACK}
      options={packOptions}
      onchange={(pack) =>
        updateSettings((draft) => {
          draft.selectedSoundPack = pack === DEFAULT_PACK ? null : pack;
        })}
    />
    <Button onclick={preview}>Preview</Button>
  </div>

  {#if previewError}
    <p class="error" role="alert">{previewError}</p>
  {/if}

  <Divider />

  <SettingRow
    title="First-run setup"
    description="Walk through permissions, the dictation key and Deepgram setup again."
  >
    {#snippet control()}
      <Button onclick={runSetupAgain}>Run Setup Again</Button>
    {/snippet}
  </SettingRow>
</GroupBox>

<Divider />

<p class="version">Wispr Lightning v2.0.0</p>

<style>
  .pack-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .version {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-tertiary);
  }

  .error {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--danger);
  }
</style>
