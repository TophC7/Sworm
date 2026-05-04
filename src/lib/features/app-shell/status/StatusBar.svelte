<script lang="ts">
  import { getSessions } from '$lib/features/sessions/state/sessions.svelte'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import { getActiveProjectId } from '$lib/features/workbench/state.svelte'
  import { getProjectById } from '$lib/features/projects/state.svelte'
  import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import NixEnvIndicator from '$lib/features/app-shell/status/NixEnvIndicator.svelte'
  import NotificationsButton from '$lib/features/notifications/NotificationsButton.svelte'
  import StatusBarBranchPopover from '$lib/features/app-shell/status/StatusBarBranchPopover.svelte'
  import AheadBehindBadge from '$lib/features/git/AheadBehindBadge.svelte'
  import { getEffectiveBindings } from '$lib/features/command-palette/shortcuts/overrides.svelte'
  import { formatShortcut } from '$lib/features/command-palette/shortcuts/spec'
  import { Circle, AlertTriangle, GitBranchIcon, Minus, Plus } from '$lib/icons/lucideExports'

  let activeProjectId = $derived(getActiveProjectId())
  let activeProject = $derived(activeProjectId ? getProjectById(activeProjectId) : null)
  let sessions = $derived(activeProjectId ? getSessions(activeProjectId) : [])
  let liveSessions = $derived(sessions.filter((s) => s.status === 'running'))
  let zoom = $derived(getZoomLevel())
  let gitSummary = $derived(activeProjectId ? getGitSummary(activeProjectId) : null)
  let zoomOutShortcut = $derived(formatShortcut(getEffectiveBindings('zoom-out', ['Ctrl+-'])[0]))
  let zoomResetShortcut = $derived(formatShortcut(getEffectiveBindings('zoom-reset', ['Ctrl+0'])[0]))
  let zoomInShortcut = $derived(formatShortcut(getEffectiveBindings('zoom-in', ['Ctrl+=', 'Ctrl++'])[0]))
</script>

<footer
  class="flex min-h-6 shrink-0 items-center justify-between gap-3 border-t border-edge bg-surface px-3 py-0.5 text-xs"
>
  <div class="flex items-center gap-2.5">
    {#if gitSummary?.branch && activeProjectId && activeProject}
      <StatusBarBranchPopover projectId={activeProjectId} projectPath={activeProject.path}>
        {#snippet children()}
          <span class="flex items-center gap-1 font-mono text-muted">
            <GitBranchIcon size={10} />
            {gitSummary.branch}
            <AheadBehindBadge
              ahead={gitSummary.ahead ?? 0}
              behind={gitSummary.behind ?? 0}
              size="xs"
              twoColor
            />
          </span>
        {/snippet}
      </StatusBarBranchPopover>
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
      <IconButton tooltip="Zoom out" shortcut={zoomOutShortcut} onclick={zoomOut}>
        <Minus size={10} />
      </IconButton>
      <TooltipRoot>
        <TooltipTrigger
          class="min-w-6 cursor-pointer border-none bg-transparent px-0.5 text-center text-2xs text-muted transition-colors hover:text-fg"
          onclick={zoomReset}
        >
          {Math.round(zoom * 100)}%
        </TooltipTrigger>
        <TooltipContent>
          Reset zoom
          {#if zoomResetShortcut}
            <kbd class="ml-2 font-mono text-xs text-subtle">{zoomResetShortcut}</kbd>
          {/if}
        </TooltipContent>
      </TooltipRoot>
      <IconButton tooltip="Zoom in" shortcut={zoomInShortcut} onclick={zoomIn}>
        <Plus size={10} />
      </IconButton>
    </div>

    <NotificationsButton />
  </div>
</footer>
