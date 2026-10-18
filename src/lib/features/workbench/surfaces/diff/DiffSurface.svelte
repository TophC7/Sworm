<script lang="ts">
  import WorkingDiffView from '$lib/features/workbench/surfaces/diff/WorkingDiffView.svelte'
  import CommitDiffView from '$lib/features/workbench/surfaces/diff/CommitDiffView.svelte'
  import StashDiffView from '$lib/features/workbench/surfaces/diff/StashDiffView.svelte'
  import RangeDiffView from '$lib/features/workbench/surfaces/diff/RangeDiffView.svelte'
  import CompareDiffView from '$lib/features/workbench/surfaces/diff/CompareDiffView.svelte'
  import type { DiffTab } from '$lib/features/workbench/model'

  let {
    tab = null,
    folderPath
  }: {
    tab: DiffTab | null
    folderPath: string
  } = $props()
</script>

{#if tab?.source.kind === 'commit'}
  <CommitDiffView commitHash={tab.source.commitHash} {folderPath} initialFile={tab.initialFile} />
{:else if tab?.source.kind === 'working'}
  <WorkingDiffView
    {folderPath}
    staged={tab.source.staged}
    scopePath={tab.source.scopePath}
    initialFile={tab.initialFile}
    revealNonce={tab.source.revealNonce}
  />
{:else if tab?.source.kind === 'stash'}
  <StashDiffView stashIndex={tab.source.stashIndex} {folderPath} initialFile={tab.initialFile} />
{:else if tab?.source.kind === 'range'}
  <RangeDiffView
    {folderPath}
    base={tab.source.base}
    baseLabel={tab.source.baseLabel}
    target={tab.source.target}
    targetLabel={tab.source.targetLabel}
    initialFile={tab.initialFile}
  />
{:else if tab?.source.kind === 'compare'}
  <CompareDiffView {folderPath} left={tab.source.left} right={tab.source.right} />
{/if}
