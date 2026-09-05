<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { backend } from '$lib/api/backend'
  import { preloadBuiltinCatalog } from '$lib/features/builtins/catalog'
  import WorkbenchView from '$lib/features/app-shell/WorkbenchView.svelte'
  import { loadRecentFolders } from '$lib/features/folders/state.svelte'
  import { loadProviders } from '$lib/features/sessions/providers/state.svelte'
  import { openFolder, restoreWorkbench, setWindowLabel } from '$lib/features/workbench/state.svelte'
  import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
  import type { OpenTarget } from '$lib/types/backend'
  import { describeClientError, logClientError } from '$lib/utils/client-error'

  let bootstrapping = $state(true)
  let bootstrapError = $state<string | null>(null)

  async function openTarget(target: OpenTarget): Promise<void> {
    if (target.type === 'folder') await openFolder(target.folder_path)
    else await openTextFile(target.folder_path, target.file_path)
  }

  onMount(() => {
    const currentWindow = getCurrentWindow()
    const label = currentWindow.label
    setWindowLabel(label)
    let disposed = false
    let unlisten: (() => void) | undefined

    void loadProviders()
    void preloadBuiltinCatalog().catch((error) => {
      logClientError('builtin catalog preload failed', { error })
    })

    void (async () => {
      try {
        const cleanup = await currentWindow.listen<OpenTarget>('sworm://open-target', ({ payload }) => {
          void openTarget(payload).catch((error) => logClientError('open target failed', { error, payload }))
        })
        if (disposed) cleanup()
        else unlisten = cleanup

        await Promise.all([loadRecentFolders(), restoreWorkbench(label)])
        await backend.window.ready()
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

    return () => {
      disposed = true
      unlisten?.()
    }
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
