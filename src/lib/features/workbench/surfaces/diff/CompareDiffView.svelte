<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { DiffFileContent, FileDiff } from '$lib/types/backend'
  import { filePathToLanguage } from '$lib/features/editor/languageMap'
  import { watchFileDir } from '$lib/features/files/explorer.svelte'
  import type { CompareFileSide, CompareSide } from '$lib/features/workbench/model'
  import DiffStack from '$lib/features/workbench/surfaces/diff/DiffStack.svelte'
  import { compareSideLabel } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { GitCompareIcon } from '$lib/icons/lucideExports'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { basename, dirname, normalizeAbsolutePath } from '$lib/utils/paths'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'

  let { left, right }: { left: CompareSide; right: CompareFileSide } = $props()

  // Approximates the git diff 2 MiB byte cap.
  const MAX_COMPARE_CHARS = 2 * 1024 * 1024
  let diskVersion = $state(0)
  let content = $state<DiffFileContent | null>(null)
  let error = $state<string | null>(null)
  const compareLoad = createTrackedAsyncLoad<string>()
  const leftLabel = $derived(compareSideLabel(left))
  const rightLabel = $derived(compareSideLabel(right))
  const row = $derived<FileDiff>({
    path: right.path,
    oldPath: null,
    status: 'modified',
    lang: filePathToLanguage(right.path),
    additions: null,
    deletions: null
  })

  function sideKey(side: CompareSide): string {
    return side.kind === 'file'
      ? `file:${side.folderPath}:${side.path}`
      : `clipboard:${side.content.length}:${side.content}`
  }

  function describeSide(side: CompareSide): string {
    if (side.kind === 'clipboard') return 'Clipboard'
    return side.folderPath === right.folderPath
      ? side.path
      : `${basename(normalizeAbsolutePath(side.folderPath))}/${side.path}`
  }

  async function readSide(side: CompareSide): Promise<string | null> {
    let text: string
    if (side.kind === 'clipboard') {
      text = side.content
    } else {
      try {
        text = (await backend.files.read(side.folderPath, side.path)).content
      } catch (cause) {
        if ((cause as { kind?: unknown } | null)?.kind === 'tooLarge') return null
        throw cause
      }
    }
    return text.length > MAX_COMPARE_CHARS || text.slice(0, 8192).includes('\0') ? null : text
  }

  $effect(() => {
    const watched: { folderPath: string; dir: string }[] = []
    for (const side of [left, right]) {
      if (side.kind !== 'file') continue
      const dir = dirname(side.path)
      if (!watched.some((pair) => pair.folderPath === side.folderPath && pair.dir === dir)) {
        watched.push({ folderPath: side.folderPath, dir })
      }
    }
    const unwatches = watched.map(({ folderPath, dir }) => watchFileDir(folderPath, dir))
    let disposed = false
    let unlisten: (() => void) | null = null
    void backend.files
      .onChanged((event) => {
        if (disposed) return
        if (watched.some(({ folderPath, dir }) => event.folder_path === folderPath && event.dirs.includes(dir))) {
          diskVersion += 1
        }
      })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch(() => {})
    return () => {
      disposed = true
      unlisten?.()
      unlisten = null
      for (const unwatch of unwatches) unwatch()
    }
  })

  $effect(() => {
    const oldSide = left
    const newSide = right
    const key = `${sideKey(oldSide)}|${sideKey(newSide)}|${diskVersion}`
    compareLoad.run(key, async (isCurrent) => {
      try {
        const [oldContent, newContent] = await Promise.all([readSide(oldSide), readSide(newSide)])
        if (!isCurrent()) return
        const next: DiffFileContent =
          oldContent === null || newContent === null
            ? { oldContent: null, newContent: null, binary: true }
            : { oldContent, newContent, binary: false }
        if (
          !content ||
          content.oldContent !== next.oldContent ||
          content.newContent !== next.newContent ||
          content.binary !== next.binary
        ) {
          content = next
        }
        error = null
      } catch (cause) {
        if (!isCurrent()) return
        error = getErrorMessage(cause)
        content = null
      }
    })
  })
</script>

<div class="flex h-full flex-col overflow-hidden">
  <div class="shrink-0 border-b border-edge bg-surface px-4 py-3">
    <h2 class="mb-2 text-md leading-snug font-semibold text-bright">{leftLabel} ↔ {rightLabel}</h2>
    <div class="flex items-center gap-2 text-sm text-muted">
      <GitCompareIcon size={12} />
      <span class="font-mono">{describeSide(left)} ↔ {describeSide(right)}</span>
    </div>
  </div>
  {#if error}
    <div class="px-4 py-3 text-base text-danger">{error}</div>
  {:else if content && !content.binary && content.oldContent === content.newContent}
    <div class="flex flex-1 items-center justify-center text-base text-subtle">
      {leftLabel} and {rightLabel} are identical.
    </div>
  {:else if content}
    {#key content}
      {@const loadedContent = content}
      <DiffStack
        files={[row]}
        initialFile={right.path}
        idPrefix="compare-file"
        folderPath={right.folderPath}
        contentFetcher={() => Promise.resolve(loadedContent)}
        loading={false}
      />
    {/key}
  {:else}
    <div class="flex flex-1 items-center justify-center text-base text-subtle">Loading…</div>
  {/if}
</div>
