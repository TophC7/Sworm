<script lang="ts">
  import '../app.css'
  import { onDestroy } from 'svelte'
  import type { Snippet } from 'svelte'
  import AppShell from '$lib/features/app-shell/AppShell.svelte'
  import { setWorkbenchId } from '$lib/features/workbench/state.svelte'
  import { registerHostTransport } from '$lib/api/transport'
  import { createWebHostTransport, type WebConnectionState } from '$lib/api/transport.web'
  import { installPlatform } from '$lib/platform'
  import { createWebPlatform } from '$lib/platform/web'
  import { createWebRecovery } from './recovery'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { logClientError } from '$lib/utils/client-error'

  let { children }: { children: Snippet } = $props()
  const secure = window.isSecureContext && typeof window.crypto?.randomUUID === 'function' && !!window.crypto.subtle
  let mounted = $state(false)
  let connectionState = $state<WebConnectionState>('connecting')
  let connectionError = $state<string | null>(null)
  let transport: ReturnType<typeof createWebHostTransport> | undefined
  let recovery: ReturnType<typeof createWebRecovery> | undefined
  let disposed = false

  if (secure) {
    const url = new URL(window.location.href)
    let id = url.searchParams.get('workbench')
    if (!id) {
      id = window.crypto.randomUUID()
      url.searchParams.set('workbench', id)
      window.history.replaceState(window.history.state, '', url)
    }

    recovery = createWebRecovery(id)
    installPlatform(createWebPlatform(id))
    transport = createWebHostTransport({
      url: new URL('/ws', window.location.href),
      onConnectionState(state) {
        if (disposed) return
        if (state === 'reconnecting') recovery?.onDisconnected()
        connectionState = state
      },
      onReconnected() {
        if (disposed) return
        void recovery?.onReconnected().catch((error: unknown) => {
          if (disposed) return
          logClientError('web recovery failed', { error })
          notify.error('Could not restore web connection', getErrorMessage(error))
        })
      },
      onLspDisconnected(serverDefinitionId) {
        if (!disposed) recovery?.onLspDisconnected(serverDefinitionId)
      }
    })
    registerHostTransport(transport)
    setWorkbenchId(id)
    void transport.ready
      .then(() => {
        if (!disposed) mounted = true
      })
      .catch((error: unknown) => {
        if (disposed) return
        connectionError = getErrorMessage(error)
        logClientError('web connection failed', { error })
      })
  }

  onDestroy(() => {
    disposed = true
    recovery?.dispose()
    transport?.dispose()
  })
</script>

{#if !secure}
  <div class="flex h-screen flex-col items-center justify-center gap-2 bg-ground p-6 text-center">
    <h1 class="text-3xl text-bright">Secure Context Required</h1>
    <p class="text-base text-muted">Use localhost HTTP or an HTTPS reverse proxy to open Sworm Web.</p>
  </div>
{:else if connectionError}
  <div role="alert" class="flex h-screen items-center justify-center bg-ground p-6 text-base text-danger-bright">
    {connectionError}
  </div>
{:else if !mounted}
  <div role="status" class="flex h-screen items-center justify-center bg-ground text-base text-muted">
    Connecting to server…
  </div>
{:else}
  <AppShell>{@render children()}</AppShell>
  {#if connectionState === 'reconnecting'}
    <div
      role="status"
      class="pointer-events-none fixed top-10 right-3 z-10 border border-edge bg-surface px-3 py-2 text-sm text-warning"
    >
      Disconnected from server. Reconnecting…
    </div>
  {/if}
{/if}
