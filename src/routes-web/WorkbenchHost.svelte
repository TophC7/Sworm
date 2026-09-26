<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import AppShell from '$lib/features/app-shell/AppShell.svelte'
  import WorkbenchPage from '$lib/features/app-shell/WorkbenchPage.svelte'
  import { Button } from '$lib/components/ui/button'
  import { statusChipVariants } from '$lib/components/ui/status-chip'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import { cn } from '$lib/utils/cn'
  import { backend } from '$lib/api/backend'
  import { setWorkbenchId } from '$lib/features/workbench/state.svelte'
  import { flushWorkbench, stopWorkbenchPersistence } from '$lib/features/workbench/persistence'
  import { hasAnyDirtyTextSurfaces } from '$lib/features/workbench/surfaces/text/service.svelte'
  import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
  import * as taskRegistry from '$lib/features/tasks/taskRegistry'
  import { registerHostTransport } from '$lib/api/transport'
  import {
    createWebHostTransport,
    type WebConnectionState,
    type WebHandshakeError,
    type WebHostTransport
  } from '$lib/api/transport.web'
  import { installPlatform } from '$lib/platform'
  import { createWebPlatform } from '$lib/platform/web'
  import { createWebRecovery } from './recovery'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { logClientError } from '$lib/utils/client-error'

  type Surface = 'connecting' | 'workbench' | 'busy' | 'revoked' | 'closed' | 'error'

  const props: { workbenchId: string } = $props()
  // Workbench identity is fixed for the document; capture it once so the
  // platform, recovery, and transport are installed synchronously and never re-bound.
  const workbenchId = untrack(() => props.workbenchId)

  let surface = $state<Surface>('connecting')
  let connectionState = $state<WebConnectionState>('connecting')
  let errorMessage = $state<string | null>(null)
  let actions = $state<HTMLDivElement>()
  let tornDown = $state(false)
  let disposed = false
  // Set only after the Close Workbench confirmation, immediately before the request.
  let closeRequested = false
  // This document is navigating away on purpose (own close or bfcache reload); skip the leave prompt.
  let leaving = false

  const recovery = createWebRecovery(workbenchId)
  installPlatform(
    createWebPlatform(workbenchId, {
      closeCurrent,
      // Resolve lazily so takeover's replacement adapter serves media reads.
      readFileBytes: (request) => transport.readFileBytes(request)
    })
  )
  // Adapters report state synchronously during construction; the attempt number
  // fences callbacks from a disposed busy adapter after Take Over.
  let attempt = 0
  let transport = connect(false)
  setWorkbenchId(workbenchId)

  function connect(takeover: boolean): WebHostTransport {
    const current = ++attempt
    const isCurrent = () => !disposed && attempt === current
    const adapter = createWebHostTransport({
      url: new URL('/ws', window.location.href),
      workbenchId,
      takeover,
      onConnectionState(state, error) {
        if (!isCurrent()) return
        if (state === 'closed' && closeRequested) {
          finishClose()
          return
        }
        if (state === 'busy' || state === 'revoked' || state === 'closed' || state === 'error') {
          terminate(state, error)
          return
        }
        // A close whose outcome is unknown after connection loss is never resubmitted or assumed.
        if (state === 'reconnecting') {
          closeRequested = false
          recovery.onDisconnected()
        }
        connectionState = state
      },
      onReconnected() {
        if (!isCurrent()) return
        void recovery.onReconnected().catch((error: unknown) => {
          if (disposed) return
          logClientError('web recovery failed', { error })
          notify.error('Could not restore web connection', getErrorMessage(error))
        })
      },
      onLspDisconnected(serverDefinitionId) {
        if (isCurrent()) recovery.onLspDisconnected(serverDefinitionId)
      }
    })
    registerHostTransport(adapter)
    void adapter.ready
      .then(() => {
        if (isCurrent() && surface === 'connecting') surface = 'workbench'
      })
      .catch((error: unknown) => {
        // Terminal states report synchronously before this rejection handler runs.
        if (!isCurrent() || surface !== 'connecting') return
        surface = 'error'
        errorMessage = getErrorMessage(error)
        logClientError('web connection failed', { error })
      })
    return adapter
  }

  /** Stops this page's writers and detaches (never stops) its runs; only a fresh document may control the workbench again. */
  function teardown(): void {
    if (tornDown) return
    tornDown = true
    recovery.dispose()
    stopWorkbenchPersistence()
    sessionRegistry.disposeAll()
    taskRegistry.disposeAll()
  }

  function terminate(state: 'busy' | 'revoked' | 'closed' | 'error', error?: WebHandshakeError): void {
    closeRequested = false
    // The transport already rejects every call; stop this page's writers before unmounting the workbench.
    if (surface === 'workbench') teardown()
    connectionState = state
    if (state !== 'error') {
      surface = state
      return
    }
    surface = 'error'
    errorMessage = error ? getErrorMessage(error) : 'Connection failed'
    logClientError('web connection failed', { error })
  }

  /** Makes this document inert before a deliberate navigation; idempotent. */
  function leave(): void {
    if (leaving) return
    leaving = true
    closeRequested = false
    teardown()
    disposed = true
    transport.dispose()
    document.body.inert = true
  }

  function finishClose(): void {
    if (leaving) return
    leave()
    window.location.assign('/')
  }

  async function closeCurrent(): Promise<void> {
    // Never queue a destructive request while reconnecting or after this page lost control.
    if (surface !== 'workbench' || connectionState !== 'connected' || closeRequested || leaving) {
      throw new Error('Workbench is not connected')
    }
    closeRequested = true
    try {
      await backend.workbenches.close(workbenchId)
    } catch (error) {
      if (leaving) return
      closeRequested = false
      throw error
    }
    // The reply can precede the terminal `closed` frame; both take the same finish path.
    if (closeRequested) finishClose()
  }

  function takeOver(): void {
    transport.dispose()
    surface = 'connecting'
    connectionState = 'connecting'
    transport = connect(true)
  }

  const heading = $derived(
    {
      connecting: '',
      workbench: '',
      busy: 'Open Elsewhere',
      revoked: 'Taken Over',
      closed: 'Workbench Closed',
      error: 'Connection Failed'
    }[surface]
  )

  $effect(() => {
    actions?.querySelector('button')?.focus()
  })

  // Document-scoped: stays armed on revoked/closed surfaces while discarded edits remain dirty.
  $effect(() => {
    if (!hasAnyDirtyTextSurfaces()) return
    const guard = (event: BeforeUnloadEvent) => {
      // `leaving` is read at event time: effect cleanup would run after a synchronous navigation.
      if (leaving) return
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', guard)
    return () => window.removeEventListener('beforeunload', guard)
  })

  $effect(() => {
    const onVisibilityChange = () => {
      if (
        document.visibilityState !== 'hidden' ||
        surface !== 'workbench' ||
        connectionState !== 'connected' ||
        tornDown ||
        closeRequested ||
        leaving
      ) {
        return
      }
      // Failures requeue inside persistence; reconnect recovery or the next mutation retries.
      void flushWorkbench(workbenchId).catch((error: unknown) => {
        logClientError('workbench flush on hide failed', { error })
      })
    }
    const onPageShow = (event: PageTransitionEvent) => {
      if (!event.persisted) return
      // A restored bfcache document must not regain control before a fresh handshake.
      leave()
      window.location.reload()
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    window.addEventListener('pageshow', onPageShow)
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange)
      window.removeEventListener('pageshow', onPageShow)
    }
  })

  onDestroy(() => {
    disposed = true
    recovery.dispose()
    transport.dispose()
  })
</script>

{#if surface === 'workbench'}
  <AppShell>
    {#snippet connectionStatus()}
      {@const connected = connectionState === 'connected'}
      <span role="status" aria-live="polite" class="inline-flex">
        <TooltipRoot>
          <TooltipTrigger
            class={statusChipVariants({ tone: connected ? 'default' : 'warning', class: 'cursor-default' })}
          >
            <span class={cn('size-1.5 shrink-0 rounded-full', connected ? 'bg-success' : 'bg-warning')}></span>
            {connected ? 'Connected' : 'Reconnecting…'}
          </TooltipTrigger>
          <TooltipContent class="max-w-md">
            {connected
              ? 'Control connection to server is connected.'
              : 'Disconnected from server. Reconnecting automatically.'}
            Terminal, LSP, and file streams have independent state.
          </TooltipContent>
        </TooltipRoot>
      </span>
    {/snippet}
    <WorkbenchPage />
  </AppShell>
{:else if surface === 'connecting'}
  <div role="status" class="flex h-screen items-center justify-center bg-ground text-base text-muted">
    Connecting to server…
  </div>
{:else}
  <div class="flex h-screen flex-col items-center justify-center gap-3 bg-ground p-6 text-center">
    <h1 class="text-3xl text-bright">{heading}</h1>
    {#if surface === 'busy'}
      <p class="text-base text-muted">This workbench is controlled by another page.</p>
    {:else if surface === 'revoked'}
      <p class="text-base text-muted">Another page controls this workbench.</p>
    {:else if surface === 'error'}
      <p role="alert" class="max-w-xl text-base break-words text-danger-bright">{errorMessage}</p>
    {/if}
    <div bind:this={actions} class="mt-2 flex gap-2.5">
      {#if surface === 'busy' && !tornDown}
        <Button variant="accent" onclick={takeOver}>Take Over</Button>
      {:else if surface === 'busy' || surface === 'revoked' || surface === 'error'}
        <Button onclick={() => window.location.reload()}>Reload</Button>
      {/if}
      <Button onclick={() => window.location.assign('/')}>New Workbench</Button>
    </div>
  </div>
{/if}
