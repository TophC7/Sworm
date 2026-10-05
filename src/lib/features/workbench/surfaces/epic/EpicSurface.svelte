<!--
  @component
  EpicSurface; thin shell that loads detail by id and mounts
  EpicDetailForm under {#key tab.epicId}.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { Button, IconButton } from '$lib/components/ui/button'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { Layers, RefreshCwIcon } from '$lib/icons/lucideExports'
  import { getEpicDetail, reloadEpicDetail, watchEpicDetail } from '$lib/features/issues/state.svelte'
  import EpicDetailForm from './EpicDetailForm.svelte'
  import type { EpicTab } from '$lib/features/workbench/model'

  let { tab, folderPath }: { tab: EpicTab; folderPath: string } = $props()

  let slot = $derived(getEpicDetail(folderPath, tab.epicId))
  let detail = $derived(slot?.value ?? null)

  $effect(() => {
    const id = tab.epicId
    const folder = folderPath
    return untrack(() => watchEpicDetail(folder, id))
  })

  async function refresh() {
    await reloadEpicDetail(folderPath, tab.epicId)
  }
</script>

<section class="flex h-full flex-col bg-ground">
  <PanelHeader>
    {#snippet left()}
      <Layers size={13} class="text-warning" />
      <span class="font-mono text-2xs text-muted">{tab.epicId}</span>
      <span class="max-w-[36ch] truncate text-xs text-fg">{detail?.title ?? tab.title}</span>
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
      <div class="flex flex-1 items-center justify-center text-sm text-subtle">Loading epic…</div>
    {/if}
  {:else}
    {#key detail.id}
      <EpicDetailForm {detail} {folderPath} {tab} />
    {/key}
  {/if}
</section>
