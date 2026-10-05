<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { StashEntry } from '$lib/types/backend'
  import DiffStack from '$lib/features/workbench/surfaces/diff/DiffStack.svelte'
  import type { DiffContentFetcher } from '$lib/features/editor/renderers/monaco/diff/diffModels.svelte'
  import { parseStashMessage } from '$lib/features/git/gitRefs'
  import { PackageIcon, GitBranchIcon, CalendarIcon } from '$lib/icons/lucideExports'
  import { timeAgo, formatFullDate } from '$lib/utils/date'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'

  let {
    stashIndex,
    folderPath,
    initialFile = null
  }: {
    stashIndex: number
    folderPath: string
    initialFile?: string | null
  } = $props()

  let stashEntry = $state<StashEntry | null>(null)
  let files = $derived(stashEntry?.files ?? [])
  const stashLoad = createTrackedAsyncLoad<number>()
  let loading = $derived(stashLoad.loading)

  let parsed = $derived(stashEntry ? parseStashMessage(stashEntry.message) : { branch: null, label: '' })

  $effect(() => {
    const idx = stashIndex
    const path = folderPath
    stashLoad.run(idx, async (isCurrent) => {
      stashEntry = null
      const list = await backend.git.stashList(path)
      if (!isCurrent()) return
      stashEntry = list.find((s) => s.index === idx) ?? null
    })
  })

  const contentFetcher: DiffContentFetcher = (entry) =>
    backend.git.getDiffFile(folderPath, { kind: 'stash', index: stashIndex }, entry.path, entry.oldPath, entry.status)
</script>

{#if !stashEntry}
  <div class="flex h-full items-center justify-center text-base text-subtle">Loading stash...</div>
{:else}
  <div class="flex h-full flex-col overflow-hidden">
    <!-- Stash header -->
    <div class="shrink-0 border-b border-edge bg-surface px-4 py-3">
      <h2 class="mb-2 text-md leading-snug font-semibold text-bright">{parsed.label}</h2>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-muted">
        <span class="flex items-center gap-1">
          <PackageIcon size={12} />
          <code class="font-mono text-accent">stash@{'{' + stashIndex + '}'}</code>
        </span>
        <span class="flex items-center gap-1">
          <CalendarIcon size={12} />
          {timeAgo(stashEntry.date)}
          <span class="text-subtle">({formatFullDate(stashEntry.date)})</span>
        </span>
        {#if parsed.branch}
          <span class="flex items-center gap-1">
            <GitBranchIcon size={12} />
            <code class="font-mono text-muted">{parsed.branch}</code>
          </span>
        {/if}
      </div>
    </div>

    <DiffStack {files} {loading} {initialFile} idPrefix="stash-file" {folderPath} {stashIndex} {contentFetcher} />
  </div>
{/if}
