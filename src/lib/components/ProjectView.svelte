<script lang="ts">
  import type { Project } from '$lib/types/backend'
  import { loadSessions } from '$lib/stores/sessions.svelte'
  import { startGitPolling, stopGitPolling, getGitSummary, refreshGit } from '$lib/stores/git.svelte'
  import ActivityBar from '$lib/components/ActivityBar.svelte'
  import GitSidebar from '$lib/components/git/GitSidebar.svelte'
  import FilesSidebar from '$lib/components/files/FilesSidebar.svelte'
  import SessionHistoryView from '$lib/components/session/SessionHistoryView.svelte'
  import { getSidebarWidth, setSidebarWidth, isSidebarCollapsed, getSidebarView } from '$lib/stores/ui.svelte'
  import PaneGrid from '$lib/components/PaneGrid.svelte'
  import {
    addCommitTab,
    addChangesTab,
    addStashTab,
    getFocusedTab,
    promoteTemporaryTab
  } from '$lib/stores/workspace.svelte'

  let {
    project
  }: {
    project: Project
  } = $props()

  let gitSummary = $derived(getGitSummary(project.id))
  let sidebarCollapsed = $derived(isSidebarCollapsed())
  let sidebarWidth = $derived(getSidebarWidth())
  let sidebarView = $derived(getSidebarView())
  let layoutEl = $state<HTMLDivElement | null>(null)

  // Resize handle for git sidebar (left side)
  function sidebarResizeHandle(element: HTMLElement) {
    function onPointerDown(e: PointerEvent) {
      e.preventDefault()
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'

      function onMove(e: PointerEvent) {
        if (!layoutEl) return
        const w = e.clientX - layoutEl.getBoundingClientRect().left
        setSidebarWidth(w)
      }
      function onUp() {
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
      }
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    }
    element.addEventListener('pointerdown', onPointerDown)
    return () => element.removeEventListener('pointerdown', onPointerDown)
  }

  // Load sessions and start git polling when project changes
  $effect(() => {
    const id = project.id
    const path = project.path

    loadSessions(id)
    startGitPolling(id, path)

    return () => {
      stopGitPolling(id)
    }
  })

  function handleRefreshGit() {
    void refreshGit(project.id, project.path)
  }
</script>

<!-- Horizontal layout: activity-bar | sidebar-content | resize | panes -->
<div class="flex min-h-0 flex-1 overflow-hidden">
  <!-- Activity bar: always visible -->
  <ActivityBar />

  <!-- Sidebar content + pane grid (resize handle relative to this container) -->
  <div class="flex min-h-0 flex-1 overflow-hidden" bind:this={layoutEl}>
    <!-- Sidebar panel (collapsible) -->
    {#if !sidebarCollapsed}
      <div class="shrink-0 overflow-hidden" style="width: {sidebarWidth}px;">
        {#if sidebarView === 'git'}
          <GitSidebar
            summary={gitSummary}
            projectId={project.id}
            projectPath={project.path}
            onRefresh={handleRefreshGit}
            onFileClick={(filePath, staged) => addChangesTab(project.id, staged, filePath)}
            onPersistTab={() => {
              const tab = getFocusedTab(project.id)
              if (tab && tab.kind !== 'session' && tab.temporary) promoteTemporaryTab(tab.id)
            }}
            onCommitFileClick={(hash, shortHash, message, filePath) =>
              addCommitTab(project.id, hash, shortHash, message, filePath)}
            onStashFileClick={(stashIndex, message, filePath) => addStashTab(project.id, stashIndex, message, filePath)}
            onViewAllChanges={(staged) => addChangesTab(project.id, staged, null, false)}
          />
        {:else if sidebarView === 'sessions'}
          <SessionHistoryView projectId={project.id} />
        {:else if sidebarView === 'files'}
          <FilesSidebar projectId={project.id} projectPath={project.path} />
        {/if}
      </div>

      <!-- Sidebar resize handle -->
      <div
        class="w-px shrink-0 cursor-col-resize bg-edge transition-colors hover:bg-accent/40"
        {@attach sidebarResizeHandle}
        role="separator"
        aria-label="Resize sidebar"
      ></div>
    {/if}

    <!-- Right column: session tabs + content -->
    <div class="flex min-w-0 flex-1 flex-col overflow-hidden">
      <div class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <PaneGrid projectId={project.id} projectPath={project.path} />
      </div>
    </div>
  </div>
</div>
