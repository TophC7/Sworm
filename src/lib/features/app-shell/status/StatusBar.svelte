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
  import {
    ensureSettingsDiagnosticsListener,
    getSettingsDiagnostics,
    refreshSettingsDiagnostics
  } from '$lib/features/settings/state/diagnostics.svelte'
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
  let settingsDiagnostics = $derived(getSettingsDiagnostics())

  $effect(() => {
    ensureSettingsDiagnosticsListener()
    void refreshSettingsDiagnostics(activeProject?.path)
  })
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
            <AheadBehindBadge ahead={gitSummary.ahead ?? 0} behind={gitSummary.behind ?? 0} size="xs" twoColor />
          </span>
        {/snippet}
      </StatusBarBranchPopover>
    {/if}
    <NixEnvIndicator />
  </div>

  <div class="flex items-center gap-2.5">
    {#if settingsDiagnostics.length > 0}
      <TooltipRoot>
        <TooltipTrigger class="flex items-center gap-1 text-warning transition-colors hover:text-warning-bright">
          <AlertTriangle size={10} />
          {settingsDiagnostics.length} settings
        </TooltipTrigger>
        <TooltipContent class="max-w-md">
          <div class="space-y-1 text-left">
            <div class="text-xs font-medium text-warning-bright">Settings diagnostics</div>
            {#each settingsDiagnostics.slice(0, 5) as diagnostic}
              <div class="font-mono text-2xs text-muted">
                {diagnostic.layer}: {diagnostic.path}{diagnostic.pointer ? ` ${diagnostic.pointer}` : ''}
                <span class="font-sans text-warning-bright">{diagnostic.message}</span>
              </div>
            {/each}
            {#if settingsDiagnostics.length > 5}
              <div class="text-2xs text-subtle">+{settingsDiagnostics.length - 5} more</div>
            {/if}
          </div>
        </TooltipContent>
      </TooltipRoot>
    {/if}

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
