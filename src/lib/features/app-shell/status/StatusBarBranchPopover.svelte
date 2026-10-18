<!--
  @component
  StatusBarBranchPopover: current-branch status chip with a quick-switch popover.
-->

<script lang="ts">
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { SearchInput } from '$lib/components/ui/input'
  import { statusChipVariants } from '$lib/components/ui/status-chip'
  import { revealGitBranches } from '$lib/features/app-shell/sidebar/state.svelte'
  import {
    ArrowDownToLineIcon,
    ArrowRight,
    ArrowUpFromLineIcon,
    Check,
    CloudUploadIcon,
    GitBranchIcon,
    GitCommitIcon,
    GitMergeIcon,
    MoreHorizontalIcon,
    RefreshCwIcon,
    RotateCw,
    ShieldAlertIcon
  } from '$lib/icons/lucideExports'
  import AheadBehindBadge from '$lib/features/git/AheadBehindBadge.svelte'
  import * as branches from '$lib/features/git/branches.svelte'
  import {
    fetchFolder,
    forcePushWithLease,
    publishBranch,
    pullFolder,
    pushFolder,
    syncFolder
  } from '$lib/features/git/actions.svelte'
  import { remoteActionState } from '$lib/features/git/commitButton'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import { describeOperation } from '$lib/features/git/operation'
  import type { BranchSummary } from '$lib/types/backend'
  import { timeAgo } from '$lib/utils/date'

  let { folderPath }: { folderPath: string } = $props()

  let gitSummary = $derived(getGitSummary(folderPath))
  const operation = $derived(gitSummary?.operation ?? null)
  const operationDisplay = $derived(operation ? describeOperation(operation) : null)
  const remote = $derived(gitSummary ? remoteActionState(gitSummary) : { tracking: false, publishable: false })
  const shortHead = $derived(gitSummary?.head?.slice(0, 7) ?? 'HEAD')
  const headName = $derived(gitSummary?.branch ?? operation?.headName ?? null)
  const headerNote = $derived(
    operationDisplay?.headline ??
      (gitSummary?.unborn ? 'No commits yet' : headName && !gitSummary?.upstream ? 'No upstream' : null)
  )
  const actionItemClass = 'flex-1 justify-center gap-1 px-1.5'
  const branchLabel = $derived.by(() => {
    if (!gitSummary) return ''
    const name = gitSummary.branch ?? operation?.headName ?? shortHead
    return operationDisplay ? `${name} ${operationDisplay.chipSuffix}` : name
  })
  let branchDirty = $derived((gitSummary?.changes.length ?? 0) > 0)
  let branchStatus = $derived(
    branchDirty
      ? `${gitSummary?.staged_count ?? 0} staged, ${gitSummary?.unstaged_count ?? 0} unstaged, ${gitSummary?.untracked_count ?? 0} untracked`
      : 'Clean working tree'
  )

  let open = $state(false)
  let search = $state('')

  let entry = $derived(branches.byFolder.get(folderPath))

  // Make sure the entry is loaded before the popover opens; the user
  // may click the chunk before any other view has loaded branches.
  $effect(() => {
    if (open && !entry) {
      void branches.loadFor(folderPath, { autoFetch: false })
    }
  })

  // Resolve recents → BranchSummary, dropping names that no longer
  // exist (deleted branch, fresh clone). Cap at 5.
  let recentSummaries = $derived.by(() => {
    if (!entry) return [] as BranchSummary[]
    const byName = new Map<string, BranchSummary>()
    for (const b of entry.list) byName.set(b.name, b)
    const out: BranchSummary[] = []
    for (const name of entry.prefs.recent) {
      const summary = byName.get(name)
      if (summary) out.push(summary)
      if (out.length >= 5) break
    }
    return out
  })

  // Fallback when the user hasn't built up a recents list yet: the
  // five most recently active local branches by tip date, excluding
  // the current branch.
  let fallbackSummaries = $derived.by(() => {
    if (!entry) return [] as BranchSummary[]
    return entry.list
      .filter((b) => b.kind === 'local' && !b.isCurrent)
      .toSorted((a, b) => new Date(b.tip.date).getTime() - new Date(a.tip.date).getTime())
      .slice(0, 5)
  })

  let candidates = $derived.by(() => {
    const out: BranchSummary[] = []
    const seen = new Set<string>()
    for (const branch of [...recentSummaries, ...fallbackSummaries]) {
      if (seen.has(branch.name)) continue
      seen.add(branch.name)
      out.push(branch)
      if (out.length >= 5) break
    }
    return out
  })

  let visible = $derived.by(() => {
    const q = search.trim().toLowerCase()
    if (!q) return candidates
    return candidates.filter((b) => b.name.toLowerCase().includes(q))
  })

  function selectBranch(name: string) {
    open = false
    void branches.checkoutRef(folderPath, { kind: 'branch', name })
  }

  function viewAllBranches() {
    revealGitBranches(folderPath)
    open = false
  }
</script>

{#if gitSummary?.branch || gitSummary?.operation || gitSummary?.detached}
  <DropdownMenu.Root bind:open>
    <DropdownMenu.Trigger
      class={statusChipVariants({
        tone: operation || branchDirty ? 'warning' : 'success',
        class: 'font-mono'
      })}
      title={operationDisplay?.headline ?? branchStatus}
    >
      {#if operation}
        <GitMergeIcon size={10} class="text-warning" />
      {:else if gitSummary.detached}
        <GitCommitIcon size={10} />
      {:else}
        <GitBranchIcon size={10} />
      {/if}
      {branchLabel}
      <AheadBehindBadge ahead={gitSummary.ahead ?? 0} behind={gitSummary.behind ?? 0} twoColor />
    </DropdownMenu.Trigger>

    <DropdownMenu.Content class="w-80 p-0" sideOffset={8} align="start">
      <div class="border-b border-edge px-3 py-2">
        <div class="flex min-w-0 items-center gap-1.5 font-mono text-sm">
          {#if headName}
            <GitBranchIcon size={12} />
            <span class="truncate text-fg">{headName}</span>
            {#if gitSummary.upstream}
              <ArrowRight size={11} class="text-subtle" />
              <span class="truncate text-muted">{gitSummary.upstream}</span>
              <span class="ms-auto">
                <AheadBehindBadge ahead={gitSummary.ahead ?? 0} behind={gitSummary.behind ?? 0} twoColor />
              </span>
            {/if}
          {:else}
            <GitCommitIcon size={12} />
            <span class="truncate text-fg">Detached at {shortHead}</span>
          {/if}
        </div>
        {#if headerNote}
          <div class={operationDisplay ? 'text-xs text-warning' : 'text-xs text-subtle'}>{headerNote}</div>
        {/if}
      </div>

      <div class="flex items-center gap-0.5 border-b border-edge p-1">
        <DropdownMenu.Item
          class={actionItemClass}
          disabled={!remote.tracking}
          onSelect={() => void syncFolder(folderPath)}
        >
          <RefreshCwIcon size={12} />
          Sync
        </DropdownMenu.Item>
        <DropdownMenu.Item
          class={actionItemClass}
          disabled={!remote.tracking}
          onSelect={() => void pullFolder(folderPath)}
        >
          <ArrowDownToLineIcon size={12} />
          Pull
        </DropdownMenu.Item>
        {#if remote.publishable}
          <DropdownMenu.Item
            class={actionItemClass}
            onSelect={() => void publishBranch(folderPath, gitSummary?.branch ?? null)}
          >
            <CloudUploadIcon size={12} />
            Publish
          </DropdownMenu.Item>
        {:else}
          <DropdownMenu.Item
            class={actionItemClass}
            disabled={!remote.tracking}
            onSelect={() => void pushFolder(folderPath)}
          >
            <ArrowUpFromLineIcon size={12} />
            Push
          </DropdownMenu.Item>
        {/if}
        <DropdownMenu.Item class={actionItemClass} onSelect={() => void fetchFolder(folderPath)}>
          <RotateCw size={12} />
          Fetch
        </DropdownMenu.Item>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger class="px-2" aria-label="More Actions" disabled={!remote.tracking}>
            <MoreHorizontalIcon size={12} />
          </DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent>
            <DropdownMenu.Item disabled={!remote.tracking} onSelect={() => void pullFolder(folderPath, true)}>
              <ArrowDownToLineIcon size={12} />
              Pull (Rebase)
            </DropdownMenu.Item>
            <DropdownMenu.Item disabled={!remote.tracking} onSelect={() => void syncFolder(folderPath, true)}>
              <RefreshCwIcon size={12} />
              Sync (Rebase)
            </DropdownMenu.Item>
            <DropdownMenu.Separator />
            <DropdownMenu.Item
              destructive
              disabled={!remote.tracking}
              onSelect={() => void forcePushWithLease(folderPath)}
            >
              <ShieldAlertIcon size={12} />
              Force Push (with lease)
            </DropdownMenu.Item>
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
      </div>

      <div class="border-b border-edge p-2">
        <SearchInput bind:value={search} placeholder="Switch to branch…" class="text-sm" spellcheck={false} autofocus />
      </div>

      <div class="max-h-72 overflow-y-auto py-1">
        {#if !entry}
          <div class="px-3 py-2 text-sm text-subtle">Loading…</div>
        {:else if visible.length === 0}
          <div class="px-3 py-2 text-sm text-subtle">No matches.</div>
        {:else}
          {#each visible as branch (branch.name)}
            <DropdownMenu.Item class="py-1" onSelect={() => void selectBranch(branch.name)}>
              <GitBranchIcon size={12} />
              <span class="min-w-0 flex-1 truncate font-mono">{branch.name}</span>
              <AheadBehindBadge ahead={branch.ahead} behind={branch.behind} twoColor />
              <span class="shrink-0 text-2xs text-subtle">{timeAgo(branch.tip.date)}</span>
            </DropdownMenu.Item>
          {/each}
        {/if}
      </div>

      <div class="border-t border-edge p-1">
        <DropdownMenu.Item class="px-2 py-1 text-muted" onSelect={() => void branches.checkoutTo(folderPath)}>
          <Check size={12} />
          Checkout to…
        </DropdownMenu.Item>
        <DropdownMenu.Item class="px-2 py-1 text-muted" onSelect={viewAllBranches}>
          View All Branches…
        </DropdownMenu.Item>
      </div>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
