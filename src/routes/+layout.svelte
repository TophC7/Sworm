<script lang="ts">
  import '../app.css'
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
  import * as taskRegistry from '$lib/features/tasks/taskRegistry'
  import { backend } from '$lib/api/backend'
  import { refreshIssuesForFolder } from '$lib/features/issues/state.svelte'
  import { refreshNixForFolder } from '$lib/features/settings/state/nix.svelte'
  import CommandCenter from '$lib/features/command-palette/CommandCenter.svelte'
  import ConfirmHost from '$lib/features/confirm/ConfirmHost.svelte'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import NotificationsSurface from '$lib/features/notifications/NotificationsSurface.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import SettingsDialog from '$lib/features/settings/dialog/SettingsDialog.svelte'
  import StatusBar from '$lib/features/app-shell/status/StatusBar.svelte'
  import TitleBar from '$lib/features/app-shell/titlebar/TitleBar.svelte'
  import { TooltipProvider } from '$lib/components/ui/tooltip'
  import { getWindowControls } from '$lib/features/app-shell/window-controls/state.svelte'
  import { isSettingsOpen, setSettingsOpen } from '$lib/features/settings/dialog/state.svelte'
  import { isAnyModalOpen } from '$lib/utils/modalRegistry.svelte'
  import { setupGlobalShortcuts } from '$lib/features/command-palette/shortcuts/setup.svelte'
  import { openSettings } from '$lib/features/app-actions/actions.svelte'
  import { initProjectSchemas } from '$lib/features/project-config/bootstrap'
  import {
    getDirtyTextSurfaceCount,
    hasAnyDirtyTextSurfaces
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { flushWorkbench } from '$lib/features/workbench/persistence'
  import { getActiveSessionTabId, requestFocusTab } from '$lib/features/workbench/state.svelte'
  import { initTransferService } from '$lib/features/workbench/transferService.svelte'
  import type { Snippet } from 'svelte'

  let { children }: { children: Snippet } = $props()

  // Keep xterm's textarea focus aligned with the active session.
  //
  // Two classes of problem this solves:
  //   1. Tab switches. When the user moves between sessions, a
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
  const activeSessionTabId = $derived(getActiveSessionTabId())
  const anyModalOpen = $derived(isAnyModalOpen())

  $effect(() => {
    const id = activeSessionTabId
    const open = anyModalOpen
    if (open || !id || !document.hasFocus()) return
    requestAnimationFrame(() => {
      if (document.hasFocus()) sessionRegistry.focus(id)
    })
  })

  onMount(() => {
    const appWindow = getCurrentWindow()
    let cleanupTransfer: (() => void) | undefined
    let disposed = false
    const listeners = [
      backend.issues.onChanged(({ folderPath }) => refreshIssuesForFolder(folderPath)),
      backend.nix.onChanged(({ folderPath }) => refreshNixForFolder(folderPath)),
      backend.window.onFocusTab((payload) => requestFocusTab(payload.tabId, payload.reveal))
    ]
    void initTransferService().then((cleanup) => {
      if (disposed) cleanup()
      else cleanupTransfer = cleanup
    })

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

      // Persist pending workbench mutations before tearing down — the
      // backend knows nothing about the frontend's debounce queue. A
      // failed write would silently lose the layout, so let the user
      // choose between quitting anyway and keeping the app open.
      try {
        await flushWorkbench(appWindow.label)
      } catch (error) {
        const proceed = await confirmAsync({
          title: 'Could not save workbench layout',
          message: `${getErrorMessage(error)}\n\nQuit anyway? Open tabs will not be restored.`,
          confirmLabel: 'Quit',
          cancelLabel: 'Keep working'
        })
        if (!proceed) {
          event.preventDefault()
          return
        }
      }
      sessionRegistry.disposeAll()
      taskRegistry.disposeAll()
    })

    // Restore system decorations if user previously chose that
    const wc = getWindowControls()
    if (wc.useSystemDecorations) {
      appWindow.setDecorations(true)
    }

    const cleanupShortcuts = setupGlobalShortcuts()

    // Fetch folder-scoped JSON schemas (tasks, settings, ...) from
    // the backend and push them into the Monaco registry. Fire-and-
    // forget: schemas apply whenever they arrive, and a missing schema
    // just means no autocomplete, not a broken editor.
    initProjectSchemas().catch((err) => {
      console.warn('Failed to initialize project config schemas:', err)
    })

    return () => {
      disposed = true
      cleanupTransfer?.()
      cleanupShortcuts()
      for (const listener of listeners) listener.then((cleanup) => cleanup()).catch(() => {})
      unlisten.then((cleanup) => cleanup()).catch(() => {})
    }
  })
</script>

<TooltipProvider delayDuration={300}>
  <div class="flex h-screen flex-col overflow-hidden">
    <TitleBar onSettings={openSettings} />

    <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
      {@render children()}
    </main>

    <StatusBar />
  </div>

  <!-- Dialogs and overlays live under the same Tooltip.Provider as the
       main app so IconButtons inside them (e.g. Settings close) find a
       provider without needing a per-surface one. Nested providers
       confuse bits-ui's cross-tooltip coordination. -->
  <CommandCenter />
  <SettingsDialog open={isSettingsOpen()} onClose={() => setSettingsOpen(false)} />
  <NotificationsSurface />
  <ConfirmHost />
</TooltipProvider>
