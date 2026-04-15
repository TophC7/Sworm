<script lang="ts">
  import {
    getSidebarView,
    setSidebarView,
    isSidebarCollapsed,
    setSidebarCollapsed,
    type SidebarView
  } from '$lib/stores/ui.svelte'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import { GitBranchIcon, BotMessageSquare, FolderOpen } from '$lib/icons/lucideExports'
  import { cn } from '$lib/utils/cn'

  let activeView = $derived(getSidebarView())
  let collapsed = $derived(isSidebarCollapsed())

  function handleClick(view: SidebarView) {
    if (activeView === view) {
      // Toggle collapse when clicking the already-active view
      setSidebarCollapsed(!collapsed)
    } else {
      // Switch view; expand if collapsed
      setSidebarView(view)
      if (collapsed) setSidebarCollapsed(false)
    }
  }

  const views: { id: SidebarView; icon: typeof GitBranchIcon; label: string }[] = [
    { id: 'git', icon: GitBranchIcon, label: 'Git' },
    { id: 'files', icon: FolderOpen, label: 'Files' },
    { id: 'sessions', icon: BotMessageSquare, label: 'Sessions' }
  ]
</script>

<div class="flex w-9 shrink-0 flex-col items-center gap-0.5 border-r border-edge bg-ground py-2">
  {#each views as view}
    {@const isActive = activeView === view.id && !collapsed}
    <TooltipRoot>
      <TooltipTrigger
        class={cn(
          'relative flex h-7 w-7 cursor-pointer items-center justify-center rounded-sm transition-colors',
          isActive ? 'bg-surface text-bright' : 'text-muted hover:bg-surface/50 hover:text-subtle'
        )}
        aria-label={view.label}
        onclick={() => handleClick(view.id)}
      >
        {#if isActive}
          <span class="absolute top-1 bottom-1 left-0 w-0.5 rounded-r bg-accent"></span>
        {/if}
        <view.icon size={16} />
      </TooltipTrigger>
      <TooltipContent side="right">{view.label}</TooltipContent>
    </TooltipRoot>
  {/each}
</div>
