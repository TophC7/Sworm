<!--
  @component
  WorkbenchHome — Home when this window has no tabs. Continue lists
  server workbenches on desktop and other workbenches in the browser.
-->

<script lang="ts">
  import { onMount } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { confirmCloseWorkbench, openFolderPicker } from '$lib/features/app-actions/actions.svelte'
  import { setFolderSwitcherOpen } from '$lib/features/folders/switcher.svelte'
  import { getErrorMessage, runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
  import { openWorkbench } from '$lib/features/workbench/groups.svelte'
  import { openFolder } from '$lib/features/workbench/state.svelte'
  import { FolderOpen } from '$lib/icons/lucideExports'
  import { platform } from '$lib/platform'
  import type { WorkbenchInfo } from '$lib/types/backend'
  import { logClientError } from '$lib/utils/client-error'
  import Home from './Home.svelte'

  let sections = $state<{ server?: string; workbenches: WorkbenchInfo[] }[]>([])
  const closing = new SvelteSet<string>()
  const closeErrors = $state<Record<string, string>>({})
  let listSeq = 0
  let refreshing = false
  let queued = false
  let disposed = false

  // An empty workbench from another controller is noise.
  const listable = (workbench: WorkbenchInfo) =>
    !workbench.yours &&
    workbench.id !== platform.workbench.id &&
    (workbench.folders.length > 0 || workbench.running.length > 0)

  async function refresh(): Promise<void> {
    if (disposed) return
    if (refreshing) {
      queued = true
      listSeq++
      return
    }
    refreshing = true
    do {
      queued = false
      const seq = ++listSeq
      try {
        if (platform.capabilities.remoteHosts) {
          const { settings } = await backend.settings.getEffective()
          const results = await Promise.all(
            Object.keys(settings.remotes).map(async (server) => {
              try {
                return { server, workbenches: (await backend.workbenches.list(server)).filter(listable) }
              } catch (error) {
                logClientError(`Failed to list workbenches on ${server}`, { error })
                return { server, workbenches: [] }
              }
            })
          )
          if (!disposed && seq === listSeq) sections = results
        } else {
          const workbenches = (await backend.workbenches.list()).filter(listable)
          if (!disposed && seq === listSeq) sections = [{ workbenches }]
        }
      } catch (error) {
        logClientError('Failed to list workbenches', { error })
      }
    } while (queued && !disposed)
    refreshing = false
  }

  async function close(workbench: WorkbenchInfo, server?: string): Promise<void> {
    const key = `${server ?? ''}:${workbench.id}`
    if (closing.has(key)) return
    delete closeErrors[key]
    const confirmed = await confirmCloseWorkbench(workbench.folders)
    if (!confirmed || closing.has(key)) return
    closing.add(key)
    try {
      await backend.workbenches.close(workbench.id, server)
    } catch (error) {
      closeErrors[key] = getErrorMessage(error)
    } finally {
      closing.delete(key)
    }
    void refresh()
  }

  onMount(() => {
    if (!platform.capabilities.durableWorkbenches && !platform.capabilities.remoteHosts) return
    disposed = false
    const listeners = [
      backend.workbenches.onChanged(() => {
        if (!disposed) void refresh()
      }),
      ...(platform.capabilities.remoteHosts
        ? [
            backend.settings.onChanged(({ layer }) => {
              if (!disposed && layer === 'global') void refresh()
            })
          ]
        : [])
    ]
    void refresh()
    const onFocus = () => void refresh()
    window.addEventListener('focus', onFocus)
    return () => {
      disposed = true
      listSeq++
      for (const listener of listeners) void listener.then((stop) => stop()).catch(() => {})
      window.removeEventListener('focus', onFocus)
    }
  })
</script>

<Home
  onOpenProject={(path) => void openFolder(path)}
  {sections}
  {closing}
  {closeErrors}
  onOpenWorkbench={(server, workbench) =>
    void runNotifiedTask(() => openWorkbench(server, workbench.id, false), {
      loading: { title: 'Opening workbench' },
      error: { title: 'Open workbench failed' }
    })}
  onTakeOverWorkbench={(workbench, server) => {
    void runNotifiedTask(
      async () => {
        if (server) return openWorkbench(server, workbench.id, true)
        const takeOver = platform.workbench.takeOver
        if (!takeOver) throw new Error('Taking over a workbench is not supported here')
        await takeOver(workbench.id)
      },
      {
        loading: { title: 'Taking over workbench', description: server },
        error: { title: 'Take over failed' }
      }
    )
  }}
  onCloseWorkbench={(workbench, server) => void close(workbench, server)}
>
  {#snippet actions()}
    <Button
      size="sm"
      onclick={() =>
        platform.capabilities.nativeDirectoryPicker ? void openFolderPicker() : setFolderSwitcherOpen(true)}
    >
      <FolderOpen size={14} />
      Open Folder
    </Button>
  {/snippet}
</Home>
