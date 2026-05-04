<!--
  @component
  StatusBarBranchPopover: quick-switch popover for the active project branch.
-->

<script lang="ts">
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { SearchInput } from '$lib/components/ui/input'
  import {
    requestGitBranchesFocus,
    setSidebarCollapsed,
    setSidebarView
  } from '$lib/features/app-shell/sidebar/state.svelte'
  import { GitBranchIcon } from '$lib/icons/lucideExports'
  import AheadBehindBadge from '$lib/features/git/AheadBehindBadge.svelte'
  import * as branches from '$lib/features/git/branches.svelte'
  import CheckoutDialog from '$lib/features/git/dialogs/CheckoutDialog.svelte'
  import type { BranchSummary, GitSummary } from '$lib/types/backend'
  import { timeAgo } from '$lib/utils/date'

  let {
    projectId,
    projectPath,
    children
  }: {
    projectId: string
    projectPath: string
    /** Snippet that renders the chunk visuals; passed in so the
     * trigger surface matches the existing branch chunk byte-for-byte. */
    children: import('svelte').Snippet
  } = $props()

  let open = $state(false)
  let search = $state('')

  let entry = $derived(branches.byProject.get(projectId))

  // Make sure the entry is loaded before the popover opens; the user
  // may click the chunk before any other view has loaded branches.
  $effect(() => {
    if (open && !entry) {
      void branches.loadFor(projectId, projectPath, { autoFetch: false })
    }
  })

  $effect(() => {
    if (!open || !entry || entry.opState === 'idle') return
    branches.pollOpState(projectId, projectPath)
    return () => branches.stopOpStatePolling(projectId)
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
      .toSorted(
        (a, b) => new Date(b.tip.date).getTime() - new Date(a.tip.date).getTime()
      )
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
      await branches.safeCheckout(projectId, projectPath, name)
    } catch (e) {
      if (branches.isDirtyCheckoutError(e)) {
        checkoutTarget = name
        checkoutSummary = e.summary
        checkoutOpen = true
      } else {
        console.error('Checkout failed:', e)
      }
    }
  }

  function viewAllBranches() {
    setSidebarCollapsed(false)
    setSidebarView('git')
    requestGitBranchesFocus(projectId)
    open = false
  }
</script>

<DropdownMenuRoot bind:open>
  <DropdownMenuTrigger
    class="cursor-pointer border-none bg-transparent p-0 text-left transition-colors hover:text-fg focus-visible:shadow-focus-ring focus-visible:outline-none"
  >
    {@render children()}
  </DropdownMenuTrigger>

  <DropdownMenuContent class="w-72 p-0" sideOffset={8} align="start">
    <div class="border-b border-edge p-2">
      <SearchInput
        bind:value={search}
        placeholder="Switch to branch…"
        class="text-sm"
        spellcheck={false}
        autofocus
      />
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
            <AheadBehindBadge ahead={branch.ahead} behind={branch.behind} size="xs" twoColor />
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

{#if checkoutOpen}
  <CheckoutDialog
    bind:open={checkoutOpen}
    branchName={checkoutTarget}
    summary={checkoutSummary}
    {projectId}
    {projectPath}
  />
{/if}
