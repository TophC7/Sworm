<!--
  @component
  IssuesSidebar. Epic-grouped tree.

  Top level is one collapsible section per epic (header · EPIC id ·
  title · mini progress bar). Inside an expanded epic, top-level issues
  render as rows; sub-issues nest one indent deeper and are always
  visible (no chevron). File-tree-style indent guides give the only
  visual nesting cue; rows have no borders.

  Click rules (mirroring git branches/stashes):
    - Click an epic header  → toggle collapse / expand.
    - Double-click epic     → open the epic in a workbench tab.
    - Click an issue or sub-issue → open it in a workbench tab.

  Each row also has a tooltip (status / priority / age) and a context
  menu (open, claim, change status, copy id, delete).
-->

<script lang="ts">
  import { untrack, type Component } from 'svelte'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import { IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { InfoTooltip, TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'
  import { ToggleChip } from '$lib/components/ui/toggle-chip'
  import { ProgressBar } from '$lib/components/ui/progress'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuSeparator,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import {
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuRoot,
    ContextMenuSeparator,
    ContextMenuTrigger
  } from '$lib/components/ui/context-menu'
  import {
    ArchiveIcon,
    Check,
    Circle,
    CircleCheck,
    CircleDot,
    CircleSlash,
    ClipboardIcon,
    Filter,
    Layers,
    Plus,
    RefreshCwIcon,
    Trash2,
    X
  } from '$lib/icons/lucideExports'
  import IssueQuickCapture from '$lib/features/issues/IssueQuickCapture.svelte'
  import { ALL_PRIORITIES, priorityToneClass, statusGlyphTone, statusRowLabel } from '$lib/features/issues/visual'
  import { hasToken, parseQuery, toggleToken } from '$lib/features/issues/query'
  import { groupIssuesByEpic, type EpicGroup, type SortMode } from '$lib/features/issues/groups'
  import {
    claimIssue,
    createEpic,
    createIssue,
    deleteEpic,
    deleteIssue,
    getIssueEpics,
    getIssues,
    getIssuesError,
    isIssuesLoading,
    loadIssues,
    updateEpic,
    updateIssue
  } from '$lib/features/issues/state.svelte'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import { formatFullDate, timeAgo } from '$lib/utils/date'
  import { treeIndent, TreeIndentGuides } from '$lib/components/ui/tree-indent'
  import { SidebarRow, sidebarRowVariants } from '$lib/components/ui/sidebar-row'
  import { openIssueTab } from '$lib/features/workbench/surfaces/issue/service.svelte'
  import { openEpicTab } from '$lib/features/workbench/surfaces/epic/service.svelte'
  import type { Issue, IssueEpic, IssueStatus } from '$lib/types/backend'

  type CaptureMode = 'issue' | 'epic'

  const SORT_LABEL: Record<SortMode, string> = {
    'priority:asc': 'Priority — highest first',
    'updated:desc': 'Updated — newest first',
    'created:desc': 'Created — newest first',
    'created:asc': 'Created — oldest first',
    'title:asc': 'Title — A→Z'
  }
  const SORT_MODES = Object.keys(SORT_LABEL) as SortMode[]

  // Row glyphs only; tones come from visual.ts so the sidebar and
  // editor surface can't drift apart.
  const STATUS_GLYPH: Record<IssueStatus, Component> = {
    todo: Circle,
    in_progress: CircleDot,
    blocked: CircleSlash,
    completed: CircleCheck,
    wont_fix: X,
    archived: ArchiveIcon
  }

  const STATUS_TOKENS: ReadonlyArray<readonly [string, string]> = [
    ['is:open', 'Open'],
    ['is:active', 'Active'],
    ['is:done', 'Done'],
    ['is:archived', 'Archived']
  ]

  // Quick-status menu items shown on issue context menus. Order
  // matches the sidebar's status priority: active work first.
  const STATUS_MENU: ReadonlyArray<readonly [IssueStatus, string]> = [
    ['in_progress', 'Mark in progress'],
    ['todo', 'Mark todo'],
    ['blocked', 'Mark blocked'],
    ['completed', 'Mark done'],
    ['wont_fix', "Won't fix"],
    ['archived', 'Archive']
  ]

  let { projectId }: { projectId: string } = $props()

  let issues = $derived(getIssues(projectId))
  let epics = $derived(getIssueEpics(projectId))
  let loading = $derived(isIssuesLoading(projectId))
  let error = $derived(getIssuesError(projectId))

  // `query` is the single source of truth for what's visible. The
  // filter dropdown writes literal DSL tokens into this string;
  // nothing else owns visibility state.
  let query = $state('')
  let sortMode = $state<SortMode>('updated:desc')
  let captureOpen = $state(false)
  let captureMode = $state<CaptureMode>('issue')
  let captureEpicId = $state('')
  // Epics default expanded; entries here are the user-collapsed ones.
  let collapsedEpics = $state<Set<string>>(new Set())

  // `loadIssues` reads + writes the same `loadingByProject` / `errorByProject`
  // / `issuesByProject` $state via `setMapValue(map, ...)` (which clones from
  // the existing map). Without `untrack`, those reads make the effect
  // self-retrigger.
  $effect(() => {
    const id = projectId
    untrack(() => void loadIssues(id))
  })

  async function refresh() {
    await loadIssues(projectId)
  }

  function openCapture(mode: CaptureMode, epicId?: string) {
    captureMode = mode
    if (mode === 'issue') {
      captureEpicId = epicId ?? captureEpicId ?? epics[0]?.id ?? ''
    }
    captureOpen = true
  }

  async function commitCapture(title: string) {
    if (captureMode === 'epic') {
      const epic = await createEpic(projectId, title)
      if (epic) captureEpicId = epic.id
    } else {
      const issue = await createIssue(projectId, title, captureEpicId)
      if (issue) await openIssueTab(projectId, issue.id, issue.title)
    }
    captureOpen = false
  }

  function preventClose(e?: Event) {
    e?.preventDefault?.()
  }

  function toggleEpic(id: string) {
    const next = new Set(collapsedEpics)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    collapsedEpics = next
  }

  function flipToken(token: string) {
    query = toggleToken(query, token)
  }

  let terms = $derived(parseQuery(query.trim()))
  let groups = $derived(groupIssuesByEpic(issues, epics, terms, sortMode))
  let totalIssueCount = $derived(issues.length)

  async function copyId(id: string) {
    try {
      await copyToClipboard(id)
    } catch (e) {
      console.error('Copy id failed:', e)
    }
  }

  async function setStatus(issueId: string, status: IssueStatus) {
    try {
      await updateIssue(projectId, issueId, { status })
    } catch (e) {
      console.error('Update status failed:', e)
    }
  }

  async function archiveEpic(epic: IssueEpic) {
    try {
      await updateEpic(projectId, epic.id, { status: 'archived' })
    } catch (e) {
      console.error('Archive epic failed:', e)
    }
  }

  async function confirmDeleteEpic(epic: IssueEpic) {
    const ok = await confirmAsync({
      title: `Delete epic ${epic.id}?`,
      message: `Removes the epic "${epic.title}". Issues stay but become unassigned.`,
      confirmLabel: 'Delete'
    })
    if (!ok) return
    try {
      await deleteEpic(projectId, epic.id)
    } catch (e) {
      console.error('Delete epic failed:', e)
    }
  }

  async function confirmDeleteIssue(issue: Issue) {
    const ok = await confirmAsync({
      title: `Delete issue ${issue.id}?`,
      message: `Removes "${issue.title}" and any sub-issues. This can't be undone.`,
      confirmLabel: 'Delete'
    })
    if (!ok) return
    try {
      await deleteIssue(projectId, issue.id)
    } catch (e) {
      console.error('Delete issue failed:', e)
    }
  }
</script>

<SidebarPanel title="Issues">
  {#snippet headerActions()}
    <IconButton tooltip="Refresh issues" onclick={refresh}>
      <RefreshCwIcon size={11} />
    </IconButton>
  {/snippet}
  {#snippet headerExtra()}
    <InfoTooltip ariaLabel="About the issues view" contentClass="w-80">
      <div class="space-y-2">
        <p class="font-medium text-bright">Issues view</p>
        <p>
          Issues live under epics. Click an epic to collapse it; double-click to open it. Click an issue to open it as
          an editor tab. Right-click any row for actions.
        </p>
        <p class="font-medium text-bright">Search syntax</p>
        <ul class="space-y-1">
          <li><span class="font-mono text-accent">free text</span> — matches id or title</li>
          <li>
            <span class="font-mono text-accent">is:open</span>
            <span class="text-muted">/</span>
            <span class="font-mono text-accent">is:active</span>
            <span class="text-muted">/</span>
            <span class="font-mono text-accent">is:done</span>
            <span class="text-muted">/</span>
            <span class="font-mono text-accent">is:archived</span>
          </li>
          <li>
            <span class="font-mono text-accent">p:0</span>
            <span class="text-muted">or</span>
            <span class="font-mono text-accent">p:0..2</span> — priority range
          </li>
          <li><span class="font-mono text-accent">epic:abc</span> — match by epic id</li>
        </ul>
      </div>
    </InfoTooltip>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col bg-ground">
    <IssueQuickCapture
      open={captureOpen}
      mode={captureMode}
      {epics}
      bind:epicId={captureEpicId}
      onSubmit={commitCapture}
      onCancel={() => (captureOpen = false)}
    />

    <div class="flex shrink-0 items-center gap-1.5 border-b border-edge px-2 py-1.5">
      <div class="relative min-w-0 flex-1">
        <Input
          bind:value={query}
          placeholder="is:open p:0..2"
          aria-label="Filter issues"
          class="h-7 py-1 pr-7 text-sm"
          spellcheck={false}
        />
        <DropdownMenuRoot>
          <DropdownMenuTrigger
            class={iconButtonVariants({ active: false }) + ' absolute top-1/2 right-0.5 size-6 -translate-y-1/2'}
            aria-label="Filter options"
          >
            <Filter size={12} />
          </DropdownMenuTrigger>
          <DropdownMenuContent class="min-w-[230px] text-sm" align="end">
            <div class="px-3 py-1 text-xs font-medium tracking-wide text-muted uppercase">Status</div>
            {#each STATUS_TOKENS as [token, label] (token)}
              {@const on = hasToken(query, token)}
              <DropdownMenuItem
                class="flex items-center gap-2"
                onclick={(e) => {
                  preventClose(e)
                  flipToken(token)
                }}
              >
                <Check size={14} class="shrink-0 {on ? 'text-accent' : 'text-transparent'}" />
                <span class="font-mono text-2xs text-muted">{token}</span>
                <span>{label}</span>
              </DropdownMenuItem>
            {/each}

            <DropdownMenuSeparator />

            <div class="px-3 py-1 text-xs font-medium tracking-wide text-muted uppercase">Priority</div>
            <div class="flex items-center gap-1 px-3 py-1">
              {#each ALL_PRIORITIES as p (p)}
                {@const token = `p:${p}`}
                <ToggleChip class="w-7" pressed={hasToken(query, token)} onclick={() => flipToken(token)}>
                  P{p}
                </ToggleChip>
              {/each}
            </div>

            <DropdownMenuSeparator />

            <div class="px-3 py-1 text-xs font-medium tracking-wide text-muted uppercase">Sort</div>
            {#each SORT_MODES as mode (mode)}
              {@const on = sortMode === mode}
              <DropdownMenuItem
                class="flex items-center gap-2"
                onclick={(e) => {
                  preventClose(e)
                  sortMode = mode
                }}
              >
                <Check size={14} class="shrink-0 {on ? 'text-accent' : 'text-transparent'}" />
                <span>{SORT_LABEL[mode]}</span>
              </DropdownMenuItem>
            {/each}
          </DropdownMenuContent>
        </DropdownMenuRoot>
      </div>
    </div>

    <SidebarRow
      variant="action"
      divider={false}
      class="h-7 shrink-0 border-b border-edge bg-ground"
      onclick={() => openCapture('epic')}
    >
      <Layers size={12} class="shrink-0" />
      <span>New epic…</span>
      <Plus size={12} class="ml-auto shrink-0" />
    </SidebarRow>

    <div class="min-h-0 flex-1 overflow-y-auto text-base">
      {#if loading && totalIssueCount === 0 && epics.length === 0}
        <div class="px-2.5 py-3 text-sm text-subtle">Loading issues…</div>
      {:else if error}
        <div class="px-2.5 py-3 text-sm text-danger">{error}</div>
      {:else}
        <TooltipProvider delayDuration={400} skipDelayDuration={100}>
          {#each groups as group (group.epic?.id ?? '__orphans')}
            {@render epicSection(group)}
          {/each}
        </TooltipProvider>
      {/if}
    </div>
  </div>
</SidebarPanel>

<!--
  Indent guides; same recipe as the file tree so both sidebar trees
  read the same way.
-->
{#snippet indentGuides(depth: number)}
  <TreeIndentGuides {depth} />
{/snippet}

{#snippet epicTooltip(epic: IssueEpic, group: EpicGroup)}
  <div class="flex max-w-90 flex-col gap-2 py-0.5">
    <div class="flex items-center gap-1.5">
      <Layers size={10} class="text-warning" />
      <span class="min-w-0 truncate font-mono text-sm text-warning">{epic.id}</span>
      <span class="shrink-0 rounded bg-surface px-1 py-px text-2xs text-muted uppercase">epic</span>
    </div>
    <p class="text-sm leading-snug text-fg">{epic.title}</p>
    {#if epic.description}
      <p class="line-clamp-3 text-xs leading-snug text-muted">{epic.description}</p>
    {/if}
    <div class="flex items-center gap-2 text-xs">
      <span class="text-muted">Status</span>
      <span class="text-fg">{statusRowLabel(epic.status as IssueStatus)}</span>
      <span class="text-subtle">·</span>
      <span class="text-muted">P{epic.priority}</span>
    </div>
    <div class="flex items-center gap-2 text-xs">
      <span class="text-success">{group.done} done</span>
      <span class="text-accent">{group.active} active</span>
      <span class="text-subtle">/ {group.total} total</span>
    </div>
    <div class="text-2xs text-subtle">
      Updated {timeAgo(epic.updatedAt)}
      <span class="text-edge">·</span>
      {formatFullDate(epic.updatedAt)}
    </div>
    <div class="text-2xs text-subtle">Click to collapse · double-click to open</div>
  </div>
{/snippet}

{#snippet issueTooltip(issue: Issue, parent: Issue | null)}
  <div class="flex max-w-90 flex-col gap-2 py-0.5">
    <div class="flex items-center gap-1.5">
      <span class="min-w-0 truncate font-mono text-sm text-subtle">{issue.id}</span>
      <span class="shrink-0 rounded bg-surface px-1 py-px text-2xs text-muted uppercase">
        {parent ? 'sub-issue' : 'issue'}
      </span>
    </div>
    <p class="text-sm leading-snug text-fg">{issue.title}</p>
    {#if issue.description}
      <p class="line-clamp-3 text-xs leading-snug text-muted">{issue.description}</p>
    {/if}
    <div class="flex items-center gap-2 text-xs">
      <span class="text-muted">Status</span>
      <span class={statusGlyphTone(issue.status)}>{statusRowLabel(issue.status)}</span>
      <span class="text-subtle">·</span>
      <span class="font-mono {priorityToneClass(issue.priority)}">P{issue.priority}</span>
    </div>
    {#if issue.tags.length > 0}
      <div class="flex flex-wrap gap-1 text-2xs">
        {#each issue.tags as tag (tag)}
          <span class="rounded bg-surface px-1 py-px text-muted">{tag}</span>
        {/each}
      </div>
    {/if}
    {#if parent}
      <div class="text-2xs text-subtle">
        <span class="text-muted">Parent</span>
        <span class="ml-1 font-mono">{parent.id}</span>
      </div>
    {/if}
    <div class="text-2xs text-subtle">
      Updated {timeAgo(issue.updatedAt)}
      <span class="text-edge">·</span>
      {formatFullDate(issue.updatedAt)}
    </div>
  </div>
{/snippet}

{#snippet epicSection(group: EpicGroup)}
  {@const id = group.epic?.id ?? '__orphans'}
  {@const collapsed = collapsedEpics.has(id)}
  {@const epic = group.epic}
  {#if epic}
    <ContextMenuRoot>
      <ContextMenuTrigger class="contents">
        <TooltipRoot>
          <TooltipTrigger
            type="button"
            class={sidebarRowVariants({ variant: 'section' })}
            onclick={() => toggleEpic(id)}
            ondblclick={() => void openEpicTab(projectId, epic.id, epic.title)}
            aria-expanded={!collapsed}
          >
            <Layers size={12} class="shrink-0 text-warning" />
            <span class="shrink-0 font-mono text-2xs text-warning">{epic.id}</span>
            <span class="min-w-0 flex-1 truncate text-fg">{epic.title}</span>
            <ProgressBar done={group.done} active={group.active} total={group.total} />
          </TooltipTrigger>
          <TooltipContent class="max-w-90" sideOffset={6} side="right" align="center">
            {@render epicTooltip(epic, group)}
          </TooltipContent>
        </TooltipRoot>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onclick={() => void openEpicTab(projectId, epic.id, epic.title)}>
          <Layers size={14} class="shrink-0 text-muted" />
          <span>Open epic</span>
        </ContextMenuItem>
        <ContextMenuItem onclick={() => openCapture('issue', epic.id)}>
          <Plus size={14} class="shrink-0 text-muted" />
          <span>New issue in epic…</span>
        </ContextMenuItem>
        <ContextMenuSeparator />
        {#if epic.status !== 'archived'}
          <ContextMenuItem onclick={() => void archiveEpic(epic)}>
            <ArchiveIcon size={14} class="shrink-0 text-muted" />
            <span>Archive epic</span>
          </ContextMenuItem>
        {/if}
        <ContextMenuItem onclick={() => void copyId(epic.id)}>
          <ClipboardIcon size={14} class="shrink-0 text-muted" />
          <span>Copy id</span>
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem destructive onclick={() => void confirmDeleteEpic(epic)}>
          <Trash2 size={14} class="shrink-0" />
          <span>Delete epic…</span>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenuRoot>
  {:else}
    <SidebarRow variant="section" onclick={() => toggleEpic(id)} aria-expanded={!collapsed}>
      <Layers size={12} class="shrink-0 text-warning" />
      <span class="min-w-0 flex-1 truncate text-subtle italic">Unassigned</span>
      <ProgressBar done={group.done} active={group.active} total={group.total} />
    </SidebarRow>
  {/if}

  {#if !collapsed}
    <div class="border-t border-edge/30 bg-surface py-1">
      {#each group.rows as row (row.issue.id)}
        {@render issueRow(row.issue, null, 0)}
        {#each row.subs as sub (sub.id)}
          {@render issueRow(sub, row.issue, 1)}
        {/each}
      {/each}
      {#if epic}
        <SidebarRow variant="action" on="surface" divider={false} onclick={() => openCapture('issue', epic.id)}>
          <span>New issue…</span>
          <Plus size={12} class="ml-auto shrink-0" />
        </SidebarRow>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet issueRow(issue: Issue, parent: Issue | null, depth: number)}
  {@const StatusIcon = STATUS_GLYPH[issue.status]}
  {@const tone = statusGlyphTone(issue.status)}
  <ContextMenuRoot>
    <ContextMenuTrigger class="contents">
      <TooltipRoot>
        <TooltipTrigger
          type="button"
          class="group/row relative flex h-6 w-full cursor-pointer items-center gap-1.5 pr-2.5 text-left text-sm transition-colors hover:bg-raised focus-visible:shadow-focus-ring focus-visible:outline-none"
          style="padding-left: {treeIndent(depth)}"
          onclick={() => void openIssueTab(projectId, issue.id, issue.title)}
        >
          {@render indentGuides(depth)}
          <StatusIcon size={12} class="shrink-0 {tone}" />
          <span class="shrink-0 font-mono text-2xs text-subtle">{issue.id}</span>
          <span class="min-w-0 flex-1 truncate text-fg group-hover/row:text-bright">{issue.title}</span>
          <span class="shrink-0 font-mono text-2xs {priorityToneClass(issue.priority)}">P{issue.priority}</span>
        </TooltipTrigger>
        <TooltipContent class="max-w-90" sideOffset={6} side="right" align="center">
          {@render issueTooltip(issue, parent)}
        </TooltipContent>
      </TooltipRoot>
    </ContextMenuTrigger>
    <ContextMenuContent>
      <ContextMenuItem onclick={() => void openIssueTab(projectId, issue.id, issue.title)}>
        <CircleDot size={14} class="shrink-0 text-muted" />
        <span>Open issue</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={() => void claimIssue(projectId, issue.id)}>
        <Check size={14} class="shrink-0 text-muted" />
        <span>Claim</span>
      </ContextMenuItem>
      <ContextMenuSeparator />
      {#each STATUS_MENU as [status, label] (status)}
        {#if issue.status !== status}
          <ContextMenuItem onclick={() => void setStatus(issue.id, status)}>
            <span class="shrink-0 {statusGlyphTone(status)}">●</span>
            <span>{label}</span>
          </ContextMenuItem>
        {/if}
      {/each}
      <ContextMenuSeparator />
      <ContextMenuItem onclick={() => void copyId(issue.id)}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy id</span>
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem destructive onclick={() => void confirmDeleteIssue(issue)}>
        <Trash2 size={14} class="shrink-0" />
        <span>Delete issue…</span>
      </ContextMenuItem>
    </ContextMenuContent>
  </ContextMenuRoot>
{/snippet}
