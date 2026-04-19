<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { CommitDetail } from '$lib/types/backend'
  import DiffStack, { type DiffEntry } from '$lib/components/diff/DiffStack.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { GitCommitIcon, GitBranchIcon, UserIcon, CalendarIcon, CopyIcon, Check } from '$lib/icons/lucideExports'
  // Alias for CheckIcon
  const CheckIcon = Check

  let {
    commitHash,
    projectId,
    projectPath,
    initialFile = null
  }: {
    commitHash: string
    projectId: string
    projectPath: string
    initialFile?: string | null
  } = $props()

  let detail = $state<CommitDetail | null>(null)
  let rawDiffs = $state<Record<string, string>>({})
  let loadingDiffs = $state(false)
  let copied = $state(false)
  let loadedHash = ''

  let diffs = $derived.by(() => {
    const m = new Map<string, DiffEntry>()
    for (const [p, d] of Object.entries(rawDiffs)) {
      m.set(p, { rawDiff: d })
    }
    return m
  })

  $effect(() => {
    const hash = commitHash
    const path = projectPath
    if (hash === loadedHash) return
    loadedHash = hash
    rawDiffs = {}
    detail = null
    void loadDetail(path, hash)
  })

  async function loadDetail(path: string, hash: string) {
    try {
      const d = await backend.git.getCommitDetail(path, hash)
      if (hash !== loadedHash) return
      detail = d
      if (!detail) return

      loadingDiffs = true
      const dR = await backend.git.getCommitDiffs(path, hash)
      if (hash !== loadedHash) return
      rawDiffs = dR
    } catch {
      if (hash !== loadedHash) return
      detail = null
      rawDiffs = {}
    } finally {
      loadingDiffs = false
    }
  }

  function formatDate(iso: string): string {
    try {
      const d = new Date(iso)
      return (
        d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' }) +
        ' at ' +
        d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
      )
    } catch {
      return iso
    }
  }

  async function copyHash() {
    await navigator.clipboard.writeText(commitHash)
    copied = true
    setTimeout(() => (copied = false), 1500)
  }
</script>

{#if !detail}
  <div class="flex h-full items-center justify-center text-base text-subtle">Loading commit...</div>
{:else}
  <div class="flex h-full flex-col overflow-hidden">
    <!-- Commit header -->
    <div class="shrink-0 border-b border-edge bg-surface px-4 py-3">
      <h2 class="mb-2 text-md leading-snug font-semibold text-bright">{detail.message}</h2>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-muted">
        <span class="flex items-center gap-1">
          <UserIcon size={12} />
          {detail.author}
        </span>
        <span class="flex items-center gap-1">
          <CalendarIcon size={12} />
          {formatDate(detail.date)}
        </span>
        <span class="flex items-center gap-1">
          <GitCommitIcon size={12} />
          <code class="font-mono text-accent">{detail.short_hash}</code>
          <IconButton
            tooltip="Copy full hash"
            tooltipSide="bottom"
            class="rounded p-0.5 text-muted transition-colors hover:text-fg"
            onclick={copyHash}
          >
            {#if copied}
              <CheckIcon size={11} />
            {:else}
              <CopyIcon size={11} />
            {/if}
          </IconButton>
        </span>
        {#if detail.parents.length > 0}
          <span class="flex items-center gap-1">
            <GitBranchIcon size={12} />
            {#each detail.parents as parent, i}
              {#if i > 0}<span class="text-subtle">+</span>{/if}
              <code class="font-mono text-muted">{parent.slice(0, 7)}</code>
            {/each}
          </span>
        {/if}
      </div>
    </div>

    <DiffStack
      files={detail.files}
      {diffs}
      loading={loadingDiffs}
      {initialFile}
      idPrefix="commit-file"
      {projectId}
      {projectPath}
      {commitHash}
    />
  </div>
{/if}
