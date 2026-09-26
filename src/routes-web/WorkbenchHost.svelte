<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import AppShell from '$lib/features/app-shell/AppShell.svelte'
  import WorkbenchPage from '$lib/features/app-shell/WorkbenchPage.svelte'
  import { Button } from '$lib/components/ui/button'
  import { setWorkbenchId } from '$lib/features/workbench/state.svelte'
  import { stopWorkbenchPersistence } from '$lib/features/workbench/persistence'
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

  const recovery = createWebRecovery(workbenchId)
  installPlatform(createWebPlatform(workbenchId))
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
        if (state === 'busy' || state === 'revoked' || state === 'closed' || state === 'error') {
          terminate(state, error)
          return
        }
        if (state === 'reconnecting') recovery.onDisconnected()
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

  function terminate(state: 'busy' | 'revoked' | 'closed' | 'error', error?: WebHandshakeError): void {
    // The transport already rejects every call; stop this page's writers before unmounting the workbench.
    if (surface === 'workbench') {
      // Persistence is permanently stopped; only a fresh document may control this workbench again.
      tornDown = true
      recovery.dispose()
      stopWorkbenchPersistence()
      sessionRegistry.disposeAll()
      taskRegistry.disposeAll()
    }
    connectionState = state
    if (state !== 'error') {
      surface = state
      return
    }
    surface = 'error'
    errorMessage = error ? getErrorMessage(error) : 'Connection failed'
    logClientError('web connection failed', { error })
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

  onDestroy(() => {
    disposed = true
    recovery.dispose()
    transport.dispose()
  })
</script>

{#if surface === 'workbench'}
  <AppShell><WorkbenchPage /></AppShell>
  {#if connectionState === 'reconnecting'}
    <div
      role="status"
      class="pointer-events-none fixed top-10 right-3 z-10 border border-edge bg-surface px-3 py-2 text-sm text-warning"
    >
      Disconnected from server. Reconnecting…
    </div>
  {/if}
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
