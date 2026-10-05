import { invalidateLspServerEntries, restartLspServerDefinition } from '$lib/features/editor/lsp/registry'
import { markProjectFilesStale } from '$lib/features/files/projectFiles.svelte'
import { loadRecentFolders } from '$lib/features/folders/state.svelte'
import { ensureGitWatch } from '$lib/features/git/state.svelte'
import { refreshIssuesForFolder } from '$lib/features/issues/state.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/utils/client-error'
import { loadProviders, loadProvidersForFolder } from '$lib/features/sessions/providers/state.svelte'
import { detectNix, getNixDetection } from '$lib/features/settings/state/nix.svelte'
import { loadSettings } from '$lib/features/settings/state/settings.svelte'
import { refreshTasks } from '$lib/features/tasks/state.svelte'
import { flushWorkbench, getTabs } from '$lib/features/workbench/state.svelte'
import { logClientError } from '$lib/utils/client-error'

export function createWebRecovery() {
  const lostLspDefinitions = new Map<string, number>()
  let generation = 0
  let lossRevision = 0
  let connected = true
  let recoveringLsp = false
  let retryDelay = 1000
  let lspTimer: number | undefined
  let disposed = false

  function onDisconnected(): void {
    generation++
    connected = false
    window.clearTimeout(lspTimer)
    lspTimer = undefined
  }

  function onLspDisconnected(serverDefinitionId: string): void {
    if (disposed) return
    lostLspDefinitions.set(serverDefinitionId, ++lossRevision)
    scheduleLspRecovery()
  }

  function scheduleLspRecovery(): void {
    if (disposed || !connected || recoveringLsp || lspTimer !== undefined || !lostLspDefinitions.size) return
    // The exit event must finish recording registry teardown before a replacement starts.
    lspTimer = window.setTimeout(() => {
      lspTimer = undefined
      void recoverLsp()
    }, retryDelay)
  }

  async function recoverLsp(): Promise<void> {
    if (disposed || !connected || recoveringLsp) return
    recoveringLsp = true
    const current = generation
    const active = () => !disposed && connected && generation === current
    try {
      await Promise.all(
        [...lostLspDefinitions].map(async ([id, revision]) => {
          if (!active()) return
          try {
            await restartLspServerDefinition(id, active)
            if (active() && lostLspDefinitions.get(id) === revision) lostLspDefinitions.delete(id)
          } catch (error) {
            if (active()) logClientError('web LSP recovery failed', { serverDefinitionId: id, error })
          }
        })
      )
    } finally {
      recoveringLsp = false
      retryDelay = lostLspDefinitions.size ? Math.min(retryDelay * 2, 30_000) : 1000
      scheduleLspRecovery()
    }
  }

  async function onReconnected(): Promise<void> {
    if (disposed) return
    connected = true
    scheduleLspRecovery()
    const current = ++generation
    const active = () => !disposed && current === generation
    const folders = [...new Set(getTabs().map((tab) => tab.folderPath))]
    const jobs: { label: string; run: () => Promise<unknown> | void }[] = [
      { label: 'settings', run: () => loadSettings() },
      { label: 'recent folders', run: () => loadRecentFolders() },
      { label: 'workbench layout', run: () => flushWorkbench() }
    ]

    // Provider discovery is opportunistic and logs failures; the cache reset is synchronous.
    void loadProviders().catch(() => {})
    invalidateLspServerEntries()

    for (const folder of folders) {
      const open = () => active() && getTabs().some((tab) => tab.folderPath === folder)
      jobs.push(
        { label: `Git for ${folder}`, run: () => (open() ? ensureGitWatch(folder, 'all') : undefined) },
        {
          label: `files for ${folder}`,
          run: () => {
            if (open()) markProjectFilesStale(folder)
          }
        },
        { label: `tasks for ${folder}`, run: () => (open() ? refreshTasks(folder) : undefined) },
        {
          label: `environment and providers for ${folder}`,
          run: async () => {
            if (!open()) return
            if (getNixDetection(folder)) await detectNix(folder)
            if (open()) await loadProvidersForFolder(folder, open)
          }
        },
        {
          label: `issues for ${folder}`,
          run: () => (open() ? refreshIssuesForFolder(folder) : undefined)
        }
      )
    }

    const results = await Promise.allSettled(jobs.map(({ run }) => Promise.resolve().then(() => active() && run())))
    if (!active()) return
    for (let index = 0; index < results.length; index++) {
      const result = results[index]
      if (result.status !== 'rejected') continue
      logClientError('web recovery failed', { operation: jobs[index].label, error: result.reason })
      notify.error(`Could not refresh ${jobs[index].label}`, getErrorMessage(result.reason))
    }
  }

  function dispose(): void {
    disposed = true
    generation++
    window.clearTimeout(lspTimer)
    lostLspDefinitions.clear()
  }

  return { onDisconnected, onLspDisconnected, onReconnected, dispose }
}
