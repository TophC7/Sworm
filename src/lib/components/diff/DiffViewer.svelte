<script lang="ts">
  import { DiffFile, DiffMode } from '$lib/diff/types'
  import { buildRows } from '$lib/diff/viewmodel.svelte'
  import { highlightDiffFile } from '$lib/diff/highlighter.svelte'
  import { parseDiffMeta, extToHighlightLang, isBinaryDiff } from '$lib/utils/diffParser'
  import DiffUnifiedPane from '$lib/components/diff/DiffUnifiedPane.svelte'
  import DiffSplitPane from '$lib/components/diff/DiffSplitPane.svelte'

  let {
    rawDiff,
    filePath,
    oldContent = null,
    newContent = null,
    mode = DiffMode.Split,
    wrap = false,
    fontSize = 13
  }: {
    rawDiff: string
    filePath: string
    oldContent?: string | null
    newContent?: string | null
    mode?: DiffMode
    wrap?: boolean
    fontSize?: number
  } = $props()

  let meta = $derived(parseDiffMeta(rawDiff))
  let lang = $derived(extToHighlightLang(filePath))
  let binary = $derived(isBinaryDiff(rawDiff))
  let hasDiffContent = $derived(rawDiff.trim().length > 0)

  /**
   * Create and initialize a DiffFile instance.
   * Matches the original @git-diff-view/svelte init flow:
   * initRaw() → buildSplitDiffLines() → buildUnifiedDiffLines()
   */
  let diffFile = $derived.by(() => {
    if (!hasDiffContent || binary) return null

    const df = DiffFile.createInstance({
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
      hunks: [rawDiff]
    })
    df.init()
    df.initRaw()
    df.buildSplitDiffLines()
    df.buildUnifiedDiffLines()
    return df
  })

  /** Version counter — increments on DiffFile mutations to trigger reactive rebuilds. */
  let version = $state(0)

  // Subscribe to DiffFile mutations (hunk expand/collapse, syntax highlighting).
  $effect(() => {
    if (!diffFile) return
    const unsub = diffFile.subscribe(() => {
      version++
    })
    return unsub
  })

  // Trigger async syntax highlighting after DiffFile is created.
  $effect(() => {
    const df = diffFile
    const l = lang
    if (!df || l === 'plaintext') return
    void highlightDiffFile(df, l)
  })

  let rows = $derived.by(() => {
    void version
    if (!diffFile) return []
    return buildRows(diffFile, mode === DiffMode.Split ? 'split' : 'unified')
  })
</script>

{#if binary}
  <div class="flex items-center justify-center py-6 text-[0.8rem] text-subtle italic">
    Binary file — diff not available
  </div>
{:else if !hasDiffContent}
  <div class="flex items-center justify-center py-6 text-[0.8rem] text-subtle italic">No changes</div>
{:else if diffFile}
  {#if mode === DiffMode.Unified}
    <DiffUnifiedPane {diffFile} {rows} {wrap} {fontSize} />
  {:else}
    <DiffSplitPane {diffFile} {rows} {wrap} {fontSize} />
  {/if}
{/if}
