<!--
  @component
  Renders ONE side of a split diff row (old or new).

  Layout: [gutter | content]
  Empty lines (no content on this side) render as a placeholder.
-->

<script lang="ts">
  import type { DiffFile } from '$lib/diff/types'
  import { DiffLineType } from '$lib/diff/types'
  import { lineHeight } from '$lib/diff/layout'
  import { lineBgClass, gutterBgClass } from '$lib/diff/colors'
  import DiffLineContent from './DiffLineContent.svelte'

  let {
    diffFile,
    index,
    side,
    fontSize,
    wrap,
    gutterWidth
  }: {
    diffFile: DiffFile
    index: number
    side: 'old' | 'new'
    fontSize: number
    wrap: boolean
    gutterWidth: string
  } = $props()

  let line = $derived(side === 'old' ? diffFile.getSplitLeftLine(index) : diffFile.getSplitRightLine(index))
  let diff = $derived(line?.diff ?? null)
  let lineType = $derived(diff?.type ?? DiffLineType.Context)
  let hasContent = $derived(line?.lineNumber != null && line.lineNumber > 0)
  let rawLine = $derived(line?.value ?? '')
  let hasChange = $derived(lineType === DiffLineType.Add || lineType === DiffLineType.Delete)
  let rowHeight = $derived(lineHeight(fontSize))
</script>

{#if hasContent}
  <div class="flex {lineBgClass(lineType)}">
    <!-- Sticky gutter: opaque bg-ground base + semi-transparent tint -->
    <div class="sticky left-0 z-10 shrink-0" style="width: {gutterWidth}; background-color: var(--color-ground);">
      <span
        class="block text-right font-mono text-subtle select-none {gutterBgClass(lineType)}"
        style="font-size: {fontSize}px; line-height: {rowHeight}; padding: 0 6px; opacity: {hasChange ? 1 : 0.5};"
      >
        {line?.lineNumber}
      </span>
    </div>
    <div class="flex-1">
      <DiffLineContent
        {diffFile}
        {rawLine}
        diffLine={diff ?? undefined}
        {side}
        lineNumber={line?.lineNumber ?? 0}
        {wrap}
        {fontSize}
      />
    </div>
  </div>
{:else}
  <!-- Empty placeholder row (other side has content) -->
  <div class="flex" style="background-color: var(--color-surface);">
    <div class="sticky left-0 z-10 shrink-0" style="width: {gutterWidth}; background-color: var(--color-surface);">
      <span
        class="block text-right select-none"
        style="font-size: {fontSize}px; line-height: {rowHeight}; padding: 0 6px;">&ensp;</span
      >
    </div>
    <div class="flex-1">
      <span class="block" style="font-size: {fontSize}px; line-height: {rowHeight};">&ensp;</span>
    </div>
  </div>
{/if}
