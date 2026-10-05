<script lang="ts">
  import '@xterm/xterm/css/xterm.css'
  import { onMount, untrack } from 'svelte'
  import { isTerminalDropActive, terminalDropObserver } from '$lib/features/dnd/adapters/terminal.svelte'
  import type { SessionTab, TabId } from '$lib/features/workbench/model'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { startSessionProcess } from '$lib/features/sessions/service.svelte'
  import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
  import type { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'
  import { getActiveSessionTabId } from '$lib/features/workbench/state.svelte'
  import { isTabTransferring } from '$lib/features/workbench/transferService.svelte'
  import { isAnyModalOpen } from '$lib/utils/modalRegistry.svelte'

  // Dormant tabs (restored or newly created) wait this long after
  // activation before spawning, so flicking through tabs doesn't start
  // every process the cursor passes over.
  const START_DELAY_MS = 300

  let { tab }: { tab: SessionTab } = $props()

  let containerEl: HTMLDivElement | undefined = $state(undefined)
  let manager: TerminalSessionManager | null = $state(null)
  let error = $state<string | null>(null)
  let reconnecting = $state(false)
  let ended = $derived(tab.status === 'exited' || tab.status === 'failed')
  let canAcceptDrop = $derived(!tab.locked && tab.status === 'running')
  let dropActive = $derived(canAcceptDrop && isTerminalDropActive(tab.id))

  let attachedTabId: TabId | null = null
  // Generation counter invalidates stale awaits. Rapid A→B→A tab
  // switches would otherwise have two attach calls in flight; the
  // slower one would overwrite state the newer one already set.
  let attachGen = 0
  let startTimer: ReturnType<typeof setTimeout> | null = null
  let cleanupErrorListener: (() => void) | null = null
  let cleanupReconnectListener: (() => void) | null = null

  function clearManagerListeners() {
    cleanupErrorListener?.()
    cleanupErrorListener = null
    cleanupReconnectListener?.()
    cleanupReconnectListener = null
    reconnecting = false
  }

  function cancelStartTimer() {
    if (startTimer) clearTimeout(startTimer)
    startTimer = null
  }

  function bindManager(nextManager: TerminalSessionManager) {
    clearManagerListeners()

    manager = nextManager
    nextManager.setInputEnabled(!tab.locked)
    error = nextManager.getLastError()

    cleanupErrorListener = nextManager.registerErrorListener((message) => {
      error = message
    })
    cleanupReconnectListener = nextManager.registerReconnectListener((value) => {
      reconnecting = value
    })
  }

  // Give xterm focus once attach settles. Bridges the gap the layout-
  // level focus effect can't cover: on a brand-new session tab the
  // effect fires before attach resolves, so `manager.terminal` is
  // still null and focus() no-ops. Calling here runs after xterm
  // actually exists. Modal-open guard keeps the command palette /
  // settings dialog keyboard focus intact. The active-session check
  // prevents a slow attach from stealing focus if the user has since
  // moved to a different tab. Double-try (now + next frame) because
  // xterm's inner textarea isn't always in the DOM the instant open()
  // returns — first call covers the fast path, rAF covers the slow
  // path after layout/fit.
  function focusIfCurrent(mgr: TerminalSessionManager) {
    if (isAnyModalOpen()) return
    if (mgr.tabId !== getActiveSessionTabId()) return
    mgr.focus()
    requestAnimationFrame(() => {
      if (mgr.tabId !== getActiveSessionTabId()) return
      mgr.focus()
    })
  }

  async function startProcess(mgr: TerminalSessionManager) {
    error = null
    try {
      await startSessionProcess(mgr, tab)
      focusIfCurrent(mgr)
    } catch (startError) {
      error = getErrorMessage(startError)
    }
  }

  async function attachSession(tabId: TabId) {
    if (!containerEl) return
    const gen = ++attachGen
    cancelStartTimer()

    if (attachedTabId && attachedTabId !== tabId) {
      sessionRegistry.detach(attachedTabId)
      clearManagerListeners()
    }

    const nextManager = sessionRegistry.getOrCreate(tabId)
    await nextManager.attach(containerEl)
    if (gen !== attachGen) return
    attachedTabId = tabId
    bindManager(nextManager)
    focusIfCurrent(nextManager)
  }

  onMount(() => {
    return () => {
      cancelStartTimer()
      if (attachedTabId) {
        sessionRegistry.detach(attachedTabId)
      }
      clearManagerListeners()
    }
  })

  // Re-attach only when the tab *id* changes. Every status update
  // yields a new tab object — tracking identity here would re-run
  // attach on every status tick and storm the PTY with resizes.
  $effect(() => {
    const id = tab.id
    if (!containerEl) return
    if (id === attachedTabId) return
    untrack(() => void attachSession(id))
  })
  // A handoff freezes deferred starts too; an aborted move restores the dormant timer.
  $effect(() => {
    const current = manager
    const id = tab.id
    if (!current || id !== attachedTabId || tab.status !== 'dormant' || isTabTransferring(id)) return
    if (untrack(() => current.isPtyActive())) return
    startTimer = setTimeout(() => {
      startTimer = null
      if (tab.id !== id || tab.status !== 'dormant' || isTabTransferring(id)) return
      void startProcess(current)
    }, START_DELAY_MS)
    return cancelStartTimer
  })

  $effect(() => {
    manager?.setInputEnabled(!tab.locked && !isTabTransferring(tab.id))
  })

  function restart() {
    if (!manager) return
    void startProcess(manager)
  }
</script>

<div class="relative flex min-h-0 flex-1 flex-col bg-ground">
  {#if reconnecting}
    <div
      role="status"
      class="pointer-events-none absolute top-2 right-2 z-20 flex items-center gap-1.5 rounded border border-edge bg-raised px-2 py-1 text-xs text-muted"
    >
      <span class="h-1.5 w-1.5 rounded-full bg-warning"></span>
      Reconnecting…
    </div>
  {/if}
  {#if error}
    <div class="border-b border-danger-border bg-danger-bg px-2.5 py-1.5 text-base text-danger">
      {error}
    </div>
  {/if}

  {#if ended}
    <div
      class="flex items-center justify-between gap-2 border-b border-edge bg-raised px-2.5 py-1.5 text-base text-muted"
    >
      <span>{tab.status === 'failed' ? 'Process failed to start.' : 'Process exited.'}</span>
      <button
        type="button"
        class="text-foreground rounded border border-edge bg-surface px-2 py-0.5 text-sm transition-colors hover:bg-overlay"
        onclick={restart}
      >
        Restart
      </button>
    </div>
  {/if}

  <!-- `data-terminal-focus-scope` is read by the global keybinding
       dispatcher: when DOM focus lives anywhere inside this subtree
       (xterm's hidden textarea, etc.) non-`skipShell` shortcuts yield
       to the PTY so readline/tmux/nvim keep their native bindings. -->
  <div
    class="relative min-h-0 flex-1"
    data-terminal-focus-scope
    bind:this={containerEl}
    {@attach terminalDropObserver({
      tabId: tab.id,
      folderPath: tab.folderPath,
      canAcceptDrop: () => canAcceptDrop,
      onInsertText: (text) => manager?.sendText(text)
    })}
  >
    {#if dropActive}
      <div class="pointer-events-none absolute inset-1 z-20 rounded-lg border border-accent/60 bg-accent/10">
        <div class="absolute inset-0 flex items-center justify-center">
          <span
            class="rounded border border-edge-strong/70 bg-raised/90 px-2 py-0.5 text-xs font-medium tracking-wide text-bright uppercase"
          >
            Insert Path
          </span>
        </div>
      </div>
    {/if}
  </div>
</div>
