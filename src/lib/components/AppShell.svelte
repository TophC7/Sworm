<script lang="ts">
  import { backend } from '$lib/api/backend'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import EmptyState from '$lib/components/EmptyState.svelte'
  import ProjectView from '$lib/components/ProjectView.svelte'
  import SettingsDialog from '$lib/components/SettingsDialog.svelte'
  import StatusBar from '$lib/components/StatusBar.svelte'
  import TitleBar from '$lib/components/TitleBar.svelte'
  import { addProject, getActiveProject } from '$lib/stores/projects.svelte'
  import { getWindowControls, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
  import { openProject } from '$lib/stores/workspace.svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onMount } from 'svelte'

  let activeProject = $derived(getActiveProject())

  let settingsOpen = $state(false)
  let projectError = $state<string | null>(null)

  // Restore system decorations if user previously chose that (requires restart to toggle)
  onMount(() => {
    const wc = getWindowControls()
    if (wc.useSystemDecorations) {
      getCurrentWindow().setDecorations(true)
    }
  })

  // Global keyboard shortcuts
  $effect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (!e.ctrlKey && !e.metaKey) return
      // Don't steal shortcuts from the terminal
      if ((e.target as HTMLElement).closest?.('.xterm')) return

      switch (e.key) {
        case '=':
        case '+':
          e.preventDefault()
          zoomIn()
          break
        case '-':
          e.preventDefault()
          zoomOut()
          break
        case '0':
          e.preventDefault()
          zoomReset()
          break
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })

  async function handleNewProject() {
    try {
      const path = await backend.projects.selectDirectory()
      if (path) {
        const project = await addProject(path)
        openProject(project.id)
      }
    } catch (e) {
      projectError = `Failed to add project:\n${e}`
    }
  }
</script>

<div class="flex h-screen flex-col overflow-hidden">
  <TitleBar onNewProject={handleNewProject} onSettings={() => (settingsOpen = true)} />

  <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
    {#if activeProject}
      <ProjectView project={activeProject} />
    {:else}
      <EmptyState />
    {/if}
  </main>

  <StatusBar />
</div>

<SettingsDialog open={settingsOpen} onClose={() => (settingsOpen = false)} />

{#if projectError}
  <ConfirmDialog
    open={true}
    title="Project Error"
    message={projectError}
    confirmLabel="Close"
    showCancel={false}
    onCancel={() => (projectError = null)}
    onConfirm={() => (projectError = null)}
  />
{/if}
