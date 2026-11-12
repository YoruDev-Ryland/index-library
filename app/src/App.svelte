<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/state.svelte";
  import { api } from "./lib/api";
  import type { StartupInfo } from "./lib/types";
  import Sidebar from "./components/Sidebar.svelte";
  import JobsBar from "./components/JobsBar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import TitleBar from "./components/TitleBar.svelte";
  import Splash from "./components/Splash.svelte";
  import FirstRun from "./components/FirstRun.svelte";
  import ArchiveNotice from "./components/ArchiveNotice.svelte";
  import Library from "./routes/Library.svelte";
  import Dashboard from "./routes/Dashboard.svelte";
  import Duplicates from "./routes/Duplicates.svelte";
  import Cleanup from "./routes/Cleanup.svelte";
  import Archive from "./routes/Archive.svelte";
  import Review from "./routes/Review.svelte";
  import History from "./routes/History.svelte";
  import SettingsView from "./routes/Settings.svelte";
  import CompletionView from "./routes/Completion.svelte";
  import Collection from "./routes/Collection.svelte";

  let startup = $state<StartupInfo | null>(null);
  let ready = $state(false);
  let showNotice = $state(true);

  async function boot() {
    try {
      startup = await api.startup();
      if (!startup.needs_setup && app.systems.length === 0) await api.scan(false);
    } catch (e) { app.fail(e); }
  }

  onMount(async () => {
    try { await app.init(); } catch (e) { app.fail(e); }
    ready = true;
    await boot();
  });
</script>

<div class="window">
<TitleBar />
<div class="content">
{#if ready}
  {#if startup?.needs_setup}
    <FirstRun ondone={() => boot()} />
  {:else}
    <div class="shell">
      <Sidebar />
      <main>
        {#if app.view === "library"}<Library />
        {:else if app.view === "dashboard"}<Dashboard />
        {:else if app.view === "completion"}<CompletionView />
        {:else if app.view === "duplicates"}<Duplicates />
        {:else if app.view === "cleanup"}<Cleanup />
        {:else if app.view === "archive"}<Archive />
        {:else if app.view === "review"}<Review />
        {:else if app.view === "history"}<History />
        {:else if app.view === "settings"}<SettingsView />
        {:else if app.view === "collection"}<Collection />
        {/if}
      </main>
      <JobsBar />
    </div>
    {#if startup && startup.expired.length > 0 && showNotice}
      <ArchiveNotice entries={startup.expired} bytes={startup.expired_bytes} onclose={() => (showNotice = false)} />
    {/if}
  {/if}
{/if}
</div>
</div>
<Splash {ready} />
<Toasts />

<style>
  .window { display: flex; flex-direction: column; height: 100%; }
  .content { flex: 1; min-height: 0; }
  .shell { display: grid; grid-template-columns: 248px 1fr; grid-template-rows: minmax(0, 1fr) auto auto; height: 100%; }
  main { min-width: 0; min-height: 0; overflow: hidden; }
  .shell > :global(.side) { grid-row: 1 / 3; }
  .shell > :global(.activity) { grid-column: 2; grid-row: 2; }
  .shell > :global(.jobsbar) { grid-column: 1 / -1; grid-row: 3; }
</style>
