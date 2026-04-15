<script lang="ts">
  import '../app.css'
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { disposeAll } from '$lib/terminal/sessionRegistry'
  import { backend } from '$lib/api/backend'
  import CommandCenter from '$lib/components/CommandCenter.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import SettingsDialog from '$lib/components/SettingsDialog.svelte'
  import StatusBar from '$lib/components/StatusBar.svelte'
  import TitleBar from '$lib/components/TitleBar.svelte'
  import { TooltipProvider } from '$lib/components/ui/tooltip'
  import { addProject } from '$lib/stores/projects.svelte'
  import { getWindowControls } from '$lib/stores/ui.svelte'
  import { setupGlobalShortcuts } from '$lib/utils/shortcuts.svelte'
  import { openProject } from '$lib/stores/workspace.svelte'
  import type { Snippet } from 'svelte'

  let { children }: { children: Snippet } = $props()

  let settingsOpen = $state(false)
  let projectError = $state<string | null>(null)

  onMount(() => {
    const appWindow = getCurrentWindow()

    const unlisten = appWindow.onCloseRequested(() => {
      disposeAll()
    })

    // Restore system decorations if user previously chose that
    const wc = getWindowControls()
    if (wc.useSystemDecorations) {
      appWindow.setDecorations(true)
    }

    const cleanupShortcuts = setupGlobalShortcuts()

    return () => {
      cleanupShortcuts()
      unlisten.then((cleanup) => cleanup()).catch(() => {})
    }
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

<TooltipProvider delayDuration={300}>
  <div class="flex h-screen flex-col overflow-hidden">
    <TitleBar onNewProject={handleNewProject} onSettings={() => (settingsOpen = true)} />

    <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
      {@render children()}
    </main>

    <StatusBar />
  </div>
</TooltipProvider>

<CommandCenter onNewProject={handleNewProject} onSettings={() => (settingsOpen = true)} />
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
