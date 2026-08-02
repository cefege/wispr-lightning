<!--
  Privacy (ui-spec 3.5, MATRIX SET-072 to SET-074), plus the system-permission
  readout.

  The permission block is additive — the Swift app had no such UI, which is
  precisely the defect MATRIX HTK-032, LOG-012 and LIF-016 record: denied
  Input Monitoring made the app look alive while it silently never triggered.
  The port makes `Permissions::status()` queryable, and this is where the
  answer is shown, next to the two settings that actually depend on it.
-->
<script lang="ts">
  import Button from "../../lib/components/Button.svelte";
  import ErrorBanner from "../../lib/components/ErrorBanner.svelte";
  import GroupBox from "../../lib/components/GroupBox.svelte";
  import SettingRow from "../../lib/components/SettingRow.svelte";
  import Toggle from "../../lib/components/Toggle.svelte";
  import {
    describe,
    permissionsOpenSettings,
    permissionsRequest,
    permissionsStatus,
    updateSettings,
    type PermissionState,
    type Settings,
  } from "../../lib/ipc";

  interface Props {
    value: Settings;
  }

  let { value }: Props = $props();

  let permissions = $state<Record<string, PermissionState>>({});
  let permissionError = $state<string | null>(null);
  let loaded = $state(false);

  /** Known keys get a human name; anything new is shown rather than dropped. */
  const PERMISSION_LABELS: Record<string, string> = {
    microphone: "Microphone",
    accessibility: "Accessibility",
    input_monitoring: "Input Monitoring",
    screen_recording: "Screen Recording",
    automation: "Automation",
  };

  const STATE_LABELS: Record<PermissionState, string> = {
    granted: "Granted",
    denied: "Denied",
    not_determined: "Not requested",
    not_applicable: "Not required on this system",
  };

  function labelFor(key: string): string {
    return (
      PERMISSION_LABELS[key] ??
      key.replace(/_/g, " ").replace(/^./, (first) => first.toUpperCase())
    );
  }

  async function refreshPermissions() {
    try {
      permissions = await permissionsStatus();
      permissionError = null;
    } catch (cause) {
      permissionError = describe(cause);
    } finally {
      loaded = true;
    }
  }

  $effect(() => {
    void refreshPermissions();
  });

  async function act(action: () => Promise<unknown>) {
    try {
      await action();
      permissionError = null;
    } catch (cause) {
      permissionError = describe(cause);
    }
    await refreshPermissions();
  }

  const entries = $derived(Object.entries(permissions));
</script>

<GroupBox title="Privacy">
  <SettingRow
    title="Screen context (OCR)"
    description="Capture screen text for context-aware formatting"
  >
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.useScreenContext}
        ariaDescribedby={describedBy}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.useScreenContext = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow
    title="Accessibility context"
    description="Use accessibility APIs for better transcription context"
  >
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.useAccessibilityContext}
        ariaDescribedby={describedBy}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.useAccessibilityContext = on;
          })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow
    title="Share anonymous usage data"
    description="Help improve Wispr by sharing anonymous statistics"
  >
    {#snippet control({ id, describedBy })}
      <Toggle
        {id}
        checked={value.shareUsageData}
        ariaDescribedby={describedBy}
        onchange={(on) =>
          updateSettings((draft) => {
            draft.shareUsageData = on;
          })}
      />
    {/snippet}
  </SettingRow>
</GroupBox>

<GroupBox title="System Permissions">
  <p class="caption">
    Dictation needs these to hear you, to see the key you press, and to type into the app in front
    of you. A denial here is silent otherwise — the app simply never triggers.
  </p>

  {#if permissionError}
    <ErrorBanner message={permissionError} onretry={refreshPermissions} />
  {:else if entries.length === 0}
    <p class="caption">{loaded ? "No permissions to configure." : "Checking…"}</p>
  {:else}
    <ul class="permissions">
      {#each entries as [key, state] (key)}
        <li class="permission">
          <span class="name">{labelFor(key)}</span>
          <span class="state" data-state={state}>{STATE_LABELS[state]}</span>
          {#if state === "not_determined"}
            <Button onclick={() => act(() => permissionsRequest(key))}>Request Access</Button>
          {:else if state === "denied"}
            <Button onclick={() => act(() => permissionsOpenSettings(key))}>Open Settings</Button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</GroupBox>

<style>
  .caption {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .permissions {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .permission {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .name {
    flex: 1 1 auto;
    min-width: 0;
  }

  .state {
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .state[data-state="granted"] {
    color: var(--success);
  }

  .state[data-state="denied"] {
    color: var(--danger);
  }

  .state[data-state="not_determined"] {
    color: var(--warning);
  }
</style>
