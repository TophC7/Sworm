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
  import { confirmCloseWorkbench } from '$lib/features/app-actions/actions.svelte'
  import { goToFolder, goToWorkbench, openBrowser } from '$lib/features/browser/state.svelte'
  import { getWorkbenchSections, refreshPlaces, watchPlaces } from '$lib/features/browser/places.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { FolderOpen } from '$lib/icons/lucideExports'
  import type { WorkbenchInfo } from '$lib/types/backend'
  import Home from './Home.svelte'

  let sections = $derived(getWorkbenchSections())
  const closing = new SvelteSet<string>()
  const closeErrors = $state<Record<string, string>>({})

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
    void refreshPlaces()
  }

  onMount(watchPlaces)
</script>

<Home
  onOpenProject={(path) => void goToFolder(path)}
  {sections}
  {closing}
  {closeErrors}
  onOpenWorkbench={(server, workbench) => void goToWorkbench(workbench, server)}
  onTakeOverWorkbench={(workbench, server) => void goToWorkbench(workbench, server, true)}
  onCloseWorkbench={(workbench, server) => void close(workbench, server)}
>
  {#snippet actions()}
    <Button size="sm" onclick={() => openBrowser()}>
      <FolderOpen size={14} />
      Open Folder
    </Button>
  {/snippet}
</Home>
