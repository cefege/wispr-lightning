<script lang="ts">
  import Divider from "../../lib/components/Divider.svelte";
  import GroupBox from "../../lib/components/GroupBox.svelte";
  import SegmentedControl from "../../lib/components/SegmentedControl.svelte";
  import Select from "../../lib/components/Select.svelte";
  import SettingRow from "../../lib/components/SettingRow.svelte";
  import Toggle from "../../lib/components/Toggle.svelte";
  import { updateSettings, type Settings } from "../../lib/ipc";

  interface Props {
    value: Settings;
  }

  let { value }: Props = $props();

  const TYPING_SPEEDS = [
    { value: "slow", label: "Slow" },
    { value: "normal", label: "Normal" },
    { value: "expert", label: "Expert" },
  ];

  const SIGNATURES = [
    { value: "written_with_lightning", label: "Written with Wispr Lightning" },
    { value: "spoken_with_lightning", label: "Spoken with Wispr Lightning" },
  ];
</script>

<GroupBox title="Transcript behavior">
  <SettingRow title="Voice commands" description={'Convert phrases like “new line” and “comma” into layout and punctuation'}>
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.commandModeEnabled}
        ariaDescribedby={describedBy}
        onchange={(on) => updateSettings((draft) => { draft.commandModeEnabled = on; })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow title="Auto-learn words" description="Learn distinctive vocabulary from corrected transcripts">
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.autoLearnWords}
        ariaDescribedby={describedBy}
        onchange={(on) => updateSettings((draft) => { draft.autoLearnWords = on; })}
      />
    {/snippet}
  </SettingRow>

  <Divider />

  <SettingRow title="Natural Mode" description="Type character by character instead of pasting">
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.naturalModeEnabled}
        ariaDescribedby={describedBy}
        onchange={(on) => updateSettings((draft) => { draft.naturalModeEnabled = on; })}
      />
    {/snippet}
  </SettingRow>

  {#if value.naturalModeEnabled}
    <div class="labelled">
      <span id="typing-speed-label">Typing speed</span>
      <SegmentedControl
        name="typing-speed"
        value={value.naturalModeSpeed}
        options={TYPING_SPEEDS}
        ariaLabelledby="typing-speed-label"
        onchange={(speed) => updateSettings((draft) => {
          draft.naturalModeSpeed = speed === "slow" ? "slow" : speed === "expert" ? "expert" : "normal";
        })}
      />
    </div>
    <p class="caption">Slow ≈ 30 WPM, Normal ≈ 50 WPM, Expert ≈ 80 WPM</p>
  {/if}
</GroupBox>

<Divider />

<GroupBox title="Email">
  <SettingRow title="Email signature" description="Append a short signature when dictating in email apps">
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.emailAutoSignature}
        ariaDescribedby={describedBy}
        onchange={(on) => updateSettings((draft) => { draft.emailAutoSignature = on; })}
      />
    {/snippet}
  </SettingRow>

  {#if value.emailAutoSignature}
    <div class="labelled">
      <label for="email-signature">Signature</label>
      <Select
        id="email-signature"
        value={value.emailSignatureOption}
        options={SIGNATURES}
        onchange={(option) => updateSettings((draft) => {
          draft.emailSignatureOption = option === "spoken_with_lightning"
            ? "spoken_with_lightning"
            : "written_with_lightning";
        })}
      />
    </div>
  {/if}
</GroupBox>

<style>
  .caption {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .labelled {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
