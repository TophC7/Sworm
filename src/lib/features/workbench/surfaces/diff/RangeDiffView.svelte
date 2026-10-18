<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { FileDiff } from '$lib/types/backend'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import DiffStack from '$lib/features/workbench/surfaces/diff/DiffStack.svelte'
  import type { DiffContentFetcher } from '$lib/features/editor/renderers/monaco/diff/diffModels.svelte'
  import { GitCompareIcon } from '$lib/icons/lucideExports'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'

  let {
    folderPath,
    base,
    baseLabel,
    target,
    targetLabel,
    initialFile = null
  }: {
    folderPath: string
    base: string
    baseLabel: string
    target: string | null
    targetLabel: string
    initialFile?: string | null
  } = $props()

  // The working tree also changes when HEAD moves with no uncommitted changes (checkout, reset).
  let changeSignature = $derived.by(() => {
    if (target !== null) return ''
    const summary = getGitSummary(folderPath)
    const changes = (summary?.changes ?? []).map(
      (change) => `${change.path}:${change.status}:${change.additions}:${change.deletions}`
    )
    return [summary?.head ?? '', ...changes].join('\0')
  })
  let files = $state<FileDiff[]>([])
  let loaded = $state(false)
  let error = $state<string | null>(null)
  const rangeLoad = createTrackedAsyncLoad<string>()
  let loading = $derived(rangeLoad.loading)

  $effect(() => {
    const path = folderPath
    const oldRev = base
    const newRev = target
    const key = `${path}|${oldRev}|${newRev ?? ''}|${changeSignature}`
    rangeLoad.run(key, async (isCurrent) => {
      loaded = false
      error = null
      files = []
      try {
        const result = await backend.git.diffRange(path, oldRev, newRev)
        if (!isCurrent()) return
        files = result
        loaded = true
      } catch (cause) {
        if (isCurrent()) error = getErrorMessage(cause)
      }
    })
  })

  const contentFetcher: DiffContentFetcher = (entry) =>
    backend.git.getDiffFile(folderPath, { kind: 'range', base, target }, entry.path, entry.oldPath, entry.status)
</script>

<div class="flex h-full flex-col overflow-hidden">
  <div class="shrink-0 border-b border-edge bg-surface px-4 py-3">
    <h2 class="mb-2 text-md leading-snug font-semibold text-bright">{baseLabel} ↔ {targetLabel}</h2>
    <div class="flex items-center gap-2 text-sm text-muted">
      <GitCompareIcon size={12} />
      <span class="font-mono">{base.slice(0, 7)} ↔ {target?.slice(0, 7) ?? 'working tree'}</span>
    </div>
  </div>
  {#if error}
    <div class="px-4 py-3 text-base text-danger">{error}</div>
  {:else if loaded && files.length === 0}
    <div class="flex flex-1 items-center justify-center text-base text-subtle">
      No changes between {baseLabel} and {targetLabel}.
    </div>
  {:else}
    {#key `${folderPath}|${base}|${target ?? ''}|${changeSignature}`}
      <DiffStack {files} {loading} {initialFile} idPrefix="range-file" {folderPath} {contentFetcher} />
    {/key}
  {/if}
</div>
