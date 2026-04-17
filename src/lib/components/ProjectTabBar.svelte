<script lang="ts">
  import { getProjectById } from '$lib/stores/projects.svelte'
  import {
    getOpenProjectIds,
    getActiveProjectId,
    selectProject,
    closeProject,
    reorderProjects
  } from '$lib/stores/workspace.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { Plus } from '$lib/icons/lucideExports'
  import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs'

  let { onAddProject: _ }: { onAddProject: () => void } = $props()

  let openIds = $derived(getOpenProjectIds())
  let activeId = $derived(getActiveProjectId())

  let confirmClose = $state<string | null>(null)
  let confirmProject = $derived(confirmClose ? getProjectById(confirmClose) : null)

  // Drag-to-reorder state
  let dragIndex = $state<number | null>(null)

  function handleSelect(id: string) {
    selectProject(id)
  }

  function handleClose(e: Event, id: string) {
    e.stopPropagation()
    confirmClose = id
  }

  function handleAuxClick(e: MouseEvent, id: string) {
    if (e.button !== 1) return
    handleClose(e, id)
  }

  function doClose() {
    if (confirmClose) {
      void closeProject(confirmClose)
      confirmClose = null
    }
  }

  // Pointer-based drag reorder
  function handleDragStart(e: DragEvent, index: number) {
    dragIndex = index
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move'
      e.dataTransfer.setData('text/plain', String(index))
    }
  }

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault()
    if (dragIndex !== null && dragIndex !== index) {
      reorderProjects(dragIndex, index)
      dragIndex = index
    }
  }

  function handleDragEnd() {
    dragIndex = null
  }
</script>

<TabStrip variant="project" ariaLabel="Open projects">
  {#each openIds as id, index (id)}
    {@const project = getProjectById(id)}
    {#if project}
      <TabButton
        variant="project"
        active={activeId === id}
        onclick={() => handleSelect(id)}
        onauxclick={(e) => handleAuxClick(e, id)}
        onClose={(e) => handleClose(e, id)}
        draggable="true"
        ondragstart={(e) => handleDragStart(e, index)}
        ondragover={(e) => handleDragOver(e, index)}
        ondragend={handleDragEnd}
      >
        <span class="max-w-[140px] truncate">{project.name}</span>
      </TabButton>
    {/if}
  {/each}

  {#snippet trailing()}
    <IconButton
      tooltip="Open project"
      tooltipSide="bottom"
      class="ml-0.5 flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center border-none bg-transparent text-sm text-muted transition-colors hover:text-bright"
      onclick={() => selectProject(null)}
    >
      <Plus size={14} />
    </IconButton>
  {/snippet}
</TabStrip>

<ConfirmDialog
  open={confirmClose !== null}
  title="Close Project"
  message="Close {confirmProject?.name ?? 'this project'}? Running sessions will be stopped."
  confirmLabel="Close"
  onCancel={() => (confirmClose = null)}
  onConfirm={doClose}
/>
