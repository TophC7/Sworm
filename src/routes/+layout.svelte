<script lang="ts">
  import '../app.css'
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
  import { disposeAll } from '$lib/features/sessions/terminal/sessionRegistry'
  import { backend } from '$lib/api/backend'
  import CommandCenter from '$lib/features/command-palette/CommandCenter.svelte'
  import ConfirmDialog from '$lib/components/dialogs/ConfirmDialog.svelte'
  import ConfirmHost from '$lib/features/confirm/ConfirmHost.svelte'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import NotificationsSurface from '$lib/features/notifications/NotificationsSurface.svelte'
  import SettingsDialog from '$lib/features/settings/dialog/SettingsDialog.svelte'
  import StatusBar from '$lib/features/app-shell/status/StatusBar.svelte'
  import TitleBar from '$lib/features/app-shell/titlebar/TitleBar.svelte'
  import { TooltipProvider } from '$lib/components/ui/tooltip'
  import { addProject } from '$lib/features/projects/state.svelte'
  import { getWindowControls } from '$lib/features/app-shell/window-controls/state.svelte'
  import { isSettingsOpen, setSettingsOpen } from '$lib/features/settings/dialog/state.svelte'
  import { isAnyModalOpen } from '$lib/utils/modalRegistry.svelte'
  import { setupGlobalShortcuts } from '$lib/features/command-palette/shortcuts/setup.svelte'
  import { initProjectSchemas } from '$lib/features/project-config/bootstrap'
  import {
    getDirtyTextSurfaceCount,
    hasAnyDirtyTextSurfaces
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { flushPersistencePending, getActiveSessionId, openProject } from '$lib/features/workbench/state.svelte'
  import type { Snippet } from 'svelte'

  let { children }: { children: Snippet } = $props()

  let projectError = $state<string | null>(null)

  // Keep xterm's textarea focus aligned with the active session.
  //
  // Two classes of problem this solves:
  //   1. Tab/pane switches. When the user moves between sessions, a
  //      different SessionTerminal becomes visible but the previously
  //      clicked terminal keeps real DOM focus. Visible-but-unfocused
  //      xterm means Shift+Tab falls through to the browser's focus-
  //      navigation instead of reaching the PTY — feels like "terminal
  //      keys stopped working on this tab."
  //   2. Transient modals (palette, settings). bits-ui restores focus
  //      to the activeElement captured at open time. If the command
  //      that ran mutated the DOM (new tab, close tab, re-attach),
  //      that reference is stale and focus falls to <body>.
  //
  // One effect tracks both signals. We refocus the current active
  // session's xterm on any change, unless a transient modal is
  // currently open — in which case the modal owns keyboard focus and
  // the effect will fire again when it closes. rAF waits one frame so
  // bits-ui's own restoration attempt completes first and we override
  // it last.
  const activeSessionId = $derived(getActiveSessionId())
  const anyModalOpen = $derived(isAnyModalOpen())

  $effect(() => {
    const id = activeSessionId
    const open = anyModalOpen
    if (open || !id) return
    requestAnimationFrame(() => sessionRegistry.focus(id))
  })

  onMount(() => {
    const appWindow = getCurrentWindow()

    const unlisten = appWindow.onCloseRequested(async (event) => {
      // Guard before any teardown — once we've started flushing the
      // user has effectively committed to closing.
      if (hasAnyDirtyTextSurfaces()) {
        const count = getDirtyTextSurfaceCount()
        const noun = count === 1 ? 'file' : 'files'
        const proceed = await confirmAsync({
          title: 'Unsaved changes',
          message: `You have ${count} unsaved ${noun}. Quit and lose changes?`,
          confirmLabel: 'Quit',
          cancelLabel: 'Keep editing'
        })
        if (!proceed) {
          event.preventDefault()
          return
        }
      }

      // Persist pending workspace mutations before tearing down — the
      // backend `Exit` event flushes transcripts but knows nothing
      // about the frontend's debounce queue.
      try {
        await flushPersistencePending()
      } catch (error) {
        console.warn('Close-request flush failed:', error)
      }
      disposeAll()
    })

    // Restore system decorations if user previously chose that
    const wc = getWindowControls()
    if (wc.useSystemDecorations) {
      appWindow.setDecorations(true)
    }

    const cleanupShortcuts = setupGlobalShortcuts()

    // Fetch project-scoped JSON schemas (tasks, settings, ...) from
    // the backend and push them into the Monaco registry. Fire-and-
    // forget: schemas apply whenever they arrive, and a missing schema
    // just means no autocomplete, not a broken editor.
    initProjectSchemas().catch((err) => {
      console.warn('Failed to initialize project config schemas:', err)
    })

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
    <TitleBar onNewProject={handleNewProject} onSettings={() => setSettingsOpen(true)} />

    <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
      {@render children()}
    </main>

    <StatusBar />
  </div>

  <!-- Dialogs and overlays live under the same Tooltip.Provider as the
       main app so IconButtons inside them (e.g. Settings close) find a
       provider without needing a per-surface one. Nested providers
       confuse bits-ui's cross-tooltip coordination. -->
  <CommandCenter onNewProject={handleNewProject} onSettings={() => setSettingsOpen(true)} />
  <SettingsDialog open={isSettingsOpen()} onClose={() => setSettingsOpen(false)} />
  <NotificationsSurface />
  <ConfirmHost />
</TooltipProvider>

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
