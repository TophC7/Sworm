<script lang="ts">
  import FileEditor from '$lib/surfaces/text/FileEditor.svelte'
  import type { TextTab } from '$lib/workbench/state.svelte'

  let {
    tab,
    projectId,
    projectPath
  }: {
    tab: TextTab
    projectId: string
    projectPath: string
  } = $props()
</script>

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
