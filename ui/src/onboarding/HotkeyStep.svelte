<!--
  Step 3: the key you hold.

  There is nothing to choose here unless the user wants to choose: the shipped
  default is already bound and already shown as a keycap, so the honest primary
  action is Continue. `KeyCapture` is the same component the General pane uses,
  which means the recording happens in the backend and a bare modifier — the
  shipped default, and something a webview `keydown` cannot see on macOS — is
  capturable from here too.
-->
<script lang="ts">
  import KeyCapture from "../lib/components/KeyCapture.svelte";
  import { updateSettings, type Hotkey, type Settings } from "../lib/ipc";

  interface Props {
    value: Settings;
    /** Forwarded so the wizard stops treating Return as its default action. */
    oncapturingchange: (capturing: boolean) => void;
  }

  let { value, oncapturingchange }: Props = $props();

  function setHotkeys(hotkeys: Hotkey[]) {
    updateSettings((draft) => {
      draft.hotkeys = hotkeys;
    });
  }
</script>

<p class="lead">
  Hold this key while you talk. Let go, and what you said is typed where your cursor is.
</p>

<KeyCapture
  hotkeys={value.hotkeys}
  addLabel="Choose a Different Key"
  removeTooltip="Remove this hotkey"
  ariaLabel="Dictation hotkeys"
  onchange={setHotkeys}
  {oncapturingchange}
/>

<p class="footnote">
  Keep the default if it suits you. Extra keys, and what a quick tap does, are in Settings.
</p>

<style>
  .lead {
    margin: 0;
    color: var(--text-primary);
  }

  .footnote {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }
</style>
