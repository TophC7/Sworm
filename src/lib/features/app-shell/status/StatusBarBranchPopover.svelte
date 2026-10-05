<!--
  @component
  StatusBarBranchPopover: current-branch status chip with a quick-switch popover.
-->

<script lang="ts">
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { SearchInput } from '$lib/components/ui/input'
  import { statusChipVariants } from '$lib/components/ui/status-chip'
  import { revealGitBranches } from '$lib/features/app-shell/sidebar/state.svelte'
  import { GitBranchIcon } from '$lib/icons/lucideExports'
  import AheadBehindBadge from '$lib/features/git/AheadBehindBadge.svelte'
  import * as branches from '$lib/features/git/branches.svelte'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import CheckoutDialog from '$lib/features/git/dialogs/CheckoutDialog.svelte'
  import type { BranchSummary, GitSummary } from '$lib/types/backend'
  import { timeAgo } from '$lib/utils/date'

  let { folderPath }: { folderPath: string } = $props()

  let gitSummary = $derived(getGitSummary(folderPath))
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

  $effect(() => {
    if (!open || !entry || entry.opState === 'idle') return
    branches.pollOpState(folderPath)
    return () => branches.stopOpStatePolling(folderPath)
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

  // Checkout dialog state when the user picks a branch on a dirty tree.
  let checkoutOpen = $state(false)
  let checkoutTarget = $state<string>('')
  let checkoutSummary = $state<GitSummary | null>(null)

  async function selectBranch(name: string) {
    open = false
    try {
      await branches.safeCheckout(folderPath, name)
    } catch (e) {
      if (e instanceof branches.DirtyCheckoutError) {
        checkoutTarget = name
        checkoutSummary = e.summary
        checkoutOpen = true
      } else {
        console.error('Checkout failed:', e)
      }
    }
  }

  function viewAllBranches() {
    revealGitBranches(folderPath)
    open = false
  }
</script>

{#if gitSummary?.branch}
  <DropdownMenuRoot bind:open>
    <DropdownMenuTrigger
      class={statusChipVariants({ tone: branchDirty ? 'warning' : 'success', class: 'font-mono' })}
      title={branchStatus}
    >
      <GitBranchIcon size={10} />
      {gitSummary.branch}
      <AheadBehindBadge ahead={gitSummary.ahead ?? 0} behind={gitSummary.behind ?? 0} twoColor />
    </DropdownMenuTrigger>

    <DropdownMenuContent class="w-72 p-0" sideOffset={8} align="start">
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
            <DropdownMenuItem
              class="flex items-center gap-2 py-1 text-sm"
              onclick={() => void selectBranch(branch.name)}
            >
              <GitBranchIcon size={12} class="shrink-0 text-muted" />
              <span class="min-w-0 flex-1 truncate font-mono">{branch.name}</span>
              <AheadBehindBadge ahead={branch.ahead} behind={branch.behind} twoColor />
              <span class="shrink-0 text-2xs text-subtle">{timeAgo(branch.tip.date)}</span>
            </DropdownMenuItem>
          {/each}
        {/if}
      </div>

      <div class="border-t border-edge p-1">
        <DropdownMenuItem class="px-2 py-1 text-sm text-muted" onclick={viewAllBranches}>
          View all branches…
        </DropdownMenuItem>
      </div>
    </DropdownMenuContent>
  </DropdownMenuRoot>
{/if}

{#if checkoutOpen}
  <CheckoutDialog bind:open={checkoutOpen} branchName={checkoutTarget} summary={checkoutSummary} {folderPath} />
{/if}
