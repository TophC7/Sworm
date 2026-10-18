<script lang="ts">
  import type { TabId } from '$lib/features/workbench/model'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import GitContextMenu from '$lib/features/git/GitContextMenu.svelte'
  import GitCommitArea from '$lib/features/git/GitCommitArea.svelte'
  import GitOperationStrip from '$lib/features/git/GitOperationStrip.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import DropOverlay from '$lib/features/dnd/DropOverlay.svelte'
  import { gitChangeDragSource, gitDropZone, isGitDropZoneActive } from '$lib/features/dnd/adapters/git.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import {
    Check,
    FileDiff,
    MinusCircle,
    PackageIcon,
    PlusCircle,
    SquareArrowOutUpRight,
    Trash2,
    Undo2Icon
  } from '$lib/icons/lucideExports'
  import { stageFiles, unstageFiles } from '$lib/features/git/state.svelte'
  import { markResolved, stageAllFolder, stashFolder, unstageAllFolder } from '$lib/features/git/actions.svelte'
  import {
    confirmDiscardAll,
    confirmDiscardPaths,
    describeFolder,
    openActualFile,
    runPathsAction,
    type GitTreeMenuTarget
  } from '$lib/features/git/treeActions'
  import { getGitTreeReveal } from '$lib/features/git/treeReveal.svelte'
  import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { getActivationSeq, promoteTabWhenReady } from '$lib/features/workbench/state.svelte'
  import type { GitChange, GitConflict, GitSummary } from '$lib/types/backend'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { buildFileTree, findTreeRowPath, type FileTreeNode } from '$lib/utils/fileTree'
  import { untrack } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'

  let { summary, folderPath }: { summary: GitSummary; folderPath: string } = $props()

  type GitTreeTargetType = 'file' | 'directory'
  type GitTreeActionKind = 'stage' | 'unstage' | 'discard'

  interface GitTreeTarget {
    path: string
    type: GitTreeTargetType
  }

  let collapsedDirs = new SvelteSet<string>()

  let menuTarget = $state<GitTreeMenuTarget | null>(null)
  let pendingOpenedTab = $state<Promise<TabId> | null>(null)
  let resolving = $state(false)

  const gitSourceAttachmentCache = new Map<string, ReturnType<typeof gitChangeDragSource>>()
  const gitZoneAttachmentCache = new Map<string, ReturnType<typeof gitDropZone>>()

  function getDirKey(section: string, path: string): string {
    return `${section}:${path}`
  }

  function toggleDir(section: string, path: string) {
    const key = getDirKey(section, path)
    if (collapsedDirs.has(key)) collapsedDirs.delete(key)
    else collapsedDirs.add(key)
  }

  const conflictPaths = $derived(new Set(summary.conflicts.map((conflict) => conflict.path)))
  let stagedFiles = $derived(summary.changes.filter((change) => change.staged && !conflictPaths.has(change.path)))
  let unstagedFiles = $derived(summary.changes.filter((change) => !change.staged && !conflictPaths.has(change.path)))

  let stagedTree = $derived(buildFileTree(stagedFiles))
  let unstagedTree = $derived(buildFileTree(unstagedFiles))
  let conflictsTree = $derived(buildFileTree<GitConflict>(summary.conflicts))
  let hasChanges = $derived(summary.conflicts.length > 0 || stagedTree.length > 0 || unstagedTree.length > 0)

  const reveal = $derived.by(() => {
    const r = getGitTreeReveal(folderPath)
    return r && r.activation === getActivationSeq() ? r : null
  })
  const revealSection = $derived.by<'merge' | 'unstaged' | 'staged' | null>(() => {
    if (!reveal) return null
    const path = reveal.path
    const within = (p: string) => p === path || p.startsWith(path + '/')
    if (summary.conflicts.some((c) => within(c.path))) return 'merge'
    if (unstagedFiles.some((c) => within(c.path))) return 'unstaged'
    if (stagedFiles.some((c) => within(c.path))) return 'staged'
    return null
  })
  const revealRow = $derived.by<string | null>(() => {
    if (!reveal || !revealSection) return null
    const tree = revealSection === 'merge' ? conflictsTree : revealSection === 'staged' ? stagedTree : unstagedTree
    return findTreeRowPath<{ path: string }>(tree, reveal.path)
  })
  // A separate derived so the object stays the same across summary refreshes:
  // FileTreeItems re-scrolls on every new request object.
  const revealScroll = $derived(reveal && revealRow ? { path: revealRow, seq: reveal.seq } : null)

  $effect(() => {
    const row = revealRow
    const section = revealSection
    if (!row || !section) return
    untrack(() => {
      const parts = row.split('/')
      for (let i = 1; i < parts.length; i++) {
        collapsedDirs.delete(getDirKey(section, parts.slice(0, i).join('/')))
      }
    })
  })

  function handleContextMenu(_e: MouseEvent, node: FileTreeNode<GitChange>, staged: boolean) {
    const section = staged ? 'staged' : 'changes'
    if (node.type === 'file' && node.change) {
      menuTarget = { kind: 'file', section, change: node.change }
    } else if (node.type === 'directory') {
      menuTarget = {
        kind: 'folder',
        section,
        path: node.path,
        files: getFilesUnderPath(node.path, staged ? stagedFiles : unstagedFiles)
      }
    }
  }

  function resetContextTarget() {
    menuTarget = null
  }

  function getFilesUnderPath(dirPath: string, files: { path: string }[]): string[] {
    return files.filter((file) => file.path.startsWith(dirPath + '/')).map((file) => file.path)
  }

  function handleConflictContextMenu(_event: MouseEvent, node: FileTreeNode<GitConflict>) {
    if (node.type === 'file' && node.change) {
      menuTarget = { kind: 'conflict', conflict: node.change }
    } else if (node.type === 'directory') {
      menuTarget = {
        kind: 'folder',
        section: 'merge',
        path: node.path,
        files: getFilesUnderPath(node.path, summary.conflicts)
      }
    }
  }

  async function resolveConflicts(files: string[]) {
    if (resolving) return
    resolving = true
    try {
      await markResolved(folderPath, files)
    } finally {
      resolving = false
    }
  }

  function openConflict(node: FileTreeNode<GitConflict>) {
    if (!node.change || node.change.kind === 'both_deleted') {
      pendingOpenedTab = null
      return
    }
    pendingOpenedTab = openTextFile(folderPath, node.change.path, { temporary: true })
    void pendingOpenedTab.catch((error) => notify.error('Open file failed', getErrorMessage(error)))
  }

  function makeTreeTarget(node: FileTreeNode<GitChange>): GitTreeTarget {
    return {
      path: node.change?.path ?? node.path,
      type: node.type
    }
  }

  function getActionFiles(target: GitTreeTarget, action: GitTreeActionKind): string[] {
    if (target.type === 'file') return [target.path]
    return getFilesUnderPath(target.path, action === 'unstage' ? stagedFiles : unstagedFiles)
  }

  function gitSourceAttachment(node: FileTreeNode<GitChange>, staged: boolean) {
    const hasChanges =
      node.type === 'file'
        ? node.change !== undefined
        : getFilesUnderPath(node.path, staged ? stagedFiles : unstagedFiles).length > 0
    if (!hasChanges) return null
    const key = `${folderPath}:${staged ? 'staged' : 'unstaged'}:${node.type}:${node.path}`
    const cached = gitSourceAttachmentCache.get(key)
    if (cached) return cached
    const attachment = gitChangeDragSource({
      folderPath,
      changes: () =>
        node.type === 'file'
          ? node.change
            ? [node.change]
            : []
          : getFilesUnderPath(node.path, staged ? stagedFiles : unstagedFiles).map((path) => ({ path, staged }))
    })
    gitSourceAttachmentCache.set(key, attachment)
    return attachment
  }

  function gitZoneAttachment(staged: boolean) {
    const key = `${folderPath}:${staged ? 'staged' : 'unstaged'}`
    const cached = gitZoneAttachmentCache.get(key)
    if (cached) return cached
    const attachment = gitDropZone({
      folderPath,
      staged,
      onDropFiles: async (files, targetStaged) => {
        try {
          if (targetStaged) {
            if (!(await stageFiles(folderPath, files))) return
          } else {
            await unstageFiles(folderPath, files)
          }
          notify.success(
            targetStaged ? 'Files staged' : 'Files unstaged',
            `${files.length} file${files.length === 1 ? '' : 's'}`
          )
        } catch (error) {
          notify.error('Git drop failed', getErrorMessage(error))
        }
      }
    })
    gitZoneAttachmentCache.set(key, attachment)
    return attachment
  }
</script>

<div class="flex min-h-full flex-col text-base">
  <GitCommitArea {summary} {folderPath} />
  {#if summary.operation}
    <GitOperationStrip {folderPath} operation={summary.operation} />
  {/if}

  {#snippet fileTrailing(node: FileTreeNode<GitChange>)}
    {#if node.change}
      {#if node.change.status !== 'D' && (node.change.additions != null || node.change.deletions != null)}
        {@const net = (node.change.additions ?? 0) - (node.change.deletions ?? 0)}
        {#if net !== 0}
          <span class="shrink-0 font-mono {net > 0 ? 'text-success' : 'text-danger'}">
            {net > 0 ? '+' : ''}{net}
          </span>
        {:else}
          <GitStatusBadge status={node.change.status} />
        {/if}
      {:else}
        <GitStatusBadge status={node.change.status} />
      {/if}
    {/if}
  {/snippet}

  {#snippet fileGroup(
    label: string,
    tree: FileTreeNode<GitChange>[],
    keySuffix: string,
    isStaged: boolean,
    count: number
  )}
    {#if hasChanges}
      <div class="relative py-1" {@attach gitZoneAttachment(isStaged)}>
        {#snippet rowActions(node: FileTreeNode<GitChange>)}
          {@const target = makeTreeTarget(node)}
          {@const action = isStaged ? 'unstage' : 'stage'}
          {@const files = getActionFiles(target, action)}
          {@const description = target.type === 'file' ? target.path : describeFolder(target.path, files.length)}
          {#if !isStaged}
            <IconButton
              tooltip="Revert changes"
              tone="danger"
              onclick={() => void confirmDiscardPaths(folderPath, files, description)}
            >
              <Undo2Icon size={13} />
            </IconButton>
          {/if}
          {#if isStaged}
            <IconButton
              tooltip="Unstage changes"
              onclick={() => void runPathsAction(folderPath, 'unstage', files, description)}
            >
              <MinusCircle size={13} />
            </IconButton>
          {:else}
            <IconButton
              tooltip="Stage changes"
              onclick={() => void runPathsAction(folderPath, 'stage', files, description)}
            >
              <PlusCircle size={13} />
            </IconButton>
          {/if}
          {#if node.type === 'file' && node.change?.status !== 'D'}
            <IconButton tooltip="Open actual file" onclick={() => void openActualFile(folderPath, target.path)}>
              <SquareArrowOutUpRight size={13} />
            </IconButton>
          {/if}
        {/snippet}

        <div
          role="group"
          aria-label={label}
          class="group/hdr relative flex items-center px-2.5 py-1"
          oncontextmenu={() =>
            (menuTarget = {
              kind: 'section',
              section: isStaged ? 'staged' : 'changes',
              files: (isStaged ? stagedFiles : unstagedFiles).map((change) => change.path)
            })}
        >
          <span class="text-xs font-medium tracking-wide text-muted uppercase">
            {label} ({count})
          </span>
          {#if count > 0}
            <div class="ml-auto flex items-center gap-0.5 opacity-0 transition-all group-hover/hdr:opacity-100">
              {#if isStaged}
                <IconButton tooltip="Unstage all" onclick={() => unstageAllFolder(folderPath)}>
                  <MinusCircle size={13} />
                </IconButton>
              {/if}
              {#if !isStaged}
                <IconButton tooltip="Stage all" onclick={() => stageAllFolder(folderPath)}>
                  <PlusCircle size={13} />
                </IconButton>
              {/if}
              {#if !isStaged}
                <IconButton
                  tooltip="Stash all"
                  onclick={() => stashFolder(folderPath, { kind: 'all', includeUntracked: true })}
                >
                  <PackageIcon size={13} />
                </IconButton>
              {/if}
              {#if !isStaged}
                <IconButton
                  tooltip="Discard all changes"
                  class="rounded p-0.5 text-muted hover:text-danger"
                  onclick={() => void confirmDiscardAll(folderPath)}
                >
                  <Trash2 size={13} />
                </IconButton>
              {/if}
              <IconButton
                tooltip="View all {label.toLowerCase()} diffs"
                onclick={() => openWorkingTreeDiff(folderPath, isStaged, null, null, { temporary: false })}
              >
                <FileDiff size={13} />
              </IconButton>
            </div>
          {/if}
          <DropOverlay visible={isGitDropZoneActive(folderPath, isStaged)} label={isStaged ? 'Stage' : 'Unstage'} />
        </div>
        {#if tree.length > 0}
          <FileTreeItems
            nodes={tree}
            isCollapsed={(node) => collapsedDirs.has(getDirKey(keySuffix, node.path))}
            isActive={(path) => revealSection === keySuffix && path === revealRow}
            scrollRequest={revealSection === keySuffix ? revealScroll : null}
            onToggleDir={(path) => toggleDir(keySuffix, path)}
            onFileClick={(node) => {
              if (!node.change) return
              pendingOpenedTab = openWorkingTreeDiff(folderPath, node.change.staged, null, node.change.path)
            }}
            onFileDblClick={() => promoteTabWhenReady(pendingOpenedTab)}
            onFileContextMenu={(e, node) => handleContextMenu(e, node, isStaged)}
            {fileTrailing}
            {rowActions}
            dndSourceAttachment={(node) => gitSourceAttachment(node, isStaged)}
          />
        {:else}
          <div class="px-2.5 pt-2">
            <div
              class="rounded-xl border-2 border-dashed px-3 py-3 text-sm transition-colors {isGitDropZoneActive(
                folderPath,
                isStaged
              )
                ? 'border-accent bg-accent/10 text-bright'
                : 'border-edge bg-ground/50 text-subtle'}"
            >
              {isStaged ? 'Drop files here to stage them.' : 'Drop files here to unstage them.'}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  {/snippet}

  {#snippet mergeGroup()}
    <div class="py-1">
      <div
        role="group"
        aria-label="Merge Changes"
        class="group/hdr relative flex items-center px-2.5 py-1"
        oncontextmenu={() =>
          (menuTarget = {
            kind: 'section',
            section: 'merge',
            files: summary.conflicts.map((conflict) => conflict.path)
          })}
      >
        <span class="text-xs font-medium tracking-wide text-muted uppercase">
          Merge Changes ({summary.conflicts.length})
        </span>
        <div
          class="ml-auto flex items-center gap-0.5 opacity-0 group-hover/hdr:opacity-100 group-has-[:focus-visible]/hdr:opacity-100"
        >
          <IconButton
            tooltip="Mark all as resolved"
            disabled={resolving}
            onclick={() => void resolveConflicts(summary.conflicts.map((conflict) => conflict.path))}
          >
            <Check size={13} />
          </IconButton>
        </div>
      </div>
      {#snippet conflictTrailing(node: FileTreeNode<GitConflict>)}
        {#if node.change}
          <GitStatusBadge status={node.change.kind} />
        {/if}
      {/snippet}
      {#snippet conflictActions(node: FileTreeNode<GitConflict>)}
        <IconButton
          tooltip={node.type === 'directory' ? 'Mark folder resolved' : 'Mark resolved'}
          disabled={resolving}
          onclick={() =>
            void resolveConflicts(node.change ? [node.change.path] : getFilesUnderPath(node.path, summary.conflicts))}
        >
          <PlusCircle size={13} />
        </IconButton>
        {#if node.change && node.change.kind !== 'both_deleted'}
          <IconButton tooltip="Open actual file" onclick={() => void openActualFile(folderPath, node.path)}>
            <SquareArrowOutUpRight size={13} />
          </IconButton>
        {/if}
      {/snippet}
      <FileTreeItems
        nodes={conflictsTree}
        isCollapsed={(node) => collapsedDirs.has(getDirKey('merge', node.path))}
        isActive={(path) => revealSection === 'merge' && path === revealRow}
        scrollRequest={revealSection === 'merge' ? revealScroll : null}
        onToggleDir={(path) => toggleDir('merge', path)}
        onFileClick={openConflict}
        onFileDblClick={(node) => {
          if (node.change?.kind !== 'both_deleted') promoteTabWhenReady(pendingOpenedTab)
        }}
        onFileContextMenu={handleConflictContextMenu}
        fileTrailing={conflictTrailing}
        rowActions={conflictActions}
      />
    </div>
  {/snippet}

  <GitContextMenu {folderPath} {summary} target={menuTarget} onResetTarget={resetContextTarget}>
    {#if !hasChanges}
      <div class="px-2.5 py-2 text-sm text-subtle">No changes.</div>
    {/if}

    {#if summary.conflicts.length > 0}
      {@render mergeGroup()}
    {/if}
    {@render fileGroup('Staged', stagedTree, 'staged', true, stagedFiles.length)}
    {@render fileGroup('Changes', unstagedTree, 'unstaged', false, unstagedFiles.length)}
  </GitContextMenu>
</div>
