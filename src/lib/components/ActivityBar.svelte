<script lang="ts">
  import {
    getSidebarView,
    setSidebarView,
    isSidebarCollapsed,
    setSidebarCollapsed,
    type SidebarView
  } from '$lib/stores/ui.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { GitBranchIcon, BotMessageSquare, FolderOpen } from '$lib/icons/lucideExports'

  let activeView = $derived(getSidebarView())
  let collapsed = $derived(isSidebarCollapsed())

  function handleClick(view: SidebarView) {
    if (activeView === view) {
      // Toggle collapse when clicking the already-active view.
      setSidebarCollapsed(!collapsed)
    } else {
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
    <div class="relative">
      {#if isActive}
        <span class="absolute top-1 bottom-1 left-0 w-0.5 rounded-r bg-accent" aria-hidden="true"></span>
      {/if}
      <IconButton
        size="md"
        tooltip={view.label}
        tooltipSide="right"
        active={isActive}
        onclick={() => handleClick(view.id)}
      >
        <view.icon size={16} />
      </IconButton>
    </div>
  {/each}
</div>
