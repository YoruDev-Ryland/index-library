<script lang="ts">
  // The startup screen: the ground colour of the theme, the mark working in the middle. When the
  // app is ready the screen fades away, the wave still moving under it. Clicking the mark in the
  // title bar plays it again (app.splashReplay): two passes and the rest after them; a click or Esc ends it.
  import MarkAnim, { CYCLE_MS } from "./MarkAnim.svelte";
  import { app } from "../lib/state.svelte";
  let { ready }: { ready: boolean } = $props();

  const FADE_MS = 420; // the fade-out below: .38s after a .04s pause
  let shownAt = performance.now();
  let hold = 600; // a short minimum, so a fast start reads as a moment rather than a flicker
  let leaving = $state(false);
  let gone = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function leave() {
    if (leaving || gone) return;
    leaving = true;
    timer = setTimeout(() => (gone = true), FADE_MS);
  }
  $effect(() => {
    if (!ready || leaving || gone) return;
    const t = setTimeout(leave, Math.max(0, hold - (performance.now() - shownAt)));
    return () => clearTimeout(t);
  });
  // replay from the title bar
  let seen = app.splashReplay;
  $effect(() => {
    if (app.splashReplay === seen) return;
    seen = app.splashReplay;
    clearTimeout(timer);
    shownAt = performance.now();
    // two passes of the wave, then the rest that follows each one: the fade ends just as a third
    // pass would begin
    hold = 2 * CYCLE_MS - FADE_MS;
    leaving = false;
    gone = false;
  });
  const onkey = (e: KeyboardEvent) => { if (e.key === "Escape" && !gone) leave(); };
</script>

<svelte:window onkeydown={onkey} />
{#if !gone}
  <!-- a click is a shortcut; Esc (above) is the keyboard way out -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <div class="splash" class:leaving role="status" aria-label={ready ? "Index" : "Index is starting"} onclick={() => ready && leave()}>
    <MarkAnim size={64} />
  </div>
{/if}

<style>
  /* keys a step stronger than the faint ink, so the mark holds the middle of an empty screen */
  .splash { position: fixed; inset: 0; z-index: 2000; display: grid; place-items: center; background: var(--plastic); --mark-key: color-mix(in srgb, var(--ink) 34%, var(--plastic)); }
  @media (prefers-reduced-motion: no-preference) {
    .splash { transition: opacity .38s cubic-bezier(.2, .8, .2, 1) .04s; animation: arrive .3s cubic-bezier(.2, .8, .2, 1); }
    @keyframes arrive { from { opacity: 0; } }
    .splash :global(.mark-anim) { transition: transform .38s cubic-bezier(.2, .8, .2, 1); }
    .splash.leaving :global(.mark-anim) { transform: scale(.94); }
  }
  .splash.leaving { opacity: 0; pointer-events: none; }
</style>
