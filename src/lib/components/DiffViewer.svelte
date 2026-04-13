<script lang="ts">
  import { DiffView, DiffModeEnum } from '@git-diff-view/svelte'
  import '@git-diff-view/svelte/styles/diff-view-pure.css'
  import { parseDiffMeta, extToHighlightLang, isBinaryDiff } from '$lib/utils/diffParser'

  let {
    rawDiff,
    filePath,
    oldContent = null,
    newContent = null,
    mode = DiffModeEnum.Split,
    wrap = false,
    fontSize = 13
  }: {
    rawDiff: string
    filePath: string
    oldContent?: string | null
    newContent?: string | null
    mode?: DiffModeEnum
    wrap?: boolean
    fontSize?: number
  } = $props()

  let meta = $derived(parseDiffMeta(rawDiff))
  let lang = $derived(extToHighlightLang(filePath))
  let binary = $derived(isBinaryDiff(rawDiff))
  let hasDiffContent = $derived(rawDiff.trim().length > 0)
  let hunks = $derived(rawDiff.trim() ? [rawDiff] : [])

  let data = $derived({
    oldFile: {
      fileName: meta.oldFileName ?? filePath,
      fileLang: lang,
      content: oldContent ?? undefined
    },
    newFile: {
      fileName: meta.newFileName ?? filePath,
      fileLang: lang,
      content: newContent ?? undefined
    },
    hunks
  })
</script>

<div class="diff-viewer-root">
  {#if binary}
    <div class="flex items-center justify-center py-6 text-[0.8rem] text-subtle italic">
      Binary file — diff not available
    </div>
  {:else if !hasDiffContent}
    <div class="flex items-center justify-center py-6 text-[0.8rem] text-subtle italic">No changes</div>
  {:else}
    <DiffView
      {data}
      diffViewMode={mode}
      diffViewTheme="dark"
      diffViewHighlight={true}
      diffViewWrap={wrap}
      diffViewFontSize={fontSize}
    />
  {/if}
</div>
