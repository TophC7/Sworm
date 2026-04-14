<!--
  @component
  Unified diff pane.

  Each diff takes its natural height — vertical scrolling is handled
  by the parent (DiffStack). Horizontal scroll for long lines.
-->

<script lang="ts">
  import { DiffMode, type DiffFile, type DiffRow } from '$lib/diff/types'
  import { gutterWidth as calcGutterWidth, LINE_HEIGHT_RATIO } from '$lib/diff/layout'
  import { useDiffScroll } from '$lib/diff/scrollContext.svelte'
  import { createPaneVirtualizer } from '$lib/diff/virtualList.svelte'
  import DiffUnifiedRow from './DiffUnifiedRow.svelte'
  import DiffHunkRow from './DiffHunkRow.svelte'

  let {
    diffFile,
    rows,
    wrap,
    fontSize
  }: {
    diffFile: DiffFile
    rows: DiffRow[]
    wrap: boolean
    fontSize: number
  } = $props()

  let gutterWidth = $derived(calcGutterWidth(Math.max(diffFile.splitLineLength, diffFile.unifiedLineLength)))

  const scrollCtx = useDiffScroll()
  let paneEl = $state<HTMLElement | null>(null)

  const virt = createPaneVirtualizer({
    getRows: () => rows,
    getRowHeight: () => fontSize * LINE_HEIGHT_RATIO,
    getScrollCtx: () => scrollCtx,
    getPaneEl: () => paneEl
  })
</script>

<div class="overflow-x-auto overflow-y-hidden font-mono" style="overscroll-behavior-x: none;" bind:this={paneEl}>
  <div style="min-width: fit-content;">
    {#if virt.enabled}
      <div style="height: {virt.spacerBefore}px"></div>
      {#each virt.visibleRows as row, i (virt.startIndex + i)}
        {#if row.kind === 'hunk'}
          <DiffHunkRow {diffFile} index={row.index} mode={DiffMode.Unified} {fontSize} />
        {:else}
          <DiffUnifiedRow {diffFile} index={row.index} {fontSize} {wrap} {gutterWidth} />
        {/if}
      {/each}
      <div style="height: {virt.spacerAfter}px"></div>
    {:else}
      {#each rows as row, i (i)}
        {#if row.kind === 'hunk'}
          <DiffHunkRow {diffFile} index={row.index} mode={DiffMode.Unified} {fontSize} />
        {:else}
          <DiffUnifiedRow {diffFile} index={row.index} {fontSize} {wrap} {gutterWidth} />
        {/if}
      {/each}
    {/if}
  </div>
</div>
