<!-- Hotkey and microphone configuration. -->
<script lang="ts">
  import Button from "../../lib/components/Button.svelte";
  import Divider from "../../lib/components/Divider.svelte";
  import ErrorBanner from "../../lib/components/ErrorBanner.svelte";
  import GroupBox from "../../lib/components/GroupBox.svelte";
  import KeyCapture from "../../lib/components/KeyCapture.svelte";
  import Select from "../../lib/components/Select.svelte";
  import SettingRow from "../../lib/components/SettingRow.svelte";
  import Toggle from "../../lib/components/Toggle.svelte";
  import {
    audioDevices,
    describe,
    onDevicesChanged,
    updateSettings,
    type Hotkey,
    type InputDevice,
    type PressBehaviorValue,
    type Settings,
  } from "../../lib/ipc";

  interface Props {
    value: Settings;
  }

  let { value }: Props = $props();

  let devices = $state<InputDevice[]>([]);
  let deviceError = $state<string | null>(null);

  async function refreshDevices() {
    try {
      devices = await audioDevices();
      deviceError = null;
    } catch (cause) {
      deviceError = describe(cause);
    }
  }

  $effect(() => {
    void refreshDevices();
    return onDevicesChanged(() => void refreshDevices());
  });

  /**
   * `null` is a real, distinct value here — "follow the system default" — so
   * it gets its own option rather than being conflated with "unset". The empty
   * string is the wire form because HTML option values are strings.
   */
  const SYSTEM_DEFAULT = "";

  const deviceOptions = $derived([
    { value: SYSTEM_DEFAULT, label: "System Default" },
    ...devices.map((device) => ({ value: device.id, label: device.name })),
  ]);

  function selectDevice(id: string) {
    const device = devices.find((d) => d.id === id);
    updateSettings((draft) => {
      // The pair is written together: micDeviceId resolves the device and
      // micDeviceName is a display hint only, so they must never disagree.
      draft.micDeviceId = id === SYSTEM_DEFAULT ? null : id;
      draft.micDeviceName = device?.name ?? null;
    });
  }

  function setHotkeys(next: Hotkey[]) {
    updateSettings((draft) => {
      draft.hotkeys = next;
    });
  }

  /**
   * `wl_core::fsm::PressBehavior` (v2-ui-spec 3.3.2, B-015). The three
   * behaviours differ in exactly one thing — what a *quick* tap, a release
   * under half a second, means. A hold of half a second or more is
   * push-to-talk in all three, which is why two of the notes say so.
   *
   * The spec's label for the default is `"Hold or double-tap to lock
   * (legacy)"`; the parenthetical is dropped because "legacy" names the wire
   * tag, not anything the user can act on. The tag itself is unchanged.
   */
  const PRESS_BEHAVIOR_NOTES: Record<PressBehaviorValue, string> = {
    hold: "Recording lasts as long as the key is held; releasing always ends it. A quick tap stops recording immediately.",
    toggle:
      "A quick tap starts recording hands-free and the next press stops it. Holding the key is still push-to-talk.",
    legacy:
      "A quick tap keeps recording until half a second after the press and then stops; tapping again inside that window locks hands-free. Holding the key is still push-to-talk.",
  };

  const PRESS_BEHAVIOR_OPTIONS = [
    { value: "hold", label: "Hold to talk" },
    { value: "toggle", label: "Tap to start, tap to stop" },
    { value: "legacy", label: "Hold or double-tap to lock" },
  ];

  const pressBehaviorNote = $derived(PRESS_BEHAVIOR_NOTES[value.hotkeyPressBehavior]);

  function isPressBehavior(raw: string): raw is PressBehaviorValue {
    return Object.hasOwn(PRESS_BEHAVIOR_NOTES, raw);
  }

  function setPressBehavior(next: string) {
    // A `<select>` hands back a bare string. Only the three wire tags are
    // written: anything else would be silently demoted to legacy by the
    // backend, so it is better never to store it.
    if (!isPressBehavior(next)) return;
    updateSettings((draft) => {
      // `hotkeyTapToToggle` is deliberately untouched — it is a deprecated
      // mirror that the backend keeps in sync with this field.
      draft.hotkeyPressBehavior = next;
    });
  }

</script>


<GroupBox title="Dictation Hotkeys">
  <p class="lead">Any of these keys will start dictation:</p>

  <KeyCapture
    hotkeys={value.hotkeys}
    addLabel="Add Hotkey"
    removeTooltip="Remove this hotkey"
    ariaLabel="Dictation hotkeys"
    onchange={setHotkeys}
  />

  <p class="footnote">
    Modifier keys work as hold-to-talk. Regular keys use press-to-toggle.
  </p>

  <Divider />

  <SettingRow title="Press behavior" description={pressBehaviorNote}>
    {#snippet control({ id, describedBy })}
      <Select
        {id}
        value={value.hotkeyPressBehavior}
        options={PRESS_BEHAVIOR_OPTIONS}
        ariaDescribedby={describedBy}
        maxWidth="260px"
        onchange={setPressBehavior}
      />
    {/snippet}
  </SettingRow>
</GroupBox>

<Divider />

<GroupBox title="Input Device">
  {#if deviceError}
    <ErrorBanner message={deviceError} onretry={refreshDevices} />
  {/if}

  <div class="device-row">
    <Select
      value={value.micDeviceId ?? SYSTEM_DEFAULT}
      options={deviceOptions}
      ariaLabel="Input device"
      maxWidth="320px"
      onchange={selectDevice}
    />
    <Button onclick={refreshDevices}>
      <svg viewBox="0 0 14 14" width="12" height="12" aria-hidden="true">
        <path d="M12 7a5 5 0 1 1-1.6-3.7" />
        <path d="M12.2 1.6v3h-3" />
      </svg>
      Refresh
    </Button>
  </div>

  <Divider />

  <SettingRow
    title="Keep microphone active"
    description="Eliminates startup delay — recommended when using iPhone as microphone"
  >
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.keepMicrophoneActive}
        ariaDescribedby={describedBy}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.keepMicrophoneActive = on;
          })}
      />
    {/snippet}
  </SettingRow>
</GroupBox>


<style>
  .lead {
    margin: 0;
    color: var(--text-secondary);
  }

  .footnote {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-tertiary);
  }

  .device-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .device-row svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
