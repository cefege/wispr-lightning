<!--
  Step 1: what this app is, in one sentence.

  The second paragraph exists because the app has no window of its own at
  rest. On macOS a menu-bar app is a familiar idiom; on Windows the
  notification area is where it lives and is far less discoverable, so both
  platforms get told plainly where to look for it afterwards.
-->
<script lang="ts">
  import { isWindows } from "../lib/platform";

  const home = isWindows ? "the notification area, by the clock" : "the menu bar";
</script>

<p class="lead">
  Hold a key, talk, and what you said is typed into whatever app you were already using.
</p>

<p class="body">
  Wispr Lightning runs in {home}. It has no window of its own — this one is its settings.
</p>

<p class="body">
  The next three steps take about a minute: the permissions it needs, the key you hold, and who
  transcribes your voice. You can skip them and change everything later in Settings.
</p>

<style>
  .lead {
    margin: 0;
    font-size: var(--text-title);
    font-weight: var(--weight-medium);
    color: var(--text-primary);
  }

  .body {
    margin: 0;
    color: var(--text-secondary);
  }
</style>
