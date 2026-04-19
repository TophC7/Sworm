<script lang="ts">
  import { getSessions } from '$lib/stores/sessions.svelte'
  import { getGitSummary } from '$lib/stores/git.svelte'
  import { getActiveProjectId } from '$lib/stores/workspace.svelte'
  import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import NixEnvIndicator from '$lib/components/NixEnvIndicator.svelte'
  import NotificationsButton from '$lib/components/notifications/NotificationsButton.svelte'
  import { Circle, AlertTriangle, GitBranchIcon, ArrowUp, ArrowDown, Minus, Plus } from '$lib/icons/lucideExports'

  let sessions = $derived(getSessions())
  let liveSessions = $derived(sessions.filter((s) => s.status === 'running'))
  let zoom = $derived(getZoomLevel())
  let activeProjectId = $derived(getActiveProjectId())
  let gitSummary = $derived(activeProjectId ? getGitSummary(activeProjectId) : null)
</script>

<footer
  class="flex min-h-6 shrink-0 items-center justify-between gap-3 border-t border-edge bg-surface px-3 py-0.5 text-xs"
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
      <IconButton tooltip="Zoom out" shortcut="Ctrl+-" onclick={zoomOut}>
        <Minus size={10} />
      </IconButton>
      <TooltipRoot>
        <TooltipTrigger
          class="min-w-6 cursor-pointer border-none bg-transparent px-0.5 text-center text-2xs text-muted transition-colors hover:text-fg"
          onclick={zoomReset}
        >
          {Math.round(zoom * 100)}%
        </TooltipTrigger>
        <TooltipContent>Reset zoom <kbd class="ml-2 font-mono text-xs text-subtle">Ctrl+0</kbd></TooltipContent>
      </TooltipRoot>
      <IconButton tooltip="Zoom in" shortcut="Ctrl+=" onclick={zoomIn}>
        <Plus size={10} />
      </IconButton>
    </div>

    <NotificationsButton />
  </div>
</footer>
