<!--
  @component
  GitBranches: local + remote branch list with branch actions.
-->

<script lang="ts" module>
  import { treeIndent, treeIndentValue } from '$lib/components/ui/tree-indent'
  import { SidebarRow, sidebarRowVariants } from '$lib/components/ui/sidebar-row'

  const BRANCH_HISTORY_PAGE_SIZE = 5
</script>

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { SearchInput } from '$lib/components/ui/input'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import * as branches from '$lib/features/git/branches.svelte'
  import AheadBehindBadge from '$lib/features/git/AheadBehindBadge.svelte'
  import { openFolder } from '$lib/features/workbench/state.svelte'
  import { fastForwardBranch, fetchFolder, pullFolder, pushFolder, rebaseOnto } from '$lib/features/git/actions.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { groupBySlash, type BranchTreeNode } from '$lib/utils/git/branchHierarchy'
  import { basename } from '$lib/utils/paths'
  import { localNameForRemoteRef } from '$lib/features/git/gitRefs'
  import GitCommitRow from '$lib/features/git/GitCommitRow.svelte'
  import { GRAPH_COLORS } from '$lib/features/git/graph'
  import { getGitBranchesFocusRequest } from '$lib/features/app-shell/sidebar/state.svelte'
  import CheckoutDialog from '$lib/features/git/dialogs/CheckoutDialog.svelte'
  import CreateBranchDialog from '$lib/features/git/dialogs/CreateBranchDialog.svelte'
  import CreateWorktreeDialog from '$lib/features/git/dialogs/CreateWorktreeDialog.svelte'
  import RenameBranchDialog from '$lib/features/git/dialogs/RenameBranchDialog.svelte'
  import DeleteBranchDialog from '$lib/features/git/dialogs/DeleteBranchDialog.svelte'
  import SetUpstreamDialog from '$lib/features/git/dialogs/SetUpstreamDialog.svelte'
  import MergeConfirmDialog from '$lib/features/git/dialogs/MergeConfirmDialog.svelte'
  import CompareBranchModal from '$lib/features/git/dialogs/CompareBranchModal.svelte'
  import {
    ArrowDown01Icon,
    ArrowDownAZIcon,
    ArrowUpFromLineIcon,
    ArrowDownToLineIcon,
    Check,
    ClipboardIcon,
    ClockIcon,
    CloudIcon,
    CloudOffIcon,
    FolderOpenIcon,
    GitBranchIcon,
    GitCompareIcon,
    GitMergeIcon,
    GitPullRequestIcon,
    ListIcon,
    ListTreeIcon,
    MoreHorizontalIcon,
    PencilIcon,
    Plus,
    RefreshCwIcon,
    RotateCw,
    Trash2
  } from '$lib/icons/lucideExports'
  import type { BranchSummary, CommitDetail, GitSummary, GitWorktreeCheckout, GraphCommit } from '$lib/types/backend'
  import { platform } from '$lib/platform'
  import { formatFullDate, timeAgo } from '$lib/utils/date'
  import { SvelteMap } from 'svelte/reactivity'

  let {
    folderPath,
    branchColorMap
  }: {
    folderPath: string
    branchColorMap?: Map<string, string>
  } = $props()

  let entry = $derived(branches.byFolder.get(folderPath))
  let prefs = $derived(entry?.prefs)

  let search = $state('')
  let searchInputEl = $state<HTMLInputElement | null>(null)

  let checkoutOpen = $state(false)
  let checkoutTarget = $state<string>('')
  let checkoutRemoteTarget = $state<string | null>(null)
  let checkoutSummary = $state<GitSummary | null>(null)
  let checkoutFolderPath = $state('')

  let createOpen = $state(false)
  let createBase = $state<string>('')

  let createWorktreeOpen = $state(false)
  let worktreeCheckout = $state<GitWorktreeCheckout>({ kind: 'new_branch', name: '', base: 'HEAD' })

  let renameOpen = $state(false)
  let renameTarget = $state<string>('')

  let deleteOpen = $state(false)
  let deleteBranch = $state<BranchSummary | null>(null)

  let upstreamOpen = $state(false)
  let upstreamTarget = $state<string>('')
  let upstreamInitial = $state<string>('')

  let mergeOpen = $state(false)
  let mergeSource = $state<string>('')

  let compareOpen = $state(false)
  let compareTarget = $state<string>('')

  type BranchHistoryEntry = {
    commits: GraphCommit[]
    limit: number
    loading: boolean
    exhausted: boolean
    error: string | null
  }

  let expandedBranch = $state<string | null>(null)
  let historyByBranch = new SvelteMap<string, BranchHistoryEntry>()
  let commitDetailCache = new SvelteMap<string, CommitDetail>()

  // Mount: load entry. Don't depend on `entry` here; the load
  // populates it.
  $effect(() => {
    void branches.loadFor(folderPath)
  })

  async function runFetch() {
    try {
      await branches.fetchBranches(folderPath)
    } catch (e) {
      console.error('Fetch failed:', e)
    }
  }

  // SORT + FILTER //

  function compareDate(a: BranchSummary, b: BranchSummary) {
    return new Date(b.tip.date).getTime() - new Date(a.tip.date).getTime()
  }

  function compareAlpha(a: BranchSummary, b: BranchSummary) {
    return a.name.localeCompare(b.name)
  }

  function matchesSearch(branch: BranchSummary, q: string): boolean {
    if (!q) return true
    return branch.name.toLowerCase().includes(q)
  }

  let filtered = $derived.by(() => {
    if (!entry)
      return { current: null as BranchSummary | null, locals: [] as BranchSummary[], remotes: [] as BranchSummary[] }
    const q = search.trim().toLowerCase()
    const sorter = prefs?.sort === 'alpha' ? compareAlpha : compareDate
    let current: BranchSummary | null = null
    const locals: BranchSummary[] = []
    const remotes: BranchSummary[] = []
    for (const b of entry.list) {
      if (!matchesSearch(b, q)) continue
      if (b.isCurrent) current = b
      else if (b.kind === 'local') locals.push(b)
      else remotes.push(b)
    }
    locals.sort(sorter)
    remotes.sort(sorter)
    return { current, locals, remotes }
  })

  let localTree = $derived.by(() => {
    if (prefs?.layout !== 'tree') return null
    return groupBySlash(filtered.locals)
  })

  // MUTATIONS //

  async function attemptCheckout(name: string) {
    if (!name) return
    const path = folderPath
    try {
      await branches.safeCheckout(path, name)
    } catch (e) {
      if (e instanceof branches.DirtyCheckoutError) {
        checkoutFolderPath = path
        checkoutTarget = name
        checkoutRemoteTarget = null
        checkoutSummary = e.summary
        checkoutOpen = true
        return
      }
      console.error('Checkout failed:', e)
    }
  }

  function toggleLayout() {
    branches.setLayout(folderPath, prefs?.layout === 'tree' ? 'list' : 'tree')
  }

  function toggleSort() {
    branches.setSort(folderPath, prefs?.sort === 'alpha' ? 'date' : 'alpha')
  }

  function toggleShowRemote() {
    branches.setShowRemote(folderPath, !prefs?.showRemote)
  }

  function graphColor(branch: BranchSummary): string | null {
    return branchColorMap?.get(branch.name) ?? (branch.upstream ? branchColorMap?.get(branch.upstream) : null) ?? null
  }

  function branchColor(branch: BranchSummary): string {
    return graphColor(branch) ?? GRAPH_COLORS[0]
  }

  async function fetchCommitDetail(hash: string): Promise<CommitDetail | null> {
    const cached = commitDetailCache.get(hash)
    if (cached) return cached
    const path = folderPath
    const detail = await backend.git.getCommitDetail(path, hash)
    if (path === folderPath && detail) commitDetailCache.set(hash, detail)
    return detail
  }

  function prefetchCommitDetail(hash: string) {
    if (!commitDetailCache.has(hash)) void fetchCommitDetail(hash)
  }

  async function loadBranchHistory(branch: BranchSummary, limit: number) {
    const requestLimit = limit + 1
    const existing = historyByBranch.get(branch.name)
    historyByBranch.set(branch.name, {
      commits: existing?.commits ?? [],
      limit,
      loading: true,
      exhausted: existing?.exhausted ?? false,
      error: null
    })

    try {
      const path = folderPath
      const commits = await backend.git.getBranchCommits(path, branch.name, requestLimit)
      if (path !== folderPath) return
      historyByBranch.set(branch.name, {
        commits: commits.slice(0, limit),
        limit,
        loading: false,
        exhausted: commits.length <= limit,
        error: null
      })
    } catch (e) {
      historyByBranch.set(branch.name, {
        commits: existing?.commits ?? [],
        limit,
        loading: false,
        exhausted: existing?.exhausted ?? false,
        error: getErrorMessage(e)
      })
    }
  }

  function toggleBranchHistory(branch: BranchSummary) {
    if (expandedBranch === branch.name) {
      expandedBranch = null
      return
    }
    expandedBranch = branch.name
    const existing = historyByBranch.get(branch.name)
    if (!existing) void loadBranchHistory(branch, BRANCH_HISTORY_PAGE_SIZE)
  }

  let lastFocusRequest = $state(0)
  $effect(() => {
    const req = getGitBranchesFocusRequest(folderPath)
    if (req <= lastFocusRequest) return
    lastFocusRequest = req
    queueMicrotask(() => searchInputEl?.focus())
  })

  // Currently checked-out branch. Used for merge / rebase target
  // labels and as the default base for "+New branch".
  let currentName = $derived(entry?.list.find((b) => b.isCurrent)?.name ?? '')

  function openCreateDialog(base?: string) {
    createBase = base ?? currentName ?? 'HEAD'
    createOpen = true
  }

  function openCreateWorktreeDialog(branch: BranchSummary) {
    worktreeCheckout =
      branch.kind === 'local' && !branch.isCurrent && branch.worktreePath === null
        ? { kind: 'existing_branch', name: branch.name }
        : { kind: 'new_branch', name: '', base: branch.name }
    createWorktreeOpen = true
  }

  function openRenameDialog(branch: BranchSummary) {
    renameTarget = branch.name
    renameOpen = true
  }

  function openDeleteDialog(branch: BranchSummary) {
    deleteBranch = branch
    deleteOpen = true
  }

  function openUpstreamDialog(branch: BranchSummary) {
    upstreamTarget = branch.name
    const suggestions = entry?.list.filter((b) => b.kind === 'remote').map((b) => b.name) ?? []
    upstreamInitial = suggestions.find((s) => s.endsWith(`/${branch.name}`)) ?? suggestions[0] ?? ''
    upstreamOpen = true
  }

  function openMergeDialog(branch: BranchSummary) {
    mergeSource = branch.name
    mergeOpen = true
  }

  async function openRebaseDialog(branch: BranchSummary) {
    const target = branch.name
    const current = currentName
    const proceed = await confirmAsync({
      title: `Rebase ${current} onto ${target}?`,
      message: `Replays your ${current} commits on top of ${target}. Rebasing rewrites history.`,
      confirmLabel: 'Rebase'
    })
    if (!proceed) return

    await rebaseOnto(folderPath, target)
  }

  function openCompareModal(branch: BranchSummary) {
    compareTarget = branch.name
    compareOpen = true
  }

  // Smart action: smartAction(branch) returns push / pull / fetch /
  // disabled depending on counters and upstream availability.
  type SmartAction = { kind: 'push' | 'pull' | 'fetch' | 'disabled'; label: string }

  function smartAction(branch: BranchSummary): SmartAction {
    if (branch.kind === 'remote') return { kind: 'disabled', label: 'Remote-only branch' }
    if (!branch.upstream) return { kind: 'disabled', label: 'No upstream configured' }
    if (branch.behind > 0 && branch.ahead === 0) return { kind: 'pull', label: `Pull (${branch.behind})` }
    if (branch.ahead > 0 && branch.behind === 0) return { kind: 'push', label: `Push (${branch.ahead})` }
    return { kind: 'fetch', label: 'Fetch' }
  }

  async function runSmartAction(branch: BranchSummary) {
    const action = smartAction(branch)
    if (action.kind === 'disabled') return
    if (action.kind === 'push') {
      await pushFolder(folderPath, branch.name)
    } else if (action.kind === 'pull' && branch.isCurrent) {
      await pullFolder(folderPath)
    } else if (action.kind === 'pull') {
      await fastForwardBranch(folderPath, branch.name)
    } else if (action.kind === 'fetch') {
      await fetchFolder(folderPath)
    }
  }

  async function copyName(name: string) {
    try {
      await platform.clipboard.writeText(name)
    } catch (e) {
      console.error('Copy branch name failed:', e)
    }
  }

  function switchToBranch(branch: BranchSummary) {
    if (branch.kind === 'remote') void checkoutRemote(branch)
    else void attemptCheckout(branch.name)
  }

  async function checkoutRemote(branch: BranchSummary) {
    if (branch.kind !== 'remote') return
    const path = folderPath
    const localName = localNameForRemoteRef(branch.name)
    try {
      await branches.safeCheckoutRemoteAsLocal(path, branch.name, localName)
    } catch (e) {
      if (e instanceof branches.DirtyCheckoutError) {
        checkoutFolderPath = path
        checkoutTarget = localName
        checkoutRemoteTarget = branch.name
        checkoutSummary = e.summary
        checkoutOpen = true
        return
      }
      console.error('Remote checkout failed:', e)
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col overflow-hidden text-base">
  <div class="flex shrink-0 items-center gap-1.5 border-b border-edge px-2 py-1.5">
    <div class="min-w-0 flex-1">
      <SearchInput
        bind:value={search}
        bind:ref={searchInputEl}
        placeholder="Filter branches…"
        aria-label="Filter branches"
        class="h-7 py-1 text-sm"
        spellcheck={false}
      />
    </div>

    <DropdownMenu.Root>
      <DropdownMenu.Trigger class={iconButtonVariants()} aria-label="Branch actions">
        <MoreHorizontalIcon size={13} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content class="min-w-[210px]" align="end">
        <DropdownMenu.Item onSelect={() => openCreateDialog()}>
          <Plus size={14} />
          <span>New branch…</span>
        </DropdownMenu.Item>
        <DropdownMenu.Item disabled={entry?.fetching === true} onSelect={() => void runFetch()}>
          <RefreshCwIcon size={14} />
          <span>Fetch from remotes</span>
        </DropdownMenu.Item>

        <DropdownMenu.Separator />

        <DropdownMenu.Item onSelect={toggleLayout}>
          {#if prefs?.layout === 'tree'}
            <ListIcon size={14} />
            <span>Switch to flat list</span>
          {:else}
            <ListTreeIcon size={14} />
            <span>Switch to tree</span>
          {/if}
        </DropdownMenu.Item>
        <DropdownMenu.Item onSelect={toggleSort}>
          {#if prefs?.sort === 'alpha'}
            <ArrowDown01Icon size={14} />
            <span>Sort by date</span>
          {:else}
            <ArrowDownAZIcon size={14} />
            <span>Sort alphabetically</span>
          {/if}
        </DropdownMenu.Item>
        <DropdownMenu.Item onSelect={toggleShowRemote}>
          {#if prefs?.showRemote}
            <CloudOffIcon size={14} />
            <span>Hide remote branches</span>
          {:else}
            <CloudIcon size={14} />
            <span>Show remote branches</span>
          {/if}
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if !entry}
      <div class="px-2.5 py-2 text-sm text-subtle">Loading branches…</div>
    {:else if entry.list.length === 0}
      <div class="px-2.5 py-2 text-sm text-subtle">No branches yet.</div>
    {:else}
      <Tooltip.Provider delayDuration={400} skipDelayDuration={100}>
        {#if filtered.current}
          <div class="relative py-1">
            {@render branchSectionHeader('Current')}
            {@render branchRow(filtered.current, 0)}
          </div>
        {/if}

        <div class="relative py-1">
          {@render branchSectionHeader('Local', filtered.locals.length)}
          {#if filtered.locals.length === 0}
            <div class="px-2.5 py-1 text-xs text-subtle">No matches.</div>
          {:else if prefs?.layout === 'tree' && localTree}
            {#each localTree as node (node.fullName)}
              {@render treeNode(node, 0)}
            {/each}
          {:else}
            {#each filtered.locals as branch (branch.name)}
              {@render branchRow(branch, 0)}
            {/each}
          {/if}
        </div>

        {#if prefs?.showRemote}
          <div class="relative py-1">
            {@render branchSectionHeader('Remote', filtered.remotes.length)}
            {#if filtered.remotes.length === 0}
              <div class="px-2.5 py-1 text-xs text-subtle">No remote branches.</div>
            {:else}
              {#each filtered.remotes as branch (branch.name)}
                {@render branchRow(branch, 0)}
              {/each}
            {/if}
          </div>
        {/if}
      </Tooltip.Provider>
    {/if}
  </div>
</div>

{#snippet branchTooltip(branch: BranchSummary, color: string)}
  {@const branchAge = timeAgo(branch.tip.date)}
  <div class="flex max-w-90 flex-col gap-2 py-0.5">
    <div class="flex items-center gap-1.5">
      <GitBranchIcon size={10} style="color: {color}" />
      <span class="min-w-0 truncate font-mono text-sm text-fg">{branch.name}</span>
      <span class="shrink-0 rounded bg-surface px-1 py-px text-2xs text-muted uppercase">{branch.kind}</span>
    </div>

    <p class="text-sm leading-snug text-fg">{branch.tip.subject}</p>

    <div class="flex items-center gap-1 text-xs">
      <span class="font-mono text-accent">{branch.tip.shortHash}</span>
      <span class="text-muted">{branch.tip.author}</span>
      <span class="text-subtle">,</span>
      <ClockIcon size={10} class="text-subtle" />
      <span class="text-muted">{branchAge}</span>
      <span class="text-subtle">({formatFullDate(branch.tip.date)})</span>
    </div>

    {#if branch.upstream}
      <div class="flex min-w-0 items-center gap-1 text-xs">
        <span class="text-subtle">Upstream</span>
        <span class="truncate font-mono text-muted">{branch.upstream}</span>
      </div>
    {:else if branch.kind === 'local'}
      <div class="text-xs text-subtle">No upstream configured.</div>
    {/if}

    {#if branch.worktreePath}
      <div class="space-y-1 text-xs">
        <div class="flex items-center gap-1 text-muted">
          <ListTreeIcon size={11} class={branch.isCurrent ? 'text-accent' : 'text-muted'} />
          <span>{branch.isCurrent ? 'Current worktree' : 'Checked out in another worktree'}</span>
        </div>
        <div class="font-mono break-all text-muted">{branch.worktreePath}</div>
      </div>
    {/if}

    {#if branch.ahead > 0 || branch.behind > 0}
      <div class="flex items-center gap-1.5 text-xs">
        {#if branch.ahead > 0}
          <span class="text-success">{branch.ahead} ahead</span>
        {/if}
        {#if branch.behind > 0}
          <span class="text-warning">{branch.behind} behind</span>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

{#snippet branchHistory(branch: BranchSummary, depth: number)}
  {@const history = historyByBranch.get(branch.name)}
  {@const color = branchColor(branch)}
  <div class="bg-surface">
    {#if history?.loading && history.commits.length === 0}
      <SidebarRow variant="info" divider={false} {depth} class="text-subtle">Loading commits...</SidebarRow>
    {:else if history?.error}
      <SidebarRow variant="info" divider={false} {depth} class="text-danger">
        {history.error}
      </SidebarRow>
    {:else if !history || history.commits.length === 0}
      <SidebarRow variant="info" divider={false} {depth} class="text-subtle">No commits found.</SidebarRow>
    {:else}
      {#each history.commits as commit (commit.hash)}
        <GitCommitRow
          {commit}
          detail={commitDetailCache.get(commit.hash) ?? null}
          graphColor={color}
          showRefs={false}
          indent={treeIndentValue(depth)}
          onPrefetch={prefetchCommitDetail}
        />
      {/each}

      {#if history.loading}
        <SidebarRow variant="info" divider={false} {depth} class="text-subtle">Loading commits...</SidebarRow>
      {:else if !history.exhausted}
        <SidebarRow
          variant="action"
          on="surface"
          {depth}
          class="text-xs"
          onclick={() => void loadBranchHistory(branch, history.limit + BRANCH_HISTORY_PAGE_SIZE)}
        >
          <MoreHorizontalIcon size={12} class="shrink-0" />
          <span>Show more</span>
        </SidebarRow>
      {/if}
    {/if}
  </div>
{/snippet}

{#snippet branchSectionHeader(label: string, count: number | null = null)}
  <div class="group/hdr relative flex items-center px-2.5 py-1">
    <span class="text-xs font-medium tracking-wide text-muted uppercase">
      {label}{count !== null && count > 0 ? ` (${count})` : ''}
    </span>
  </div>
{/snippet}

{#snippet treeNode(node: BranchTreeNode, depth: number)}
  {#if node.branch}
    {@render branchRow(node.branch, depth, node.isSelf ? '(self)' : null)}
  {:else}
    <SidebarRow variant="info" {depth}>
      <span class="truncate">{node.name}/</span>
    </SidebarRow>
  {/if}
  {#each node.children as child (child.fullName)}
    {@render treeNode(child, depth + 1)}
  {/each}
{/snippet}

{#snippet branchRow(branch: BranchSummary, depth: number, displayName: string | null = null)}
  {@const action = smartAction(branch)}
  {@const branchAge = timeAgo(branch.tip.date)}
  {@const color = branchColor(branch)}
  {@const isExpanded = expandedBranch === branch.name}
  <ContextMenu.Root>
    <ContextMenu.Trigger class="contents">
      <div
        class="{sidebarRowVariants({
          variant: 'leaf',
          pressed: branch.isCurrent || isExpanded
        })} group"
        style:padding-left={treeIndent(depth)}
      >
        <Tooltip.Root>
          <Tooltip.Trigger
            type="button"
            class="flex h-full min-w-0 flex-1 items-center gap-1.5 border-none bg-transparent p-0 text-left focus-visible:shadow-focus-ring focus-visible:outline-none"
            aria-label={`${branch.name} branch`}
            onclick={() => toggleBranchHistory(branch)}
          >
            <GitBranchIcon size={12} class={branch.isCurrent ? 'text-accent' : 'text-muted'} {color} />
            <span class="min-w-0 flex-1 truncate">
              {displayName ??
                (prefs?.layout === 'tree' && branch.kind === 'local' ? basename(branch.name) : branch.name)}
            </span>
            {#if branch.worktreePath}
              <ListTreeIcon
                size={11}
                class={branch.isCurrent ? 'shrink-0 text-accent' : 'shrink-0 text-muted'}
                aria-label={branch.isCurrent ? 'Current worktree' : 'Checked out elsewhere'}
              />
            {/if}

            <AheadBehindBadge ahead={branch.ahead} behind={branch.behind} />
          </Tooltip.Trigger>
          <Tooltip.Content class="max-w-90" sideOffset={6} side="right" align="center">
            {@render branchTooltip(branch, color)}
          </Tooltip.Content>
        </Tooltip.Root>

        <div class="relative h-full w-[68px] shrink-0">
          <span
            class="absolute inset-y-0 right-0 flex max-w-full items-center justify-end truncate text-2xs text-subtle group-hover:hidden"
          >
            {branchAge}
          </span>
          <div class="absolute inset-y-0 right-0 hidden items-center gap-0.5 group-hover:flex">
            <IconButton
              tooltip="Compare with HEAD"
              tooltipSide="left"
              onclick={(event) => {
                event.stopPropagation()
                openCompareModal(branch)
              }}
            >
              <GitCompareIcon size={12} />
            </IconButton>
            {#if action.kind !== 'disabled'}
              <IconButton
                tooltip={action.label}
                tooltipSide="left"
                onclick={(event) => {
                  event.stopPropagation()
                  void runSmartAction(branch)
                }}
              >
                {#if action.kind === 'push'}
                  <ArrowUpFromLineIcon size={12} />
                {:else if action.kind === 'pull'}
                  <ArrowDownToLineIcon size={12} />
                {:else}
                  <RotateCw size={12} />
                {/if}
              </IconButton>
            {/if}
            {#if !branch.isCurrent}
              <IconButton
                tooltip={branch.kind === 'remote' ? 'Check out remote branch' : 'Switch to branch'}
                tooltipSide="left"
                onclick={(event) => {
                  event.stopPropagation()
                  switchToBranch(branch)
                }}
              >
                <Check size={12} />
              </IconButton>
            {/if}
          </div>
        </div>
      </div>
    </ContextMenu.Trigger>

    <ContextMenu.Content>
      {#if !branch.isCurrent}
        <ContextMenu.Item onSelect={() => switchToBranch(branch)}>
          <Check size={14} />
          <span>Switch to branch</span>
        </ContextMenu.Item>
      {/if}
      {#if branch.worktreePath && !branch.isCurrent}
        <ContextMenu.Item onSelect={() => void openFolder(branch.worktreePath!)}>
          <FolderOpenIcon size={14} />
          <span>Open Worktree</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={() => openCompareModal(branch)}>
        <GitCompareIcon size={14} />
        <span>Compare with HEAD</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onSelect={() => openCreateDialog(branch.name)}>
        <Plus size={14} />
        <span>Create branch from here…</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={() => openCreateWorktreeDialog(branch)}>
        <ListTreeIcon size={14} />
        <span>Create Worktree…</span>
      </ContextMenu.Item>
      {#if branch.kind === 'local' && !branch.isCurrent}
        <ContextMenu.Item onSelect={() => openRenameDialog(branch)}>
          <PencilIcon size={14} />
          <span>Rename…</span>
        </ContextMenu.Item>
      {/if}
      {#if !branch.isCurrent}
        <ContextMenu.Item destructive onSelect={() => openDeleteDialog(branch)}>
          <Trash2 size={14} />
          <span>Delete…</span>
        </ContextMenu.Item>
      {/if}

      {#if !branch.isCurrent}
        <ContextMenu.Separator />
        <ContextMenu.Item onSelect={() => openMergeDialog(branch)}>
          <GitMergeIcon size={14} />
          <span>Merge into current</span>
        </ContextMenu.Item>
        <ContextMenu.Item onSelect={() => void openRebaseDialog(branch)}>
          <GitPullRequestIcon size={14} />
          <span>Rebase current onto this</span>
        </ContextMenu.Item>
      {/if}

      <ContextMenu.Separator />

      {#if branch.kind === 'local'}
        <ContextMenu.Item onSelect={() => openUpstreamDialog(branch)}>
          <CloudIcon size={14} />
          <span>Set upstream…</span>
        </ContextMenu.Item>
      {/if}
      {#if !branch.isCurrent && branch.behind > 0 && branch.upstream}
        <ContextMenu.Item onSelect={() => void fastForwardBranch(folderPath, branch.name)}>
          <ArrowDownToLineIcon size={14} />
          <span>Fast-forward</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={() => void copyName(branch.name)}>
        <ClipboardIcon size={14} />
        <span>Copy name</span>
      </ContextMenu.Item>
    </ContextMenu.Content>
  </ContextMenu.Root>

  {#if isExpanded}
    {@render branchHistory(branch, depth)}
  {/if}
{/snippet}

{#if checkoutOpen}
  <CheckoutDialog
    bind:open={checkoutOpen}
    branchName={checkoutTarget}
    remoteBranchName={checkoutRemoteTarget}
    summary={checkoutSummary}
    folderPath={checkoutFolderPath}
  />
{/if}

{#if createOpen}
  <CreateBranchDialog bind:open={createOpen} defaultBase={createBase} {folderPath} />
{/if}

{#if createWorktreeOpen}
  <CreateWorktreeDialog bind:open={createWorktreeOpen} initialCheckout={worktreeCheckout} {folderPath} />
{/if}

{#if renameOpen}
  <RenameBranchDialog bind:open={renameOpen} oldName={renameTarget} {folderPath} />
{/if}

{#if deleteOpen && deleteBranch}
  <DeleteBranchDialog bind:open={deleteOpen} branch={deleteBranch} {folderPath} />
{/if}

{#if upstreamOpen}
  <SetUpstreamDialog
    bind:open={upstreamOpen}
    branchName={upstreamTarget}
    initialUpstream={upstreamInitial}
    {folderPath}
  />
{/if}

{#if mergeOpen}
  <MergeConfirmDialog bind:open={mergeOpen} source={mergeSource} current={currentName} {folderPath} />
{/if}

{#if compareOpen}
  <CompareBranchModal bind:open={compareOpen} branchName={compareTarget} {folderPath} />
{/if}
