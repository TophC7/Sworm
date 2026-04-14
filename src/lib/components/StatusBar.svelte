<script lang="ts">
  import { getSessions } from '$lib/stores/sessions.svelte'
  import { getGitSummary } from '$lib/stores/git.svelte'
  import { getActiveProjectId } from '$lib/stores/workspace.svelte'
  import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
  import { Button } from '$lib/components/ui/button'
  import NixEnvIndicator from '$lib/components/NixEnvIndicator.svelte'
  import { Circle, AlertTriangle, GitBranchIcon, ArrowUp, ArrowDown, Minus, Plus } from '$lib/icons/lucideExports'

  let sessions = $derived(getSessions())
  let liveSessions = $derived(sessions.filter((s) => s.status === 'running'))
  let zoom = $derived(getZoomLevel())
  let activeProjectId = $derived(getActiveProjectId())
  let gitSummary = $derived(activeProjectId ? getGitSummary(activeProjectId) : null)
</script>

<footer
  class="flex min-h-6 shrink-0 items-center justify-between gap-3 border-t border-edge bg-surface px-3 py-0.5 text-[0.68rem]"
>
  <div class="flex items-center gap-2.5">
    {#if gitSummary?.branch}
      <span class="flex items-center gap-1 font-mono text-muted">
        <GitBranchIcon size={10} />
        {gitSummary.branch}
      </span>
      {#if (gitSummary.ahead ?? 0) > 0}
        <span class="flex items-center gap-0.5 text-success">
          <ArrowUp size={9} />{gitSummary.ahead}
        </span>
      {/if}
      {#if (gitSummary.behind ?? 0) > 0}
        <span class="flex items-center gap-0.5 text-danger">
          <ArrowDown size={9} />{gitSummary.behind}
        </span>
      {/if}
    {/if}
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
