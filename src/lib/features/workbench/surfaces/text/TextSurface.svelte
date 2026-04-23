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

  type RenderedTextTab = Pick<TextTab, 'id' | 'filePath' | 'gitRef' | 'refLabel'>

  function snapshotTab(tab: TextTab): RenderedTextTab {
    return {
      id: tab.id,
      filePath: tab.filePath,
      gitRef: tab.gitRef,
      refLabel: tab.refLabel
    }
  }

  // Keep the last concrete tab props around until this branch unmounts.
  // Without this, a split-pane close can null out `tab` before the keyed
  // FileEditor subtree finishes tearing down, and Svelte ends up reading
  // `tab.id` during destruction.
  let renderedTab = $state<RenderedTextTab | null>(null)

  $effect.pre(() => {
    if (!tab) return
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
      {locked}
    />
  {/key}
{/if}
