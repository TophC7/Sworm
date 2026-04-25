<script lang="ts">
  import {
    getSidebarView,
    setSidebarView,
    isSidebarCollapsed,
    setSidebarCollapsed,
    type SidebarView
  } from '$lib/features/app-shell/sidebar/state.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { GitBranchIcon, BotMessageSquare, FolderOpen } from '$lib/icons/lucideExports'

  let { gitChangeCount = 0 }: { gitChangeCount?: number } = $props()

  let activeView = $derived(getSidebarView())
  let collapsed = $derived(isSidebarCollapsed())

  // Shown on the git icon when the working tree has changes. Over 99 we
  // swap to ":D" — both as a playful signal that you have bigger problems
  // than an exact count, and to keep the badge width bounded.
  let gitBadgeLabel = $derived(gitChangeCount > 99 ? ':D' : String(gitChangeCount))

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
      {#if view.id === 'git' && gitChangeCount > 0}
        <span
          class="pointer-events-none absolute -top-0.5 -right-0.5 inline-flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-accent px-1 font-mono text-2xs leading-none font-semibold text-ground ring-1 ring-ground"
          aria-label="{gitChangeCount} changed files"
        >
          {gitBadgeLabel}
        </span>
      {/if}
    </div>
  {/each}
</div>
