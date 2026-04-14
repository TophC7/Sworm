<!--
  @component
  Split diff pane — two columns with independent horizontal scroll.

  Each column has its own horizontal scrollbar for long lines.
  Vertical scrolling is handled by the parent container (DiffStack).
  Each diff takes its natural height — no internal y-scroll.
-->

<script lang="ts">
  import { DiffMode, type DiffFile, type DiffRow } from '$lib/diff/types'
  import { gutterWidth as calcGutterWidth, LINE_HEIGHT_RATIO } from '$lib/diff/layout'
  import { useDiffScroll } from '$lib/diff/scrollContext.svelte'
  import { createPaneVirtualizer } from '$lib/diff/virtualList.svelte'
  import DiffSplitSideRow from './DiffSplitSideRow.svelte'
  import DiffHunkRow from './DiffHunkRow.svelte'

  const WRAP_VIRT_THRESHOLD = 500

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

  let gutterWidth = $derived(calcGutterWidth(diffFile.splitLineLength))

  const scrollCtx = useDiffScroll()
  let paneEl = $state<HTMLElement | null>(null)

  const virt = createPaneVirtualizer({
    getRows: () => rows,
    getRowHeight: () => fontSize * LINE_HEIGHT_RATIO,
    getScrollCtx: () => scrollCtx,
    getPaneEl: () => paneEl
  })

  // CSS grid wrap mode can't be virtualized (rows must be contiguous).
  // Auto-disable wrap for large diffs.
  let wrapTooLarge = $derived(wrap && rows.length > WRAP_VIRT_THRESHOLD)
  let effectiveWrap = $derived(wrap && !wrapTooLarge)
</script>

{#if wrapTooLarge}
  <div class="bg-warning/10 px-3 py-1 text-center text-[0.72rem] text-warning">
    Wrap disabled for this file ({rows.length} lines). Use unified mode for wrapped view of large diffs.
  </div>
{/if}

{#snippet splitRows(side: 'old' | 'new', wrapMode: boolean)}
  {#if virt.enabled && !wrapMode}
    <div style="height: {virt.spacerBefore}px"></div>
    {#each virt.visibleRows as row, i (virt.startIndex + i)}
      {#if row.kind === 'hunk'}
        <DiffHunkRow {diffFile} index={row.index} mode={DiffMode.Split} {fontSize} />
      {:else}
        <DiffSplitSideRow {diffFile} index={row.index} {side} {fontSize} wrap={wrapMode} {gutterWidth} />
      {/if}
    {/each}
    <div style="height: {virt.spacerAfter}px"></div>
  {:else}
    {#each rows as row, i (i)}
      {#if row.kind === 'hunk'}
        <DiffHunkRow {diffFile} index={row.index} mode={DiffMode.Split} {fontSize} />
      {:else}
        <DiffSplitSideRow {diffFile} index={row.index} {side} {fontSize} wrap={wrapMode} {gutterWidth} />
      {/if}
    {/each}
  {/if}
{/snippet}

{#if effectiveWrap}
  <!-- Wrap mode: CSS grid so each row's height = max(left, right). Not virtualized. -->
  <div class="grid font-mono" style="grid-template-columns: minmax(0, 1fr) 1px minmax(0, 1fr);">
    {#each rows as row, i (i)}
      {#if row.kind === 'hunk'}
        <DiffHunkRow {diffFile} index={row.index} mode={DiffMode.Split} {fontSize} />
        <div class="bg-edge"></div>
        <DiffHunkRow {diffFile} index={row.index} mode={DiffMode.Split} {fontSize} />
      {:else}
        <DiffSplitSideRow {diffFile} index={row.index} side="old" {fontSize} wrap={true} {gutterWidth} />
        <div class="bg-edge"></div>
        <DiffSplitSideRow {diffFile} index={row.index} side="new" {fontSize} wrap={true} {gutterWidth} />
      {/if}
    {/each}
  </div>
{:else}
  <!-- No-wrap mode: independent columns with virtualized rows. -->
  <div class="flex font-mono" bind:this={paneEl}>
    <div class="min-w-0 flex-1 overflow-x-auto overflow-y-hidden" style="overscroll-behavior-x: none;">
      <div style="min-width: fit-content;">
        {@render splitRows('old', false)}
      </div>
    </div>
    <div class="w-px shrink-0 bg-edge"></div>
    <div class="min-w-0 flex-1 overflow-x-auto overflow-y-hidden" style="overscroll-behavior-x: none;">
      <div style="min-width: fit-content;">
        {@render splitRows('new', false)}
      </div>
    </div>
  </div>
{/if}
