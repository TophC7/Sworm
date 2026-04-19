<script lang="ts">
  import '@xterm/xterm/css/xterm.css'
  import { onMount, untrack } from 'svelte'
  import { isTerminalDropActive, terminalDropObserver } from '$lib/dnd/adapters/terminal.svelte'
  import type { Session, SessionStatus } from '$lib/types/backend'
  import * as sessionRegistry from '$lib/terminal/sessionRegistry'
  import type { TerminalSessionManager } from '$lib/terminal/TerminalSessionManager'

  let {
    session,
    projectId,
    projectPath,
    locked = false,
    onStatusChange
  }: {
    session: Session
    projectId: string
    projectPath: string
    locked?: boolean
    onStatusChange?: (status: SessionStatus) => void
  } = $props()

  let containerEl: HTMLDivElement | undefined = $state(undefined)
  let manager: TerminalSessionManager | null = $state(null)
  let error = $state<string | null>(null)
  let isHistorical = $state(false)
  let canAcceptDrop = $derived.by(() => {
    if (locked || isHistorical || !manager) return false
    return manager.isPtyActive()
  })
  let dropActive = $derived(canAcceptDrop && isTerminalDropActive(session.id))

  let attachedSessionId: string | null = null
  // Generation counter invalidates stale awaits. Rapid A→B→A tab
  // switches would otherwise have two attach calls in flight; the
  // slower one would overwrite state the newer one already set.
  let attachGen = 0
  let cleanupEventListener: (() => void) | null = null
  let cleanupErrorListener: (() => void) | null = null

  function clearManagerListeners() {
    cleanupEventListener?.()
    cleanupEventListener = null
    cleanupErrorListener?.()
    cleanupErrorListener = null
  }

  function bindManager(nextManager: TerminalSessionManager) {
    clearManagerListeners()

    manager = nextManager
    nextManager.setInputEnabled(!locked)
    error = nextManager.getLastError()

    cleanupEventListener = nextManager.registerEventListener((event) => {
      if (event.type === 'started') {
        error = null
        isHistorical = false
        onStatusChange?.('running')
      }

      if (event.type === 'exit') {
        isHistorical = true
        onStatusChange?.('exited')
      }
    })

    cleanupErrorListener = nextManager.registerErrorListener((message) => {
      error = message
    })
  }

  // loadTranscript must complete before attachLive so live bytes can't
  // reorder ahead of history. Auto-start is reserved for genuinely
  // fresh sessions — restored tabs stay historical until the user
  // explicitly resumes.
  async function attachSession(nextSession: Session) {
    if (!containerEl) return
    const gen = ++attachGen

    if (attachedSessionId && attachedSessionId !== nextSession.id) {
      sessionRegistry.detach(attachedSessionId)
      clearManagerListeners()
    }

    const nextManager = await sessionRegistry.attach(nextSession.id, containerEl)
    if (gen !== attachGen) return
    attachedSessionId = nextSession.id
    bindManager(nextManager)

    if (nextManager.isPtyActive()) {
      isHistorical = false
      return
    }

    await nextManager.loadTranscript()
    if (gen !== attachGen) return

    let resumed = false
    try {
      resumed = await nextManager.attachLive(nextSession)
    } catch (resumeError) {
      error = String(resumeError)
    }
    if (gen !== attachGen) return

    if (resumed) {
      isHistorical = false
      return
    }

    const isFreshlyCreated = nextSession.status === 'idle' && !nextManager.hasHistory()
    if (!isFreshlyCreated) {
      isHistorical = true
      return
    }

    isHistorical = false
    try {
      await nextManager.startPty(nextSession)
    } catch (startError) {
      if (gen !== attachGen) return
      error = String(startError)
      onStatusChange?.('failed')
    }
  }

  onMount(() => {
    void attachSession(session)

    return () => {
      if (attachedSessionId) {
        sessionRegistry.detach(attachedSessionId)
      }
      clearManagerListeners()
    }
  })

  // Re-attach only when the session *id* changes. Pane.svelte derives
  // `session` from `sessions.find(...)`, and every status update in the
  // sessions array yields a new object reference — tracking identity
  // here would re-run attach on every status tick and storm the PTY
  // with resizes.
  $effect(() => {
    const id = session.id
    if (!containerEl) return
    if (id === attachedSessionId) return
    untrack(() => void attachSession(session))
  })

  // Effective input lock: disabled when the user has locked the tab
  // OR when there is no live PTY (historical mode). Without this the
  // user could type into a dead transcript and the keys would silently
  // disappear.
  $effect(() => {
    manager?.setInputEnabled(!locked && !isHistorical)
  })

  async function resumeSession() {
    if (!manager) return
    error = null
    try {
      await manager.startPty(session)
      isHistorical = false
    } catch (startError) {
      error = String(startError)
      onStatusChange?.('failed')
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col bg-ground">
  {#if error}
    <div class="border-b border-danger-border bg-danger-bg px-2.5 py-1.5 text-base text-danger">
      {error}
    </div>
  {/if}

  {#if isHistorical && !error}
    <div
      class="flex items-center justify-between gap-2 border-b border-edge bg-raised px-2.5 py-1.5 text-base text-muted"
    >
      <span>Session exited. Showing restored terminal history.</span>
      <button
        type="button"
        class="text-foreground rounded border border-edge bg-surface px-2 py-0.5 text-sm transition-colors hover:bg-overlay"
        onclick={resumeSession}
      >
        Resume
      </button>
    </div>
  {/if}

  <div
    class="relative min-h-0 flex-1"
    bind:this={containerEl}
    {@attach terminalDropObserver({
      sessionId: session.id,
      projectId,
      projectPath,
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
