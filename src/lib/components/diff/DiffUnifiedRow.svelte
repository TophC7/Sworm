<!--
  @component
  Single row in the unified diff view.

  Layout: [old-line-num | new-line-num | content]
  Matches the original @git-diff-view/svelte's unified content line.
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
    fontSize,
    wrap,
    gutterWidth
  }: {
    diffFile: DiffFile
    index: number
    fontSize: number
    wrap: boolean
    gutterWidth: string
  } = $props()

  let line = $derived(diffFile.getUnifiedLine(index))
  let diff = $derived(line?.diff ?? null)
  let lineType = $derived(diff?.type ?? DiffLineType.Context)
  let rawLine = $derived(line?.value ?? '')

  /** Determine which side's data to use based on line type. */
  let syntaxSide = $derived.by((): 'old' | 'new' => {
    if (lineType === DiffLineType.Delete) return 'old'
    return 'new'
  })

  let syntaxLineNumber = $derived.by((): number => {
    if (syntaxSide === 'old') return line?.oldLineNumber ?? 0
    return line?.newLineNumber ?? 0
  })

  let hasChange = $derived(lineType === DiffLineType.Add || lineType === DiffLineType.Delete)
  let rowHeight = $derived(lineHeight(fontSize))
</script>

{#if line && !line.isHidden}
  <div class="flex {lineBgClass(lineType)}">
    <!-- Sticky gutter pair: opaque bg-ground base + semi-transparent tint -->
    <div class="sticky left-0 z-10 flex shrink-0" style="background-color: var(--color-ground);">
      <span
        class="text-right font-mono text-subtle select-none {gutterBgClass(lineType)}"
        style="width: {gutterWidth}; font-size: {fontSize}px; line-height: {rowHeight}; padding: 0 6px; opacity: {hasChange
          ? 1
          : 0.5};"
      >
        {line.oldLineNumber ?? ''}
      </span>
      <span
        class="text-right font-mono text-subtle select-none {gutterBgClass(lineType)}"
        style="width: {gutterWidth}; font-size: {fontSize}px; line-height: {rowHeight}; padding: 0 6px; opacity: {hasChange
          ? 1
          : 0.5};"
      >
        {line.newLineNumber ?? ''}
      </span>
    </div>
    <!-- Content -->
    <div class="min-w-0 flex-1">
      <DiffLineContent
        {diffFile}
        {rawLine}
        diffLine={diff ?? undefined}
        side={syntaxSide}
        lineNumber={syntaxLineNumber}
        {wrap}
        {fontSize}
      />
    </div>
  </div>
{/if}
