<script lang="ts">
  import WorkingDiffView from '$lib/features/workbench/surfaces/diff/WorkingDiffView.svelte'
  import CommitDiffView from '$lib/features/workbench/surfaces/diff/CommitDiffView.svelte'
  import StashDiffView from '$lib/features/workbench/surfaces/diff/StashDiffView.svelte'
  import type { DiffTab } from '$lib/features/workbench/model'

  let {
    tab = null,
    projectId,
    projectPath
  }: {
    tab: DiffTab | null
    projectId: string
    projectPath: string
  } = $props()
</script>

{#if tab?.source.kind === 'commit'}
  <CommitDiffView commitHash={tab.source.commitHash} {projectId} {projectPath} initialFile={tab.initialFile} />
{:else if tab?.source.kind === 'working'}
  <WorkingDiffView
    {projectId}
    {projectPath}
    staged={tab.source.staged}
    scopePath={tab.source.scopePath}
    initialFile={tab.initialFile}
    revealNonce={tab.source.revealNonce}
  />
{:else if tab?.source.kind === 'stash'}
  <StashDiffView stashIndex={tab.source.stashIndex} {projectId} {projectPath} initialFile={tab.initialFile} />
{/if}
