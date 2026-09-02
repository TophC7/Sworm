<script lang="ts">
  import { onMount } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import { backend } from '$lib/api/backend'
  import { preloadBuiltinCatalog } from '$lib/features/builtins/catalog'
  import WorkbenchView from '$lib/features/app-shell/WorkbenchView.svelte'
  import { loadRecentFolders } from '$lib/features/folders/state.svelte'
  import { loadProviders } from '$lib/features/sessions/providers/state.svelte'
  import { openFolder, restoreWorkbench } from '$lib/features/workbench/state.svelte'
  import { describeClientError, logClientError } from '$lib/utils/client-error'

  let bootstrapping = $state(true)
  let bootstrapError = $state<string | null>(null)

  // Drain the argv-supplied path (from Nautilus "Open With" or a CLI
  // invocation) and open it. Safe to call any time: returns null when
  // nothing is queued.
  async function consumePendingOpenPath() {
    try {
      const path = await backend.app.takePendingOpenPath()
      if (path) await openFolder(path)
    } catch (error) {
      logClientError('pending open-path failed', { error })
    }
  }

  // Recent folders and the workbench restore are independent and load in
  // parallel. The pending-open path runs last so an explicit "Open in
  // Sworm" always wins over the restored active tab and pushes into a
  // hydrated MRU.
  //
  // Provider and builtin-catalog preloads are independent of the
  // workbench boot and run in parallel from the very start so they're
  // already warm by the time the user opens a session.
  onMount(() => {
    let unlisten: (() => void) | undefined

    void loadProviders()
    void preloadBuiltinCatalog().catch((error) => {
      logClientError('builtin catalog preload failed', { error })
    })

    void (async () => {
      try {
        await Promise.all([loadRecentFolders(), restoreWorkbench()])
        await consumePendingOpenPath()
        bootstrapping = false
      } catch (error) {
        bootstrapError = describeClientError(error)
        bootstrapping = false
        logClientError('startup bootstrap failed', {
          phase: '+page onMount',
          error,
          detail: bootstrapError
        })
      }
    })()

    // Second-instance launches fire this from the Rust single-instance
    // callback. The payload is empty; the backend has already stashed
    // the path in the pending slot.
    void listen('sworm://pending-open-changed', () => {
      void consumePendingOpenPath()
    }).then((fn) => {
      unlisten = fn
    })

    return () => unlisten?.()
  })
</script>

{#if bootstrapError}
  <div class="flex min-h-0 flex-1 items-center justify-center bg-ground p-6">
    <div class="max-w-3xl rounded-xl border border-danger-border bg-danger-bg/30 p-4 text-left">
      <h2 class="mb-2 text-sm font-semibold text-danger-bright">Startup failed</h2>
      <p class="mb-3 text-base text-danger">
        Open the devtools console and look for the logged `[sworm] startup bootstrap failed` details.
      </p>
      <pre class="overflow-auto text-sm whitespace-pre-wrap text-fg">{bootstrapError}</pre>
    </div>
  </div>
{:else if bootstrapping}
  <div class="flex min-h-0 flex-1 items-center justify-center bg-ground text-base text-muted">
    Restoring workspace...
  </div>
{:else}
  <WorkbenchView />
{/if}
