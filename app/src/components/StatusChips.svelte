<script lang="ts">
  import type { Status } from "../lib/types";
  let { status, dat = null, compact = false }: { status: Status; dat?: string | null; compact?: boolean } = $props();
</script>
{#if status.decrypted === "true"}<span class="chip ok" title="Decrypted">{compact ? "Dec" : "Decrypted"}</span>
{:else if status.decrypted === "false"}<span class="chip warn" title="Still encrypted — needs a decrypt action">{compact ? "Enc" : "Encrypted"}</span>{/if}
{#if status.compressed === "true"}<span class="chip info">{compact ? "Cmp" : "Compressed"}</span>{/if}
{#if dat === "verified"}<span class="chip ok" title="Matches a No-Intro/Redump DAT">Verified</span>
{:else if dat === "bad"}<span class="chip bad" title="Name matches a DAT entry but the content differs">Bad dump</span>{/if}
