<!--
  @component
  WorkbenchView — sidebar rail + folder-scoped sidebar panel + the active
  tab's surface. Sidebar chrome (view, width, collapsed) is global; its
  content follows the active tab's folder.
-->

<script lang="ts">
  import { onMount } from 'svelte'
  import { disposeTauriOsDrop, initTauriOsDrop } from '$lib/features/dnd'
  import { dragObserver } from '$lib/features/dnd/observer.svelte'
  import { DND_MIME } from '$lib/features/dnd/payload'
  import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
  import { ensureGitListeners, ensureGitWatch, getGitSummary } from '$lib/features/git/state.svelte'
  import SidebarRail from '$lib/features/app-shell/sidebar/SidebarRail.svelte'
  import EmptyState from '$lib/features/app-shell/EmptyState.svelte'
  import GitSidebar from '$lib/features/git/GitSidebar.svelte'
  import FilesSidebar from '$lib/features/files/FilesSidebar.svelte'
  import IssuesSidebar from '$lib/features/issues/IssuesSidebar.svelte'
  import { ResizeDivider } from '$lib/components/ui/resize-divider'
  import {
    getSidebarWidth,
    setSidebarWidth,
    isSidebarCollapsed,
    getSidebarView
  } from '$lib/features/app-shell/sidebar/state.svelte'
  import SurfaceHost from '$lib/features/workbench/SurfaceHost.svelte'
  import {
    openCommitDiff,
    openStashDiff,
    openWorkingTreeDiff
  } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { dropForeignTab } from '$lib/features/workbench/transferService.svelte'
  import { getActiveTab, getTabs, promoteTabWhenReady } from '$lib/features/workbench/state.svelte'

  let activeTab = $derived(getActiveTab())
  let folderPath = $derived(activeTab?.folderPath ?? null)

  let gitSummary = $derived(folderPath ? getGitSummary(folderPath) : null)
  // Unique changed-path count for the sidebar rail badge. `changes` lists a
  // file twice when it has both staged and unstaged edits, so count by path.
  let gitChangeCount = $derived(gitSummary ? new Set(gitSummary.changes.map((c) => c.path)).size : 0)
  let sidebarCollapsed = $derived(isSidebarCollapsed())
  let sidebarWidth = $derived(getSidebarWidth())
  let sidebarView = $derived(getSidebarView())
  let sidebarPanelEl = $state<HTMLDivElement | null>(null)
  let tabDropActive = $state(false)
  const foreignTabDropObserver = dragObserver({
    accept: (_payload, types) => !LocalTransfer.has('tab') && types.includes(DND_MIME.SWORM_TAB),
    onEnter: () => (tabDropActive = true),
    onOver: () => (tabDropActive = true),
    onLeave: () => (tabDropActive = false),
    onDrop: (event) => {
      tabDropActive = false
      dropForeignTab(event, getTabs().length)
    },
    dropEffect: 'move'
  })
  // Panel left edge cached at drag start; it stays fixed while the
  // width changes, so per-move layout reads are unnecessary.
  let panelLeft = 0

  $effect(() => {
    const path = folderPath
    if (!path) return
    ensureGitListeners()
    void ensureGitWatch(path)
  })

  onMount(() => {
    void initTauriOsDrop()
    return () => disposeTauriOsDrop()
  })
</script>

<div class="flex min-h-0 flex-1 overflow-hidden">
  {#if folderPath}
    <SidebarRail {gitChangeCount} />
  {/if}

  {#if !sidebarCollapsed && folderPath}
    {#key folderPath}
      <div class="shrink-0 overflow-hidden" style="width: {sidebarWidth}px;" bind:this={sidebarPanelEl}>
        {#if sidebarView === 'git'}
          <GitSidebar
            summary={gitSummary}
            {folderPath}
            onFileClick={(filePath, staged) => openWorkingTreeDiff(folderPath, staged, null, filePath)}
            onPersistTab={promoteTabWhenReady}
            onCommitFileClick={(hash, shortHash, message, filePath) =>
              openCommitDiff(folderPath, hash, shortHash, message, filePath)}
            onStashFileClick={(stashIndex, message, filePath) =>
              openStashDiff(folderPath, stashIndex, message, filePath)}
            onViewAllChanges={(staged) => openWorkingTreeDiff(folderPath, staged, null, null, { temporary: false })}
          />
        {:else if sidebarView === 'issues'}
          <IssuesSidebar {folderPath} />
        {:else if sidebarView === 'files'}
          <FilesSidebar {folderPath} />
        {/if}
      </div>
    {/key}

    <!-- Sidebar resize handle: full height beside the left column. -->
    <ResizeDivider
      direction="col"
      onResizeStart={() => (panelLeft = sidebarPanelEl?.getBoundingClientRect().left ?? 0)}
      onResize={(e) => setSidebarWidth(e.clientX - panelLeft)}
      aria-label="Resize sidebar"
    />
  {/if}

  <div
    class="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
    role="region"
    aria-label="Workbench surface"
    {@attach foreignTabDropObserver}
  >
    {#if activeTab}
      <SurfaceHost {activeTab} />
    {:else}
      <EmptyState />
    {/if}
    {#if tabDropActive}
      <div
        class="pointer-events-none absolute inset-0 z-20 border-2 border-accent bg-accent/10"
        role="presentation"
      ></div>
    {/if}
  </div>
</div>
