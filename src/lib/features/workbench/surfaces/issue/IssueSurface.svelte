<!--
  @component
  IssueSurface; thin shell that loads detail by id and mounts
  IssueDetailForm under {#key tab.issueId} so the form re-seeds via
  $state initializers (no $effect mirroring) when the active id changes.
  Renders the Epic > Parent > Issue breadcrumb in the panel header.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { IconButton } from '$lib/components/ui/button'
  import {
    BreadcrumbItem,
    BreadcrumbList,
    BreadcrumbPage,
    BreadcrumbRoot,
    BreadcrumbSeparator
  } from '$lib/components/ui/breadcrumb'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { Layers, RefreshCwIcon } from '$lib/icons/lucideExports'
  import { getIssueDetail, getIssueEpics, getIssues, openIssueDetail } from '$lib/features/issues/state.svelte'
  import { openIssueTab } from '$lib/features/workbench/surfaces/issue/service.svelte'
  import IssueDetailForm from './IssueDetailForm.svelte'
  import type { IssueTab } from '$lib/features/workbench/model'

  let { tab, projectId }: { tab: IssueTab; projectId: string } = $props()

  let detail = $derived(getIssueDetail(projectId, tab.issueId))
  let allIssues = $derived(getIssues(projectId))
  let allEpics = $derived(getIssueEpics(projectId))
  let parentIssue = $derived(
    detail?.issue.parentIssueId ? (allIssues.find((i) => i.id === detail!.issue.parentIssueId) ?? null) : null
  )
  let epicRef = $derived(detail?.issue.epicId ? (allEpics.find((e) => e.id === detail!.issue.epicId) ?? null) : null)

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
      <BreadcrumbRoot>
        <BreadcrumbList class="text-2xs">
          {#if epicRef}
            <BreadcrumbItem class="font-mono text-warning" title={epicRef.title}>
              <Layers size={11} class="shrink-0" />
              <span>{epicRef.id}</span>
              <span class="max-w-[20ch] truncate font-sans text-subtle">{epicRef.title}</span>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
          {/if}
          {#if parentIssue}
            <BreadcrumbItem class="font-mono">
              <button
                type="button"
                class="flex items-center gap-1 text-accent hover:underline focus-visible:shadow-focus-ring focus-visible:outline-none"
                onclick={() => openIssueTab(projectId, parentIssue!.id, parentIssue!.title)}
                title={parentIssue.title}
              >
                <span>{parentIssue.id}</span>
                <span class="max-w-[20ch] truncate font-sans text-subtle">{parentIssue.title}</span>
              </button>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
          {/if}
          <BreadcrumbItem>
            <BreadcrumbPage class="flex items-center gap-1.5 font-mono">
              <span class="text-muted">{tab.issueId}</span>
              <span class="max-w-[36ch] truncate font-sans text-fg">
                {detail?.issue.title ?? tab.title}
              </span>
            </BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </BreadcrumbRoot>
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
