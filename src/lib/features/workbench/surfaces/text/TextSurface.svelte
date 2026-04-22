<script lang="ts">
  import FileEditor from '$lib/features/workbench/surfaces/text/FileEditor.svelte'
  import type { TextTab } from '$lib/features/workbench/state.svelte'

  let {
    tab = null,
    projectId,
    projectPath
  }: {
    // SurfaceHost can clear the active tab before this branch fully tears
    // down, so tolerate a transient null during close.
    tab: TextTab | null
    projectId: string
    projectPath: string
  } = $props()
</script>

{#if tab}
  {#key tab.id}
    <FileEditor
      tabId={tab.id}
      filePath={tab.filePath}
      {projectPath}
      {projectId}
      gitRef={tab.gitRef}
      refLabel={tab.refLabel}
    />
  {/key}
{/if}
