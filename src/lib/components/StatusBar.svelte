<script lang="ts">
  import { getSessions } from '$lib/stores/sessions.svelte'
  import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
  import { Button } from '$lib/components/ui/button'
  import NixEnvIndicator from '$lib/components/NixEnvIndicator.svelte'
  import Circle from '@lucide/svelte/icons/circle'
  import AlertTriangle from '@lucide/svelte/icons/alert-triangle'
  import Minus from '@lucide/svelte/icons/minus'
  import Plus from '@lucide/svelte/icons/plus'

  let sessions = $derived(getSessions())
  let liveSessions = $derived(sessions.filter((s) => s.status === 'running'))
  let zoom = $derived(getZoomLevel())
</script>

<footer
  class="flex min-h-6 shrink-0 items-center justify-between gap-3 border-t border-edge bg-surface px-3 py-0.5 text-[0.68rem]"
>
  <div class="flex items-center gap-2.5">
    <NixEnvIndicator />
  </div>

  <div class="flex items-center gap-2.5">
    {#if liveSessions.length > 0}
      <span class="flex items-center gap-1 text-success">
        <Circle size={6} fill="currentColor" />
        {liveSessions.length} live
      </span>
    {/if}
    {#if liveSessions.length > 1}
      <span class="flex items-center gap-1 text-warning" title="Sessions share the same working tree">
        <AlertTriangle size={10} /> shared
      </span>
    {/if}

    <div class="flex items-center gap-0.5 text-muted">
      <Button variant="ghost" size="icon-sm" onclick={zoomOut} title="Zoom out">
        <Minus size={10} />
      </Button>
      <button
        class="min-w-6 cursor-pointer border-none bg-transparent px-0.5 text-center text-[0.65rem] text-muted transition-colors hover:text-fg"
        onclick={zoomReset}
        title="Reset zoom">{Math.round(zoom * 100)}%</button
      >
      <Button variant="ghost" size="icon-sm" onclick={zoomIn} title="Zoom in">
        <Plus size={10} />
      </Button>
    </div>
  </div>
</footer>
