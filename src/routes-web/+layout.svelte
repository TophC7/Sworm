<script lang="ts">
  import '../app.css'
  import type { Snippet } from 'svelte'

  let { children }: { children: Snippet } = $props()
  const secure = window.isSecureContext && typeof window.crypto?.randomUUID === 'function' && !!window.crypto.subtle
</script>

{#if !secure}
  <div class="flex h-screen flex-col items-center justify-center gap-2 bg-ground p-6 text-center">
    <h1 class="text-3xl text-bright">Secure Context Required</h1>
    <p class="text-base text-muted">Use localhost HTTP or an HTTPS reverse proxy to open Sworm Web.</p>
  </div>
{:else}
  {@render children()}
{/if}
