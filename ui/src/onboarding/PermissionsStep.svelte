<!--
  The first and blocking setup step.

  Native permission requests are deliberately serialized. macOS otherwise
  stacks unrelated TCC prompts before the user has seen why they are needed,
  and Windows may route a denied microphone grant through Settings. Polling is
  required because those decisions happen in another process and do not send
  an event back to the webview.
-->
<script lang="ts">
  import Button from "../lib/components/Button.svelte";
  import ErrorBanner from "../lib/components/ErrorBanner.svelte";
  import { isWindows } from "../lib/platform";
  import {
    describe,
    permissionsOpenSettings,
    permissionsRequest,
    permissionsStatus,
    type PermissionState,
    type Settings,
  } from "../lib/ipc";

  interface Props {
    value: Settings;
    onreadychange: (ready: boolean) => void;
  }

  interface RequiredPermission {
    /** The wire name `permissions_status` keys its map by. */
    key: string;
    name: string;
    /** Why this one, in the order the app uses them. */
    why: string;
  }

  let { value, onreadychange }: Props = $props();

  const MACOS: readonly RequiredPermission[] = [
    { key: "microphone", name: "Microphone", why: "So it can hear you." },
    { key: "input_monitoring", name: "Input Monitoring", why: "So it can see the key you hold." },
    {
      key: "accessibility",
      name: "Accessibility",
      why: "So it can type into the app in front of you.",
    },
    {
      key: "screen_recording",
      name: "Screen Recording",
      why: "So it can recognize names and technical terms visible on screen.",
    },
  ];

  const WINDOWS: readonly RequiredPermission[] = [
    { key: "microphone", name: "Microphone", why: "So it can hear you." },
  ];

  /** How often to re-check while the native prompt or System Settings is open. */
  const POLL_MS = 1000;

  let statuses = $state<Record<string, PermissionState>>({});
  let error = $state<string | null>(null);
  let loaded = $state(false);
  let activeKey = $state<string | null>(null);
  let requested = $state<string[]>([]);

  const required = $derived(
    isWindows
      ? WINDOWS
      : value.useScreenContext
        ? MACOS
        : MACOS.filter((permission) => permission.key !== "screen_recording"),
  );

  /** Anything the platform says does not apply here is noise on this screen. */
  const rows = $derived(
    required
      .map((permission) => ({
        ...permission,
        state: statuses[permission.key],
      }))
      .filter(
        (row): row is RequiredPermission & { state: PermissionState } =>
          row.state !== undefined && row.state !== "not_applicable",
      ),
  );

  const outstanding = $derived(rows.filter((row) => row.state !== "granted").length);
  const ready = $derived(loaded && error === null && outstanding === 0);

  async function refresh() {
    try {
      statuses = await permissionsStatus();
      error = null;
    } catch (cause) {
      error = describe(cause);
    } finally {
      loaded = true;
    }
  }

  async function act(action: () => Promise<unknown>) {
    try {
      await action();
      error = null;
    } catch (cause) {
      error = describe(cause);
    }
    await refresh();
  }

  function request(key: string) {
    activeKey = key;
    if (!requested.includes(key)) requested = [...requested, key];
    void act(() => permissionsRequest(key));
  }

  function stateLabel(row: (typeof rows)[number]): string {
    if (row.state === "granted") return "Granted";
    if (row.key !== activeKey) return "Waiting";
    if (row.state === "denied") return "Needs approval";
    return "Awaiting decision";
  }

  $effect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });

  $effect(() => {
    onreadychange(ready);
  });

  // A successful grant advances the queue. Only this effect starts requests,
  // which guarantees there is never more than one native prompt in flight.
  $effect(() => {
    if (!loaded || error !== null || ready) return;

    const active = rows.find((row) => row.key === activeKey);
    if (active && active.state !== "granted") return;

    const next = rows.find((row) => row.state !== "granted");
    if (next && !requested.includes(next.key)) request(next.key);
  });
</script>

<p class="lead">
  {#if isWindows}
    Approve microphone access to continue. Windows Settings will open if access was previously
    denied.
  {:else}
    Approve each macOS request in order. Setup continues only after every permission required by
    your current configuration is granted.
  {/if}
</p>

{#if error}
  <ErrorBanner message={error} onretry={refresh} />
{/if}

{#if rows.length === 0}
  <p class="footnote">{loaded ? "Nothing to grant on this system." : "Checking permissions…"}</p>
{:else}
  <ul class="permissions">
    {#each rows as row (row.key)}
      <li class="permission">
        <span class="text">
          <span class="name">{row.name}</span>
          <span class="why">{row.why}</span>
        </span>
        <span
          class="state"
          data-state={row.state}
          data-active={row.key === activeKey}>{stateLabel(row)}</span
        >
        {#if row.key === activeKey && row.state === "not_determined"}
          <Button onclick={() => request(row.key)}>Request Access</Button>
        {:else if row.key === activeKey && row.state === "denied"}
          <Button onclick={() => act(() => permissionsOpenSettings(row.key))}>
            Open Settings
          </Button>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<p class="footnote" role="status">
  {#if ready}
    All required permissions are granted. Continue to choose your dictation key.
  {:else if loaded}
    Finish the highlighted permission to unlock the next request.
  {:else}
    Checking permissions…
  {/if}
</p>

<style>
  .lead {
    margin: 0;
    color: var(--text-primary);
  }

  .permissions {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 100%;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .permission {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .text {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    color: var(--text-primary);
  }

  .why {
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .state {
    flex: none;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }

  .state[data-state="granted"] {
    color: var(--success);
  }

  .state[data-state="denied"][data-active="true"] {
    color: var(--danger);
  }

  .state[data-state="not_determined"][data-active="true"] {
    color: var(--warning);
  }

  .footnote {
    margin: 0;
    font-size: var(--text-subheadline);
    color: var(--text-secondary);
  }
</style>
