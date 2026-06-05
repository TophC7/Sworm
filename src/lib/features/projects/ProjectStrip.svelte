<!--
  @component
  ProjectStrip. Compact vertical tab strip for switching between open
  projects, pinned above the sidebar rail and panel. Rows behave like
  browser-style vertical tabs: click to switch, middle-click to close.
  Sized to content up to a 25% cap; the bottom edge drags to resize
  (double-click resets). When the sidebar is collapsed the strip
  narrows to initial tiles.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { cn } from '$lib/utils/cn'
  import { SidebarRow } from '$lib/components/ui/sidebar-row'
  import { ResizeDivider } from '$lib/components/ui/resize-divider'
  import { getProjects } from '$lib/features/projects/state.svelte'
  import {
    getOpenProjectIds,
    getActiveProjectId,
    selectProject,
    closeProject,
    reorderProjects
  } from '$lib/features/workbench/state.svelte'
  import { getAgentSessions, loadSessionGroupsForDisplay } from '$lib/features/sessions/state/sessions.svelte'
  import { sessionDotClass } from '$lib/features/sessions/visual'
  import { isSidebarCollapsed } from '$lib/features/app-shell/sidebar/state.svelte'
  import type { Project } from '$lib/types/backend'

  let projects = $derived(getProjects())
  let openIds = $derived(getOpenProjectIds())
  let activeProjectId = $derived(getActiveProjectId())
  let collapsed = $derived(isSidebarCollapsed())

  let stripEl = $state<HTMLDivElement | null>(null)
  // User-dragged height cap; null means auto (content-sized up to 25%).
  let userHeight = $state<number | null>(null)

  // Open projects in workbench tab order, skipping ids the project
  // list hasn't loaded yet.
  let openProjects = $derived(
    openIds.map((id) => projects.find((p) => p.id === id)).filter((p): p is Project => p !== undefined)
  )

  // Warm the session display cache so each row's status dot resolves
  // without per-project fanout. untrack keeps the cache writes from
  // retriggering this effect.
  $effect(() => {
    const ids = openIds
    untrack(() => {
      void loadSessionGroupsForDisplay(ids)
    })
  })

  function dotClass(projectId: string): string {
    const latest = getAgentSessions(projectId)[0] ?? null
    return latest ? sessionDotClass(latest) : 'bg-muted'
  }

  // DRAG REORDER //
  // Strip-local HTML5 drag with a private mime type so pane/terminal
  // drop targets ignore it. Drops land in workbench reorderProjects.
  let dragIndex = $state<number | null>(null)
  // Raw insertion slot (0..n); null when no reorder drag is active.
  let dropIndex = $state<number | null>(null)

  function handleDragStart(e: DragEvent, index: number) {
    if (!e.dataTransfer) return
    dragIndex = index
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('application/vnd.sworm.project-reorder', openProjects[index].id)
  }

  function handleDragOver(e: DragEvent, index: number) {
    if (dragIndex === null) return
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    dropIndex = e.clientY < rect.top + rect.height / 2 ? index : index + 1
  }

  function handleDrop(e: DragEvent) {
    if (dragIndex === null || dropIndex === null) return
    e.preventDefault()
    // Insertion slot to post-removal index for reorderProjects.
    const to = dropIndex > dragIndex ? dropIndex - 1 : dropIndex
    if (to !== dragIndex) reorderProjects(dragIndex, to)
    handleDragEnd()
  }

  function handleDragEnd() {
    dragIndex = null
    dropIndex = null
  }

  // Strip geometry cached at drag start; top edge and parent height
  // are stable while the strip resizes.
  let stripTop = 0
  let stripMaxHeight = Infinity

  function handleResizeStart() {
    stripTop = stripEl?.getBoundingClientRect().top ?? 0
    stripMaxHeight = (stripEl?.parentElement?.getBoundingClientRect().height ?? Infinity) * 0.8
  }

  function handleResize(e: PointerEvent) {
    userHeight = Math.max(28, Math.min(e.clientY - stripTop, stripMaxHeight))
  }
</script>

<div
  class={cn('flex shrink-0 flex-col overflow-hidden bg-ground', collapsed && 'border-r border-edge')}
  style:max-height={userHeight !== null ? `${userHeight}px` : '25%'}
  bind:this={stripEl}
>
  <div class="scrollbar-none min-h-0 flex-1 overflow-y-auto">
    {#each openProjects as project, i (project.id)}
      {@const active = project.id === activeProjectId}
      <SidebarRow
        variant="section"
        pressed={active}
        divider={false}
        class={cn(
          collapsed && 'justify-center px-0',
          dragIndex === i && 'opacity-50',
          dropIndex === i && 'shadow-[inset_0_1px_0_var(--color-accent)]',
          dropIndex === i + 1 && i === openProjects.length - 1 && 'shadow-[inset_0_-1px_0_var(--color-accent)]'
        )}
        draggable="true"
        onclick={() => selectProject(project.id)}
        onauxclick={(e: MouseEvent) => e.button === 1 && void closeProject(project.id)}
        ondragstart={(e: DragEvent) => handleDragStart(e, i)}
        ondragover={(e: DragEvent) => handleDragOver(e, i)}
        ondrop={handleDrop}
        ondragend={handleDragEnd}
      >
        <span
          class={cn(
            'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm font-mono text-2xs uppercase',
            active ? 'bg-accent text-ground' : 'bg-raised text-muted'
          )}
        >
          {project.name.charAt(0)}
        </span>
        {#if !collapsed}
          <span class="min-w-0 flex-1 truncate text-fg">{project.name}</span>
          <span class="h-1.5 w-1.5 shrink-0 rounded-full {dotClass(project.id)}"></span>
        {/if}
      </SidebarRow>
    {/each}
  </div>

  <!-- Strip resize handle; double-click resets to the auto 25% cap. -->
  <ResizeDivider
    direction="row"
    onResizeStart={handleResizeStart}
    onResize={handleResize}
    ondblclick={() => (userHeight = null)}
    aria-label="Resize project strip"
  />
</div>
