<script lang="ts">
  import FileEditor from '$lib/features/workbench/surfaces/text/FileEditor.svelte'
  import type { TextTab } from '$lib/features/workbench/model'

  let {
    tab = null,
    locked = false,
    projectId,
    projectPath
  }: {
    // SurfaceHost can clear the active tab before this branch fully tears
    // down, so tolerate a transient null during close.
    tab: TextTab | null
    locked?: boolean
    projectId: string
    projectPath: string
  } = $props()

  interface RenderedTextTab extends Pick<TextTab, 'id' | 'filePath' | 'gitRef' | 'refLabel'> {
    initialTemporary: boolean
  }

  function snapshotTab(tab: TextTab): RenderedTextTab {
    return {
      id: tab.id,
      filePath: tab.filePath,
      gitRef: tab.gitRef,
      refLabel: tab.refLabel,
      initialTemporary: tab.temporary
    }
  }

  function renderedTabChanged(current: RenderedTextTab, next: TextTab): boolean {
    return current.filePath !== next.filePath || current.gitRef !== next.gitRef || current.refLabel !== next.refLabel
  }

  // Keep the last concrete tab props around until this branch unmounts.
  // Without this, a split-pane close can null out `tab` before the keyed
  // FileEditor subtree finishes tearing down, and Svelte ends up reading
  // `tab.id` during destruction.
  let renderedTab = $state<RenderedTextTab | null>(null)

  $effect.pre(() => {
    if (!tab) return
    if (renderedTab?.id === tab.id && !renderedTabChanged(renderedTab, tab)) return
    renderedTab = snapshotTab(tab)
  })
</script>

{#if renderedTab}
  {#key renderedTab.id}
    <FileEditor
      tabId={renderedTab.id}
      filePath={renderedTab.filePath}
      {projectPath}
      {projectId}
      gitRef={renderedTab.gitRef}
      refLabel={renderedTab.refLabel}
      initialTemporary={renderedTab.initialTemporary}
      {locked}
    />
  {/key}
{/if}
