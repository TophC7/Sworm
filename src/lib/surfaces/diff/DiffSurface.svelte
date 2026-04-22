<script lang="ts">
  import WorkingDiffView from '$lib/surfaces/diff/WorkingDiffView.svelte'
  import CommitDiffView from '$lib/surfaces/diff/CommitDiffView.svelte'
  import StashDiffView from '$lib/surfaces/diff/StashDiffView.svelte'
  import type { DiffTab } from '$lib/workbench/state.svelte'

  let {
    tab,
    projectId,
    projectPath
  }: {
    tab: DiffTab
    projectId: string
    projectPath: string
  } = $props()
</script>

{#if tab.source.kind === 'commit'}
  <CommitDiffView commitHash={tab.source.commitHash} {projectId} {projectPath} initialFile={tab.initialFile} />
{:else if tab.source.kind === 'working'}
  <WorkingDiffView
    {projectId}
    {projectPath}
    staged={tab.source.staged}
    scopePath={tab.source.scopePath}
    initialFile={tab.initialFile}
    revealNonce={tab.source.revealNonce}
  />
{:else}
  <StashDiffView stashIndex={tab.source.stashIndex} {projectId} {projectPath} initialFile={tab.initialFile} />
{/if}
