<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { platform } from '$lib/platform'
  import type { TabId } from '$lib/features/workbench/model'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import GitContextMenu from '$lib/features/git/GitContextMenu.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import DropOverlay from '$lib/features/dnd/DropOverlay.svelte'
  import { gitChangeDragSource, gitDropZone, isGitDropZoneActive } from '$lib/features/dnd/adapters/git.svelte'
  import { Button, IconButton } from '$lib/components/ui/button'
  import { ButtonGroup } from '$lib/components/ui/button-group'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuSeparator,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { Textarea } from '$lib/components/ui/input'
  import TreeFilterInput from '$lib/components/file-tree/TreeFilterInput.svelte'
  import {
    ChevronDown,
    FileDiff,
    MinusCircle,
    PackageIcon,
    PlusCircle,
    SquareArrowOutUpRight,
    Trash2,
    Undo2Icon
  } from '$lib/icons/lucideExports'
  import { discardFiles, stageFiles, unstageFiles } from '$lib/features/git/state.svelte'
  import {
    commitFolder, discardAllFolder, fetchFolder, forcePushWithLease, pullFolder,
    pushFolder, stageAllFolder, stashAllFolder, undoLastCommit, unstageAllFolder
  } from '$lib/features/git/actions.svelte'
  import { openHeadSnapshot, openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { promoteTabWhenReady } from '$lib/features/workbench/state.svelte'
  import type { GitChange, GitSummary } from '$lib/types/backend'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import { buildTreeFilter } from '$lib/utils/fileTreeFilter'
  import { resolveProjectFile, splitRemotePath } from '$lib/utils/paths'
  import { SvelteSet } from 'svelte/reactivity'

  let { summary, folderPath }: { summary: GitSummary; folderPath: string } = $props()
  let hasCommits = $derived(!!summary.branch)
  let commitMessage = $state('')
  let remoteFolder = $derived(splitRemotePath(folderPath))

  type GitTreeTargetType = 'file' | 'directory'
  type GitTreeActionKind = 'stage' | 'unstage' | 'discard'

  interface GitTreeTarget {
    path: string
    type: GitTreeTargetType
  }

  let collapsedDirs = new SvelteSet<string>()

  let contextFilePath = $state<string | null>(null)
  let contextTargetType = $state<GitTreeTargetType | null>(null)
  let contextIsStaged = $state(false)
  let contextCanOpenFile = $state(false)
  let pendingOpenedTab = $state<Promise<TabId> | null>(null)

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

  let stagedFiles = $derived(summary.changes.filter((c) => c.staged))
  let unstagedFiles = $derived(summary.changes.filter((c) => !c.staged))

  let stagedTree = $derived(buildFileTree(stagedFiles))
  let unstagedTree = $derived(buildFileTree(unstagedFiles))
  let hasChanges = $derived(stagedTree.length > 0 || unstagedTree.length > 0)

  // Tree filter shared across both Staged and Changes groups. The
  // input below the commit area drives a single query; each group
  // computes its own match/expand sets so dimming and auto-expand
  // stay correct when the same path appears in only one group.
  let filterQuery = $state('')
  let filterActive = $derived(filterQuery.trim().length > 0)
  let stagedFilter = $derived(buildTreeFilter(stagedTree, filterQuery))
  let unstagedFilter = $derived(buildTreeFilter(unstagedTree, filterQuery))

  let canCommit = $derived(commitMessage.trim().length > 0 && stagedFiles.length > 0)

  function handleCommit() {
    if (!canCommit) return
    void commitFolder(folderPath, commitMessage.trim())
    commitMessage = ''
  }

  async function handleUndoLastCommit() {
    const message = await undoLastCommit(folderPath)
    if (typeof message === 'string' && message.length > 0) commitMessage = message
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault()
      handleCommit()
    }
  }

  function handleContextMenu(_e: MouseEvent, node: FileTreeNode<GitChange>, staged: boolean) {
    contextFilePath = node.change?.path ?? node.path
    contextTargetType = node.type
    contextIsStaged = staged
    contextCanOpenFile = canOpenActualFile(node)
  }

  function resetContextTarget() {
    contextFilePath = null
    contextTargetType = null
    contextIsStaged = false
    contextCanOpenFile = false
  }

  function getFilesUnderPath(dirPath: string, staged: boolean): string[] {
    const changes = staged ? stagedFiles : unstagedFiles
    return changes.filter((c) => c.path.startsWith(dirPath + '/')).map((c) => c.path)
  }

  function makeTreeTarget(node: FileTreeNode<GitChange>): GitTreeTarget {
    return {
      path: node.change?.path ?? node.path,
      type: node.type
    }
  }

  function canOpenActualFile(node: FileTreeNode<GitChange>): boolean {
    return node.type === 'file' && node.change?.status !== 'D'
  }

  function describeFileCount(count: number): string {
    return `${count} file${count === 1 ? '' : 's'}`
  }

  function getActionFiles(target: GitTreeTarget, action: GitTreeActionKind): string[] {
    if (target.type === 'file') return [target.path]
    return getFilesUnderPath(target.path, action === 'unstage')
  }

  function describeActionTarget(target: GitTreeTarget, action: GitTreeActionKind): string {
    if (target.type === 'file') return target.path
    const count = getActionFiles(target, action).length
    return `${describeFileCount(count)} under ${target.path}`
  }

  async function queueDiscard(target: GitTreeTarget): Promise<void> {
    if (getActionFiles(target, 'discard').length === 0) return
    const proceed = await confirmAsync({
      title: 'Revert Changes?',
      message: `Revert unstaged changes for ${describeActionTarget(target, 'discard')}? Tracked edits cannot be recovered; untracked files move to the trash.`,
      confirmLabel: 'Revert'
    })
    if (!proceed) return
    await runTreeAction('discard', target)
  }

  async function confirmDiscardAll(): Promise<void> {
    const proceed = await confirmAsync({
      title: 'Discard All Changes?',
      message: 'Discard all unstaged changes? Tracked edits cannot be recovered; untracked files move to the trash.',
      confirmLabel: 'Discard All'
    })
    if (!proceed) return
    await discardAllFolder(folderPath)
  }


  async function openActualFile(filePath: string) {
    try {
      await openTextFile(folderPath, filePath, { temporary: false })
    } catch (e) {
      notify.error('Open file failed', getErrorMessage(e))
    }
  }

  function handleCtxOpenFile() {
    if (!contextFilePath) return
    void openActualFile(contextFilePath)
  }

  function handleCtxOpenChanges() {
    if (!contextFilePath) return

    if (contextTargetType === 'file') {
      openWorkingTreeDiff(folderPath, contextIsStaged, contextFilePath, contextFilePath, { temporary: false })
      return
    }

    if (contextTargetType === 'directory') {
      openWorkingTreeDiff(folderPath, contextIsStaged, contextFilePath, null, { temporary: false })
    }
  }

  function handleCtxOpenFileHead() {
    if (!contextFilePath) return
    openHeadSnapshot(folderPath, contextFilePath)
  }

  function handleCtxStage() {
    if (!contextFilePath || !contextTargetType) return
    void runTreeAction('stage', { path: contextFilePath, type: contextTargetType })
  }

  function handleCtxUnstage() {
    if (!contextFilePath || !contextTargetType) return
    void runTreeAction('unstage', { path: contextFilePath, type: contextTargetType })
  }

  function handleCtxDiscard() {
    if (!contextFilePath || !contextTargetType) return
    queueDiscard({ path: contextFilePath, type: contextTargetType })
  }

  async function runTreeAction(action: GitTreeActionKind, target: GitTreeTarget) {
    const files = getActionFiles(target, action)
    if (files.length === 0) return
    const description = describeActionTarget(target, action)

    try {
      switch (action) {
        case 'stage':
          await stageFiles(folderPath, files)
          break
        case 'unstage':
          await unstageFiles(folderPath, files)
          break
        case 'discard':
          if (!(await discardFiles(folderPath, files))) return
          break
      }
      notify.success(
        action === 'discard' ? 'Changes reverted' : action === 'stage' ? 'Changes staged' : 'Changes unstaged',
        description
      )
    } catch (e) {
      notify.error(
        action === 'discard' ? 'Revert failed' : action === 'stage' ? 'Stage failed' : 'Unstage failed',
        getErrorMessage(e)
      )
    }
  }

  async function handleCtxReveal() {
    const native = platform.native
    if (!contextFilePath || remoteFolder || !native) return
    const absPath = resolveProjectFile(folderPath, contextFilePath)
    await native.files.reveal(absPath)
  }

  async function handleCtxCopyPath() {
    if (!contextFilePath) return
    const absPath = resolveProjectFile(folderPath, contextFilePath)
    await platform.clipboard.writeText(splitRemotePath(absPath)?.path ?? absPath)
  }

  async function handleCtxCopyRelativePath() {
    if (!contextFilePath) return
    await platform.clipboard.writeText(contextFilePath)
  }

  async function handleCtxCopyPatch() {
    if (!contextFilePath) return
    try {
      const patch = await backend.git.getPathPatch(folderPath, [contextFilePath], contextIsStaged)
      if (patch) await platform.clipboard.writeText(patch)
      else
        notify.info('No diff to copy', `${contextFilePath} has no ${contextIsStaged ? 'staged' : 'unstaged'} changes.`)
    } catch (e) {
      notify.error('Copy patch failed', getErrorMessage(e))
    }
  }

  async function handleCtxCopyFolderPatch() {
    if (!contextFilePath) return
    const files = getFilesUnderPath(contextFilePath, contextIsStaged)
    if (files.length === 0) return
    try {
      const patch = await backend.git.getPathPatch(folderPath, files, contextIsStaged)
      if (patch) await platform.clipboard.writeText(patch)
    } catch (e) {
      notify.error('Copy folder patch failed', getErrorMessage(e))
    }
  }

  async function handleCtxCopyFullPatch() {
    try {
      const patch = await backend.git.getFullPatch(folderPath)
      if (patch) await platform.clipboard.writeText(patch)
      else notify.info('No diff to copy', 'No changes in the working tree.')
    } catch (e) {
      notify.error('Copy patch failed', getErrorMessage(e))
    }
  }

  function gitSourceAttachment(node: FileTreeNode<GitChange>, staged: boolean) {
    const hasChanges =
      node.type === 'file' ? node.change !== undefined : getFilesUnderPath(node.path, staged).length > 0
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
          : getFilesUnderPath(node.path, staged).map((path) => ({ path, staged }))
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
          await (targetStaged ? stageFiles(folderPath, files) : unstageFiles(folderPath, files))
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
  <div class="border-b border-edge px-2.5 py-2">
    <Textarea rows={2} placeholder="Commit message..." bind:value={commitMessage} onkeydown={handleKeydown} />
    <div class="mt-1.5">
      <ButtonGroup class="w-full">
        <Button variant="default" size="sm" class="flex-1 rounded" disabled={!canCommit} onclick={handleCommit}>
          Commit{stagedFiles.length > 0 ? ` (${stagedFiles.length})` : ''}
        </Button>
        <DropdownMenuRoot>
          <DropdownMenuTrigger
            data-slot="button"
            class="flex items-center rounded border border-edge bg-raised px-1 py-1 text-muted transition-colors hover:border-accent hover:text-bright"
          >
            <ChevronDown size={11} />
          </DropdownMenuTrigger>
          <DropdownMenuContent class="min-w-[180px] text-sm">
            <DropdownMenuItem onclick={() => pullFolder(folderPath)}>Pull</DropdownMenuItem>
            <DropdownMenuItem onclick={() => pushFolder(folderPath)}>Push</DropdownMenuItem>
            <DropdownMenuItem onclick={() => fetchFolder(folderPath)}>Fetch</DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onclick={() => forcePushWithLease(folderPath)}>Force Push (with lease)</DropdownMenuItem>
            {#if hasCommits}
              <DropdownMenuSeparator />
              <DropdownMenuItem destructive onclick={handleUndoLastCommit}>Undo Last Commit</DropdownMenuItem>
            {/if}
          </DropdownMenuContent>
        </DropdownMenuRoot>
      </ButtonGroup>
    </div>
  </div>

  {#if hasChanges}
    <TreeFilterInput bind:value={filterQuery} placeholder="Filter changes..." ariaLabel="Filter changed files" />
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

  {#snippet fileGroup(label: string, tree: FileTreeNode<GitChange>[], keySuffix: string, isStaged: boolean, count: number)}
    {#if hasChanges}
      <div class="relative py-1" {@attach gitZoneAttachment(isStaged)}>
        {#snippet rowActions(node: FileTreeNode<GitChange>)}
          {@const target = makeTreeTarget(node)}
          {#if !isStaged}
            <IconButton
              tooltip="Revert changes"
              tone="danger"
              onclick={() => queueDiscard(target)}
            >
              <Undo2Icon size={13} />
            </IconButton>
          {/if}
          {#if isStaged}
            <IconButton
              tooltip="Unstage changes"
              onclick={() => void runTreeAction('unstage', target)}
            >
              <MinusCircle size={13} />
            </IconButton>
          {:else}
            <IconButton
              tooltip="Stage changes"
              onclick={() => void runTreeAction('stage', target)}
            >
              <PlusCircle size={13} />
            </IconButton>
          {/if}
          {#if node.type === 'file' && canOpenActualFile(node)}
            <IconButton
              tooltip="Open actual file"
              onclick={() => void openActualFile(target.path)}
            >
              <SquareArrowOutUpRight size={13} />
            </IconButton>
          {/if}
        {/snippet}

        <div class="group/hdr relative flex items-center px-2.5 py-1">
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
                <IconButton tooltip="Stash all" onclick={() => stashAllFolder(folderPath)}>
                  <PackageIcon size={13} />
                </IconButton>
              {/if}
              {#if !isStaged}
                <IconButton
                  tooltip="Discard all changes"
                  class="rounded p-0.5 text-muted hover:text-danger"
                  onclick={confirmDiscardAll}
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
          {@const filter = isStaged ? stagedFilter : unstagedFilter}
          <FileTreeItems
            nodes={tree}
            isCollapsed={(node) => {
              if (filterActive && filter.shouldExpand(node)) return false
              return collapsedDirs.has(getDirKey(keySuffix, node.path))
            }}
            isDimmed={filterActive ? (node) => !filter.isMatch(node) : undefined}
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

  <GitContextMenu
    filePath={contextFilePath}
    targetType={contextTargetType}
    isStaged={contextIsStaged}
    canOpenFile={contextCanOpenFile}
    canRevealInFileManager={!remoteFolder && platform.native !== null}
    onOpenChanges={handleCtxOpenChanges}
    onOpenFile={handleCtxOpenFile}
    onOpenFileHead={handleCtxOpenFileHead}
    onStage={handleCtxStage}
    onUnstage={handleCtxUnstage}
    onDiscard={handleCtxDiscard}
    onRevealInFolder={handleCtxReveal}
    onCopyPath={handleCtxCopyPath}
    onCopyRelativePath={handleCtxCopyRelativePath}
    onCopyPatch={handleCtxCopyPatch}
    onCopyFolderPatch={handleCtxCopyFolderPatch}
    onPush={() => pushFolder(folderPath)}
    onPull={() => pullFolder(folderPath)}
    onFetch={() => fetchFolder(folderPath)}
    onCopyFullPatch={handleCtxCopyFullPatch}
    onResetTarget={resetContextTarget}
  >
    {#if !hasChanges}
      <div class="px-2.5 py-2 text-sm text-subtle">No changes.</div>
    {/if}

    {@render fileGroup('Staged', stagedTree, 'staged', true, stagedFiles.length)}
    {@render fileGroup('Changes', unstagedTree, 'unstaged', false, unstagedFiles.length)}
  </GitContextMenu>
</div>

