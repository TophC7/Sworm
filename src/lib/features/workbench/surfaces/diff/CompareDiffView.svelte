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
  import { dirname } from '$lib/utils/paths'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'

  let { folderPath, left, right }: { folderPath: string; left: CompareSide; right: CompareFileSide } = $props()

  // Matches the git diff 2 MiB cap: files are gated on bytes before reading,
  // clipboard text on characters.
  const MAX_COMPARE_BYTES = 2 * 1024 * 1024
  let diskVersion = $state(0)
  let content = $state<DiffFileContent | null>(null)
  let error = $state<string | null>(null)
  /** Last result per file; a disk event re-reads only files whose stat identity moved. */
  const fileCache = new Map<string, { version: string; text: string | null }>()
  const compareLoad = createTrackedAsyncLoad<object>()
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

  function diffable(text: string): string | null {
    return text.length > MAX_COMPARE_BYTES || text.slice(0, 8192).includes('\0') ? null : text
  }

  async function readFile(path: string): Promise<string | null> {
    const stat = await backend.files.stat(folderPath, path)
    const key = `${folderPath}\0${path}`
    const cached = fileCache.get(key)
    if (cached?.version === stat.version) return cached.text
    const text = stat.size > MAX_COMPARE_BYTES ? null : diffable((await backend.files.read(folderPath, path)).content)
    // Keyed by the pre-read stat: a write racing the read changes the identity, so the next event re-reads.
    fileCache.set(key, { version: stat.version, text })
    return text
  }

  function readSide(side: CompareSide): Promise<string | null> {
    return side.kind === 'clipboard' ? Promise.resolve(diffable(side.content)) : readFile(side.path)
  }

  $effect(() => {
    const dirs = [...new Set([left, right].flatMap((side) => (side.kind === 'file' ? [dirname(side.path)] : [])))]
    const unwatches = dirs.map((dir) => watchFileDir(folderPath, dir))
    let disposed = false
    let unlisten: (() => void) | null = null
    void backend.files
      .onChanged((event) => {
        if (disposed) return
        if (event.folder_path === folderPath && dirs.some((dir) => event.dirs.includes(dir))) {
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
    // A fresh key object per run: the effect re-runs only when folderPath, a side or diskVersion
    // changes, and the key just supersedes the in-flight load (no O(clipboard) key compare).
    const request = { folderPath, left, right, diskVersion }
    compareLoad.run(request, async (isCurrent) => {
      try {
        const [oldContent, newContent] = await Promise.all([readSide(request.left), readSide(request.right)])
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
      <span class="font-mono">{left.kind === 'clipboard' ? 'Clipboard' : left.path} ↔ {right.path}</span>
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
        {folderPath}
        contentFetcher={() => Promise.resolve(loadedContent)}
        loading={false}
      />
    {/key}
  {:else}
    <div class="flex flex-1 items-center justify-center text-base text-subtle">Loading…</div>
  {/if}
</div>
