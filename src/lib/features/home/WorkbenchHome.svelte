<!--
  @component
  WorkbenchHome — Home inside a workbench with no tabs. Projects open here.
  With durable workbenches (web), Continue lists the others worth returning
  to; switching away leaves this empty one to be pruned by the server.
-->

<script lang="ts">
  import { onMount } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { openFolderPicker } from '$lib/features/app-actions/actions.svelte'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import { setFolderSwitcherOpen } from '$lib/features/folders/switcher.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { openFolder } from '$lib/features/workbench/state.svelte'
  import { FolderOpen } from '$lib/icons/lucideExports'
  import { platform } from '$lib/platform'
  import type { WorkbenchInfo } from '$lib/types/backend'
  import { basename } from '$lib/utils/paths'
  import { logClientError } from '$lib/utils/client-error'
  import Home from './Home.svelte'

  const POLL_MS = 5000

  let listed = $state<WorkbenchInfo[] | null>(null)
  const closing = new SvelteSet<string>()
  const closeErrors = $state<Record<string, string>>({})
  let listSeq = 0
  let listedAt = 0

  // Only workbenches with something left in them; another page's empty one is noise.
  let others = $derived(
    listed?.filter(
      (workbench) =>
        workbench.id !== platform.workbench.id && (workbench.folders.length > 0 || workbench.running.length > 0)
    ) ?? null
  )

  // Latest request wins, so a poll that started before a Close cannot resurrect its row.
  async function refresh(): Promise<void> {
    const seq = ++listSeq
    listedAt = Date.now()
    try {
      const next = await backend.workbenches.list()
      if (seq === listSeq) listed = next
    } catch (error) {
      logClientError('Failed to list workbenches', { error })
    }
  }

  async function close(workbench: WorkbenchInfo): Promise<void> {
    if (closing.has(workbench.id)) return
    delete closeErrors[workbench.id]
    const confirmed = await confirmAsync({
      title: 'Close Workbench',
      message: `Stop all sessions and tasks in ${workbench.folders.map(basename).join(', ') || 'this workbench'} and delete its saved tabs and layout? Unsaved changes in any connected page will be lost.`,
      confirmLabel: 'Close Workbench',
      cancelLabel: 'Cancel'
    })
    if (!confirmed || closing.has(workbench.id)) return
    closing.add(workbench.id)
    try {
      await backend.workbenches.close(workbench.id)
    } catch (error) {
      closeErrors[workbench.id] = getErrorMessage(error)
    } finally {
      closing.delete(workbench.id)
    }
    void refresh()
  }

  onMount(() => {
    if (!platform.capabilities.durableWorkbenches) return
    void refresh()
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') void refresh()
    }, POLL_MS)
    // Focus right after a poll would only repeat it.
    const onFocus = () => {
      if (Date.now() - listedAt >= POLL_MS) void refresh()
    }
    window.addEventListener('focus', onFocus)
    return () => {
      window.clearInterval(timer)
      window.removeEventListener('focus', onFocus)
    }
  })
</script>

<Home
  onOpenProject={(path) => void openFolder(path)}
  workbenches={others}
  {closing}
  {closeErrors}
  onCloseWorkbench={(workbench) => void close(workbench)}
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
