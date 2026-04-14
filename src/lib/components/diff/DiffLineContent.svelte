<!--
  @component
  Renders a single line of diff content using @git-diff-view/core's
  template system — matching the original library's rendering path.

  Three-tier rendering priority:
  1. Syntax template ({@html} from getSyntaxDiffTemplate/getSyntaxLineTemplate)
  2. Plain template ({@html} from getPlainDiffTemplate/getPlainLineTemplate)
  3. Raw text fallback
-->

<script lang="ts">
  import {
    DiffLineType,
    type DiffFile,
    type DiffLine,
    type SyntaxLine,
    getPlainDiffTemplate,
    getPlainLineTemplate,
    getSyntaxDiffTemplate,
    getSyntaxLineTemplate
  } from '$lib/diff/types'
  import { LINE_HEIGHT_RATIO } from '$lib/diff/layout'

  let {
    diffFile,
    rawLine,
    diffLine = undefined,
    side = 'new' as 'old' | 'new',
    lineNumber = 0,
    wrap,
    fontSize
  }: {
    diffFile: DiffFile
    rawLine: string
    diffLine?: DiffLine
    side?: 'old' | 'new'
    lineNumber?: number
    wrap: boolean
    fontSize: number
  } = $props()

  let isAdded = $derived(diffLine?.type === DiffLineType.Add)
  let isDelete = $derived(diffLine?.type === DiffLineType.Delete)
  let operator = $derived(isAdded ? '+' : isDelete ? '-' : ' ')
  let operatorSide = $derived.by((): 'add' | 'del' => (isDelete ? 'del' : 'add'))

  // Reactive syntax + plain line data — refreshes on DiffFile subscription
  const getPlain = () =>
    lineNumber > 0
      ? side === 'old'
        ? diffFile.getOldPlainLine(lineNumber)
        : diffFile.getNewPlainLine(lineNumber)
      : undefined

  const getSyntax = () => {
    if (lineNumber <= 0) return undefined
    try {
      return side === 'old' ? diffFile.getOldSyntaxLine(lineNumber) : diffFile.getNewSyntaxLine(lineNumber)
    } catch {
      return undefined
    }
  }

  let plainLine = $state(getPlain())
  let syntaxLine = $state<(SyntaxLine & { template?: string }) | undefined>(getSyntax())

  const refresh = () => {
    plainLine = getPlain()
    syntaxLine = getSyntax()
  }

  // Re-fetch syntax/plain data when DiffFile mutates (e.g. syntax loaded)
  $effect(() => {
    refresh()
    return diffFile.subscribe(refresh)
  })

  let enableSyntax = $derived(syntaxLine && syntaxLine.nodeList?.length > 0 && syntaxLine.nodeList.length <= 150)

  $effect(() => {
    // Trigger template generation when data changes
    void plainLine
    void syntaxLine

    if (enableSyntax && syntaxLine && diffLine) {
      // Syntax path
      if (diffLine.changes?.hasLineChange) {
        if (!diffLine.syntaxTemplate) {
          getSyntaxDiffTemplate({ diffFile, diffLine, syntaxLine, operator: operatorSide })
        }
      } else if (!syntaxLine.template) {
        syntaxLine.template = getSyntaxLineTemplate(syntaxLine)
      }
    } else if (diffLine) {
      // Plain path
      if (diffLine.changes?.hasLineChange) {
        if (!diffLine.plainTemplate) {
          getPlainDiffTemplate({ diffLine, rawLine, operator: operatorSide })
        }
      } else if (plainLine && !plainLine.template) {
        plainLine.template = getPlainLineTemplate(plainLine.value)
      }
    }
  })
</script>

<div
  class="diff-line-content-item pl-[2em]"
  style="white-space: {wrap ? 'pre-wrap' : 'pre'}; word-break: {wrap
    ? 'break-all'
    : 'initial'}; font-size: {fontSize}px; line-height: {fontSize * LINE_HEIGHT_RATIO}px; tab-size: 4;"
>
  <!-- Operator glyph -->
  <span
    class="ml-[-1.5em] inline-block w-[1.5em] indent-[0.2em] select-none"
    class:text-success={isAdded}
    class:text-danger={isDelete}
    class:text-subtle={!isAdded && !isDelete}
  >
    {operator}
  </span>

  <!-- Content rendering (three-tier) -->
  {#if enableSyntax && syntaxLine}
    <!-- Syntax-highlighted rendering -->
    {#if diffLine?.changes?.hasLineChange && diffLine.syntaxTemplate}
      <span class="diff-line-syntax-raw">{@html diffLine.syntaxTemplate}</span>
    {:else if syntaxLine.template}
      <span class="diff-line-syntax-raw">{@html syntaxLine.template}</span>
    {:else}
      <!-- Syntax nodeList fallback -->
      <span class="diff-line-syntax-raw">
        {#each syntaxLine.nodeList as { node, wrapper }}
          <span style={wrapper?.properties?.style}>{node.value}</span>
        {/each}
      </span>
    {/if}
  {:else if diffLine?.changes?.hasLineChange && diffLine.plainTemplate}
    <!-- Plain text with char-level diff template -->
    <span class="diff-line-content-raw">{@html diffLine.plainTemplate}</span>
  {:else if plainLine?.template}
    <!-- Plain text template (escaped) -->
    <span class="diff-line-content-raw">{@html plainLine.template}</span>
  {:else}
    <!-- Raw text fallback -->
    <span class="diff-line-content-raw">{rawLine}</span>
  {/if}
</div>
