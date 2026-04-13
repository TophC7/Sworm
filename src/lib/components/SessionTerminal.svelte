<script lang="ts">
  import '@xterm/xterm/css/xterm.css'
  import { onMount } from 'svelte'
  import type { Session, SessionStatus } from '$lib/types/backend'
  import * as sessionRegistry from '$lib/terminal/sessionRegistry'
  import type { TerminalSessionManager } from '$lib/terminal/TerminalSessionManager'

  let {
    session,
    locked = false,
    onStatusChange
  }: {
    session: Session
    locked?: boolean
    onStatusChange?: (status: SessionStatus) => void
  } = $props()

  let containerEl: HTMLDivElement | undefined = $state(undefined)
  let manager: TerminalSessionManager | null = $state(null)
  let error = $state<string | null>(null)

  let mounted = false
  let attachedSessionId: string | null = null
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
        onStatusChange?.('running')
      }

      if (event.type === 'exit') {
        onStatusChange?.('exited')
      }
    })

    cleanupErrorListener = nextManager.registerErrorListener((message) => {
      error = message
    })
  }

  async function attachSession(nextSession: Session) {
    if (!mounted || !containerEl) {
      return
    }

    const switchingSessions = attachedSessionId !== nextSession.id

    if (attachedSessionId && attachedSessionId !== nextSession.id) {
      sessionRegistry.detach(attachedSessionId)
      clearManagerListeners()
    }

    const nextManager = await sessionRegistry.attach(nextSession.id, containerEl)
    attachedSessionId = nextSession.id
    bindManager(nextManager)

    // Auto-start when switching to a session whose PTY isn't active.
    // Covers both webview-reload reconnection (status running/starting)
    // and opening historical sessions from the sidebar (status stopped/exited/idle).
    // Skip failed sessions so the user sees the error before retrying.
    const shouldAutoStart = switchingSessions && !nextManager.isPtyActive() && nextSession.status !== 'failed'

    if (shouldAutoStart) {
      try {
        await nextManager.startPty(nextSession)
      } catch (startError) {
        error = String(startError)
        onStatusChange?.('failed')
      }
    }
  }

  onMount(() => {
    mounted = true
    void attachSession(session)

    return () => {
      if (attachedSessionId) {
        sessionRegistry.detach(attachedSessionId)
      }
      clearManagerListeners()
    }
  })

  $effect(() => {
    if (!mounted || !containerEl) {
      return
    }

    void attachSession(session)
  })

  $effect(() => {
    manager?.setInputEnabled(!locked)
  })
</script>

<div class="flex min-h-0 flex-1 flex-col bg-ground">
  {#if error}
    <div class="border-b border-danger-border bg-danger-bg px-2.5 py-1.5 text-[0.8rem] text-danger">
      {error}
    </div>
  {/if}

  <div class="min-h-0 flex-1" bind:this={containerEl}></div>
</div>
