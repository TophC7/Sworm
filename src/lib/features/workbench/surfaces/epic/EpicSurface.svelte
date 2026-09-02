<!--
  @component
  EpicSurface; thin shell that loads detail by id and mounts
  EpicDetailForm under {#key tab.epicId}.
-->

<script lang="ts">
  import { untrack } from 'svelte'
  import { IconButton } from '$lib/components/ui/button'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { Layers, RefreshCwIcon } from '$lib/icons/lucideExports'
  import { getEpicDetail, openEpicDetail } from '$lib/features/issues/state.svelte'
  import EpicDetailForm from './EpicDetailForm.svelte'
  import type { EpicTab } from '$lib/features/workbench/model'

  let { tab, folderPath }: { tab: EpicTab; folderPath: string } = $props()

  let detail = $derived(getEpicDetail(folderPath, tab.epicId))

  $effect(() => {
    const id = tab.epicId
    untrack(() => void openEpicDetail(folderPath, id))
  })

  async function refresh() {
    await openEpicDetail(folderPath, tab.epicId)
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

  {#if !detail}
    <div class="flex flex-1 items-center justify-center text-sm text-subtle">Loading epic…</div>
  {:else}
    {#key detail.id}
      <EpicDetailForm {detail} {folderPath} {tab} />
    {/key}
  {/if}
</section>
