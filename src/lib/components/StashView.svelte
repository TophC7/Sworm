<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { StashEntry } from '$lib/types/backend'
  import DiffStack, { type DiffEntry } from '$lib/components/diff/DiffStack.svelte'
  import { parseStashMessage } from '$lib/utils/git'
  import { PackageIcon, GitBranchIcon, CalendarIcon } from '$lib/icons/lucideExports'
  import { timeAgo, formatFullDate } from '$lib/utils/date'

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
  let rawDiffs = $state<Record<string, string>>({})
  let loadingDiffs = $state(false)
  let loadedIndex = -1

  let diffs = $derived.by(() => {
    const m = new Map<string, DiffEntry>()
    for (const [p, d] of Object.entries(rawDiffs)) {
      m.set(p, { rawDiff: d })
    }
    return m
  })

  let parsed = $derived(stashEntry ? parseStashMessage(stashEntry.message) : { branch: null, label: '' })

  $effect(() => {
    const idx = stashIndex
    const path = projectPath
    if (idx === loadedIndex) return
    loadedIndex = idx
    rawDiffs = {}
    stashEntry = null
    void loadStash(path, idx)
  })

  async function loadStash(path: string, idx: number) {
    try {
      // Load stash metadata from the list
      const list = await backend.git.stashList(path)
      if (idx !== loadedIndex) return
      stashEntry = list.find((s) => s.index === idx) ?? null
      if (!stashEntry) return

      loadingDiffs = true
      const dR = await backend.git.getStashDiffs(path, idx)
      if (idx !== loadedIndex) return
      rawDiffs = dR
    } catch {
      if (idx !== loadedIndex) return
      stashEntry = null
      rawDiffs = {}
    } finally {
      loadingDiffs = false
    }
  }
</script>

{#if !stashEntry}
  <div class="flex h-full items-center justify-center text-[0.78rem] text-subtle">Loading stash...</div>
{:else}
  <div class="flex h-full flex-col overflow-hidden">
    <!-- Stash header -->
    <div class="shrink-0 border-b border-edge bg-surface px-4 py-3">
      <h2 class="mb-2 text-[0.88rem] leading-snug font-semibold text-bright">{parsed.label}</h2>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-[0.72rem] text-muted">
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

    <DiffStack
      files={stashEntry.files}
      {diffs}
      loading={loadingDiffs}
      {initialFile}
      idPrefix="stash-file"
      {projectId}
      {projectPath}
      {stashIndex}
    />
  </div>
{/if}
