<!--
  @component
  IssueSurface; thin shell that loads detail by id and mounts
  IssueDetailForm under {#key tab.issueId} so the form re-seeds via
  $state initializers (no $effect mirroring) when the active id changes.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { IconButton } from '$lib/components/ui/button'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { CircleDot, RefreshCwIcon } from '$lib/icons/lucideExports'
  import { getIssueDetail, openIssueDetail } from '$lib/features/issues/state.svelte'
  import IssueDetailForm from './IssueDetailForm.svelte'
  import type { IssueTab } from '$lib/features/workbench/model'

  let { tab, projectId }: { tab: IssueTab; projectId: string } = $props()

  let detail = $derived(getIssueDetail(projectId, tab.issueId))

  // Real Tauri side effect: load the detail row when the tab id changes.
  // Tracked read of tab.issueId only; the actual load runs outside the
  // reactive scope so it doesn't pull other dependencies.
  $effect(() => {
    const id = tab.issueId
    untrack(() => void openIssueDetail(projectId, id))
  })

  async function refresh() {
    await openIssueDetail(projectId, tab.issueId)
  }
</script>

<section class="flex h-full flex-col bg-ground">
  <PanelHeader>
    {#snippet left()}
      <CircleDot size={13} class="text-accent" />
      <span class="font-mono text-2xs text-muted">{tab.issueId}</span>
      <span class="max-w-[36ch] truncate text-xs text-fg">{detail?.issue.title ?? tab.title}</span>
    {/snippet}
    {#snippet right()}
      <IconButton tooltip="Refresh" onclick={refresh}>
        <RefreshCwIcon size={11} />
      </IconButton>
    {/snippet}
  </PanelHeader>

  {#if !detail}
    <div class="flex flex-1 items-center justify-center text-sm text-subtle">Loading issue…</div>
  {:else}
    {#key detail.issue.id}
      <IssueDetailForm {detail} {projectId} {tab} />
    {/key}
  {/if}
</section>
