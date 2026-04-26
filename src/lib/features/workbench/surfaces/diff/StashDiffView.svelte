<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { FileDiff, StashEntry } from '$lib/types/backend'
  import DiffStack from '$lib/features/workbench/surfaces/diff/DiffStack.svelte'
  import { parseStashMessage } from '$lib/features/git/git'
  import { PackageIcon, GitBranchIcon, CalendarIcon } from '$lib/icons/lucideExports'
  import { timeAgo, formatFullDate } from '$lib/utils/date'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'

  let {
    stashIndex,
    projectId,
    projectPath,
    initialFile = null
  }: {
    stashIndex: number
    projectId: string
    projectPath: string
    initialFile?: string | null
  } = $props()

  let stashEntry = $state<StashEntry | null>(null)
  let files = $state<FileDiff[]>([])
  const stashLoad = createTrackedAsyncLoad<number>()
  let loading = $derived(stashLoad.loading)

  let parsed = $derived(stashEntry ? parseStashMessage(stashEntry.message) : { branch: null, label: '' })

  $effect(() => {
    const idx = stashIndex
    const path = projectPath
    stashLoad.run(idx, async (isCurrent) => {
      stashEntry = null
      files = []
      const [list, f] = await Promise.all([
        backend.git.stashList(path),
        backend.git.getDiffFiles(path, { kind: 'stash', index: idx })
      ])
      if (!isCurrent()) return
      stashEntry = list.find((s) => s.index === idx) ?? null
      files = f
    })
  })
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

    <DiffStack {files} {loading} {initialFile} idPrefix="stash-file" {projectId} {projectPath} {stashIndex} />
  </div>
{/if}
