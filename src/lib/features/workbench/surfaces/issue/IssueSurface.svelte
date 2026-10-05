<!--
  @component
  IssueSurface; thin shell that loads detail by id and mounts
  IssueDetailForm under {#key tab.issueId} so the form re-seeds via
  $state initializers (no $effect mirroring) when the active id changes.
  Renders the Epic > Parent > Issue breadcrumb in the panel header.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { Button, IconButton } from '$lib/components/ui/button'
  import {
    Breadcrumb,
    BreadcrumbItem,
    BreadcrumbPage,
    BreadcrumbSeparator
  } from '$lib/components/ui/breadcrumb'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { Layers, RefreshCwIcon } from '$lib/icons/lucideExports'
  import { getIssueDetail, getIssueEpics, getIssues, reloadIssueDetail, watchIssueDetail } from '$lib/features/issues/state.svelte'
  import { openIssueTab } from '$lib/features/workbench/surfaces/issue/service.svelte'
  import IssueDetailForm from './IssueDetailForm.svelte'
  import type { IssueTab } from '$lib/features/workbench/model'

  let { tab, folderPath }: { tab: IssueTab; folderPath: string } = $props()

  let slot = $derived(getIssueDetail(folderPath, tab.issueId))
  let detail = $derived(slot?.value ?? null)
  let allIssues = $derived(getIssues(folderPath))
  let allEpics = $derived(getIssueEpics(folderPath))
  let parentIssue = $derived(
    detail?.issue.parentIssueId ? (allIssues.find((i) => i.id === detail!.issue.parentIssueId) ?? null) : null
  )
  let epicRef = $derived(detail?.issue.epicId ? (allEpics.find((e) => e.id === detail!.issue.epicId) ?? null) : null)

  $effect(() => {
    const id = tab.issueId
    const folder = folderPath
    return untrack(() => watchIssueDetail(folder, id))
  })

  async function refresh() {
    await reloadIssueDetail(folderPath, tab.issueId)
  }
</script>

<section class="flex h-full flex-col bg-ground">
  <PanelHeader>
    {#snippet left()}
      <Breadcrumb class="text-2xs">
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
                onclick={() => openIssueTab(folderPath, parentIssue!.id, parentIssue!.title)}
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
      </Breadcrumb>
    {/snippet}
    {#snippet right()}
      <IconButton tooltip="Refresh" onclick={refresh}>
        <RefreshCwIcon size={11} />
      </IconButton>
    {/snippet}
  </PanelHeader>

  {#if slot?.error}
    <div role="alert" class="flex items-center justify-between gap-3 border-b border-danger-border bg-danger-bg px-3 py-2 text-sm text-danger">
      <span>{slot.error}</span>
      <Button size="sm" onclick={refresh}>Refresh</Button>
    </div>
  {/if}
  {#if !detail}
    {#if !slot?.error}
      <div class="flex flex-1 items-center justify-center text-sm text-subtle">Loading issue…</div>
    {/if}
  {:else}
    {#key detail.issue.id}
      <IssueDetailForm {detail} {folderPath} {tab} {parentIssue} {epicRef} />
    {/key}
  {/if}
</section>
