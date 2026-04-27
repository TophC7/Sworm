<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { TabId } from '$lib/features/workbench/model'
  import ConfirmDialog from '$lib/components/dialogs/ConfirmDialog.svelte'
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
  import { runGitAction } from '$lib/features/git/state.svelte'
  import { openHeadSnapshot, openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
  import type { GitChange, GitSummary } from '$lib/types/backend'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import { notify } from '$lib/features/notifications/state.svelte'

  function errMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e)
  }
  import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree'
  import { buildTreeFilter } from '$lib/utils/fileTreeFilter'
  import { join } from '@tauri-apps/api/path'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { SvelteSet } from 'svelte/reactivity'

  let {
    summary,
    projectId,
    projectPath,
    hasCommits = false,
    commitMessage = $bindable(''),
    onFileClick,
    onPersistTab,
    onViewAllChanges,
    onCommit,
    onStageAll,
    onUnstageAll,
    onDiscardAll,
    onStashAll,
    onUndoLastCommit,
    onPush,
    onPushForceWithLease,
    onPull,
    onFetch
  }: {
    summary: GitSummary
    projectId: string
    projectPath: string
    hasCommits?: boolean
    commitMessage?: string
    onFileClick?: (filePath: string, staged: boolean) => TabId | Promise<TabId> | void
    onPersistTab?: (openedTab: TabId | Promise<TabId> | null | undefined) => void
    onViewAllChanges?: (staged: boolean) => void
    onCommit?: (message: string) => void
    onStageAll?: () => void
    onUnstageAll?: () => void
    onDiscardAll?: () => void
    onStashAll?: () => void
    onUndoLastCommit?: () => void
    onPush?: () => void
    onPushForceWithLease?: () => void
    onPull?: () => void
    onFetch?: () => void
  } = $props()

  type GitTreeTargetType = 'file' | 'directory'
  type GitTreeActionKind = 'stage' | 'unstage' | 'discard'

  interface GitTreeTarget {
    path: string
    type: GitTreeTargetType
  }

  interface PendingGitConfirm {
    action: GitTreeActionKind
    target: GitTreeTarget
  }

  let collapsedDirs = new SvelteSet<string>()
  let committing = $state(false)

  let contextFilePath = $state<string | null>(null)
  let contextTargetType = $state<GitTreeTargetType | null>(null)
  let contextIsStaged = $state(false)
  let contextCanOpenFile = $state(false)
  let pendingOpenedTab = $state<Promise<TabId> | null>(null)

  let pendingGitConfirm = $state<PendingGitConfirm | null>(null)
  const gitSourceAttachmentCache = new Map<string, ReturnType<typeof gitChangeDragSource>>()
  const gitZoneAttachmentCache = new Map<string, ReturnType<typeof gitDropZone>>()

  $effect(() => {
    projectPath
    collapsedDirs.clear()
    gitSourceAttachmentCache.clear()
    gitZoneAttachmentCache.clear()
  })

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

  let canCommit = $derived(commitMessage.trim().length > 0 && stagedFiles.length > 0 && !committing)

  function handleCommit() {
    if (!canCommit) return
    committing = true
    onCommit?.(commitMessage.trim())
    commitMessage = ''
    committing = false
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

  function getConfirmTitle(action: GitTreeActionKind): string {
    switch (action) {
      case 'stage':
        return 'Stage Changes?'
      case 'unstage':
        return 'Unstage Changes?'
      case 'discard':
        return 'Revert Changes?'
    }
  }

  function getConfirmLabel(action: GitTreeActionKind): string {
    switch (action) {
      case 'stage':
        return 'Stage'
      case 'unstage':
        return 'Unstage'
      case 'discard':
        return 'Revert'
    }
  }

  function getConfirmMessage(confirm: PendingGitConfirm): string {
    const subject = describeActionTarget(confirm.target, confirm.action)
    if (confirm.action === 'discard') {
      return `This will permanently revert unstaged changes for ${subject}. This cannot be undone.`
    }
    return `${getConfirmLabel(confirm.action)} changes for ${subject}?`
  }

  function queueGitConfirm(action: GitTreeActionKind, target: GitTreeTarget): void {
    if (getActionFiles(target, action).length === 0) return
    pendingGitConfirm = { action, target }
  }

  function trackOpenedTab(openedTab: TabId | Promise<TabId> | void): void {
    if (openedTab == null) {
      pendingOpenedTab = null
      return
    }
    pendingOpenedTab = Promise.resolve(openedTab)
  }

  async function openActualFile(filePath: string) {
    try {
      await openTextFile(projectId, filePath, { temporary: false })
    } catch (e) {
      notify.error('Open file failed', errMessage(e))
    }
  }

  function handleCtxOpenFile() {
    if (!contextFilePath) return
    void openActualFile(contextFilePath)
  }

  function handleCtxOpenChanges() {
    if (!contextFilePath) return

    if (contextTargetType === 'file') {
      openWorkingTreeDiff(projectId, contextIsStaged, contextFilePath, contextFilePath, { temporary: false })
      return
    }

    if (contextTargetType === 'directory') {
      openWorkingTreeDiff(projectId, contextIsStaged, contextFilePath, null, { temporary: false })
    }
  }

  function handleCtxOpenFileHead() {
    if (!contextFilePath) return
    openHeadSnapshot(projectId, contextFilePath)
  }

  function handleCtxStage() {
    if (!contextFilePath || !contextTargetType) return
    queueGitConfirm('stage', { path: contextFilePath, type: contextTargetType })
  }

  function handleCtxUnstage() {
    if (!contextFilePath || !contextTargetType) return
    queueGitConfirm('unstage', { path: contextFilePath, type: contextTargetType })
  }

  function handleCtxDiscard() {
    if (!contextFilePath || !contextTargetType) return
    queueGitConfirm('discard', { path: contextFilePath, type: contextTargetType })
  }

  async function confirmGitAction() {
    const confirm = pendingGitConfirm
    if (!confirm) return
    pendingGitConfirm = null
    const files = getActionFiles(confirm.target, confirm.action)
    if (files.length === 0) return
    const description =
      confirm.target.type === 'file'
        ? confirm.target.path
        : `${describeFileCount(files.length)} under ${confirm.target.path}`

    try {
      await runGitAction(projectId, projectPath, (path) => {
        switch (confirm.action) {
          case 'stage':
            return backend.git.stageFiles(path, files)
          case 'unstage':
            return backend.git.unstageFiles(path, files)
          case 'discard':
            return backend.git.discardFiles(path, files)
        }
      })
      notify.success(
        confirm.action === 'discard'
          ? 'Changes reverted'
          : confirm.action === 'stage'
            ? 'Changes staged'
            : 'Changes unstaged',
        description
      )
    } catch (e) {
      notify.error(
        confirm.action === 'discard' ? 'Revert failed' : confirm.action === 'stage' ? 'Stage failed' : 'Unstage failed',
        errMessage(e)
      )
    }
  }

  async function handleCtxReveal() {
    if (!contextFilePath) return
    const absPath = await join(projectPath, contextFilePath)
    await revealItemInDir(absPath)
  }

  async function handleCtxCopyPath() {
    if (!contextFilePath) return
    const absPath = await join(projectPath, contextFilePath)
    await copyToClipboard(absPath)
  }

  async function handleCtxCopyRelativePath() {
    if (!contextFilePath) return
    await copyToClipboard(contextFilePath)
  }

  async function handleCtxCopyPatch() {
    if (!contextFilePath) return
    try {
      const patch = await backend.git.getPathPatch(projectPath, [contextFilePath], contextIsStaged)
      if (patch) await copyToClipboard(patch)
      else
        notify.info('No diff to copy', `${contextFilePath} has no ${contextIsStaged ? 'staged' : 'unstaged'} changes.`)
    } catch (e) {
      notify.error('Copy patch failed', errMessage(e))
    }
  }

  async function handleCtxCopyFolderPatch() {
    if (!contextFilePath) return
    const files = getFilesUnderPath(contextFilePath, contextIsStaged)
    if (files.length === 0) return
    try {
      const patch = await backend.git.getPathPatch(projectPath, files, contextIsStaged)
      if (patch) await copyToClipboard(patch)
    } catch (e) {
      notify.error('Copy folder patch failed', errMessage(e))
    }
  }

  async function handleCtxCopyFullPatch() {
    try {
      const patch = await backend.git.getFullPatch(projectPath)
      if (patch) await copyToClipboard(patch)
      else notify.info('No diff to copy', 'No changes in the working tree.')
    } catch (e) {
      notify.error('Copy patch failed', errMessage(e))
    }
  }

  function gitSourceAttachment(node: FileTreeNode<GitChange>, staged: boolean) {
    const hasChanges =
      node.type === 'file' ? node.change !== undefined : getFilesUnderPath(node.path, staged).length > 0
    if (!hasChanges) return null
    const key = `${projectId}:${staged ? 'staged' : 'unstaged'}:${node.type}:${node.path}`
    const cached = gitSourceAttachmentCache.get(key)
    if (cached) return cached
    const attachment = gitChangeDragSource({
      projectId,
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
    const key = `${projectId}:${staged ? 'staged' : 'unstaged'}`
    const cached = gitZoneAttachmentCache.get(key)
    if (cached) return cached
    const attachment = gitDropZone({
      projectId,
      staged,
      onDropFiles: async (files, targetStaged) => {
        try {
          await runGitAction(projectId, projectPath, (path) =>
            targetStaged ? backend.git.stageFiles(path, files) : backend.git.unstageFiles(path, files)
          )
          notify.success(
            targetStaged ? 'Files staged' : 'Files unstaged',
            `${files.length} file${files.length === 1 ? '' : 's'}`
          )
        } catch (error) {
          notify.error('Git drop failed', errMessage(error))
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
            <DropdownMenuItem onclick={() => onPull?.()}>Pull</DropdownMenuItem>
            <DropdownMenuItem onclick={() => onPush?.()}>Push</DropdownMenuItem>
            <DropdownMenuItem onclick={() => onFetch?.()}>Fetch</DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onclick={() => onPushForceWithLease?.()}>Force Push (with lease)</DropdownMenuItem>
            {#if hasCommits}
              <DropdownMenuSeparator />
              <DropdownMenuItem destructive onclick={() => onUndoLastCommit?.()}>Undo Last Commit</DropdownMenuItem>
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

  {#snippet fileGroup(label: string, tree: FileTreeNode<GitChange>[], keySuffix: string, isStaged: boolean)}
    {#if hasChanges}
      {@const fileCount = countFiles(tree)}
      <div class="relative py-1" {@attach gitZoneAttachment(isStaged)}>
        {#snippet rowActions(node: FileTreeNode<GitChange>)}
          {@const target = makeTreeTarget(node)}
          {#if !isStaged}
            <IconButton
              tooltip="Revert changes"
              tooltipSide="bottom"
              tone="danger"
              onclick={() => queueGitConfirm('discard', target)}
            >
              <Undo2Icon size={13} />
            </IconButton>
          {/if}
          {#if isStaged}
            <IconButton
              tooltip="Unstage changes"
              tooltipSide="bottom"
              onclick={() => queueGitConfirm('unstage', target)}
            >
              <MinusCircle size={13} />
            </IconButton>
          {:else}
            <IconButton tooltip="Stage changes" tooltipSide="bottom" onclick={() => queueGitConfirm('stage', target)}>
              <PlusCircle size={13} />
            </IconButton>
          {/if}
          {#if node.type === 'file' && canOpenActualFile(node)}
            <IconButton
              tooltip="Open actual file"
              tooltipSide="bottom"
              onclick={() => void openActualFile(target.path)}
            >
              <SquareArrowOutUpRight size={13} />
            </IconButton>
          {/if}
        {/snippet}

        <div class="group/hdr relative flex items-center px-2.5 py-1">
          <span class="text-xs font-medium tracking-wide text-muted uppercase">
            {label} ({fileCount})
          </span>
          {#if fileCount > 0}
            <div class="ml-auto flex items-center gap-0.5 opacity-0 transition-all group-hover/hdr:opacity-100">
              {#if isStaged && onUnstageAll}
                <IconButton tooltip="Unstage all" tooltipSide="bottom" onclick={() => onUnstageAll?.()}>
                  <MinusCircle size={13} />
                </IconButton>
              {/if}
              {#if !isStaged && onStageAll}
                <IconButton tooltip="Stage all" tooltipSide="bottom" onclick={() => onStageAll?.()}>
                  <PlusCircle size={13} />
                </IconButton>
              {/if}
              {#if !isStaged && onStashAll}
                <IconButton tooltip="Stash all" tooltipSide="bottom" onclick={() => onStashAll?.()}>
                  <PackageIcon size={13} />
                </IconButton>
              {/if}
              {#if !isStaged && onDiscardAll}
                <IconButton
                  tooltip="Discard all changes"
                  tooltipSide="bottom"
                  class="rounded p-0.5 text-muted hover:text-danger"
                  onclick={() => onDiscardAll?.()}
                >
                  <Trash2 size={13} />
                </IconButton>
              {/if}
              {#if onViewAllChanges}
                <IconButton
                  tooltip="View all {label.toLowerCase()} diffs"
                  tooltipSide="bottom"
                  onclick={() => onViewAllChanges?.(isStaged)}
                >
                  <FileDiff size={13} />
                </IconButton>
              {/if}
            </div>
          {/if}
          <DropOverlay
            visible={isGitDropZoneActive(projectId, isStaged)}
            zone="merge"
            label={isStaged ? 'Stage' : 'Unstage'}
          />
        </div>
        {#if tree.length > 0}
          {@const filter = isStaged ? stagedFilter : unstagedFilter}
          <FileTreeItems
            nodes={tree}
            isCollapsed={(path) => {
              if (filterActive && filter.expand.has(path)) return false
              return collapsedDirs.has(getDirKey(keySuffix, path))
            }}
            isDimmed={filterActive ? (node) => !filter.matched.has(node.path) : undefined}
            onToggleDir={(path) => toggleDir(keySuffix, path)}
            onFileClick={(node) => {
              if (!node.change) return
              trackOpenedTab(onFileClick?.(node.change.path, node.change.staged))
            }}
            onFileDblClick={() => onPersistTab?.(pendingOpenedTab)}
            onFileContextMenu={(e, node) => handleContextMenu(e, node, isStaged)}
            {fileTrailing}
            {rowActions}
            dndEnabled={true}
            dndSourceAttachment={(node) => gitSourceAttachment(node, isStaged)}
          />
        {:else}
          <div class="px-2.5 pt-2">
            <div
              class="rounded-xl border-2 border-dashed px-3 py-3 text-sm transition-colors {isGitDropZoneActive(
                projectId,
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
    onPush={() => onPush?.()}
    onPull={() => onPull?.()}
    onFetch={() => onFetch?.()}
    onCopyFullPatch={handleCtxCopyFullPatch}
    onResetTarget={resetContextTarget}
  >
    {#if !hasChanges}
      <div class="px-2.5 py-2 text-sm text-subtle">No changes.</div>
    {/if}

    {@render fileGroup('Staged', stagedTree, 'staged', true)}
    {@render fileGroup('Changes', unstagedTree, 'unstaged', false)}
  </GitContextMenu>
</div>

<ConfirmDialog
  open={pendingGitConfirm !== null}
  title={pendingGitConfirm ? getConfirmTitle(pendingGitConfirm.action) : 'Confirm Git Action'}
  message={pendingGitConfirm ? getConfirmMessage(pendingGitConfirm) : ''}
  confirmLabel={pendingGitConfirm ? getConfirmLabel(pendingGitConfirm.action) : 'Confirm'}
  onConfirm={confirmGitAction}
  onCancel={() => {
    pendingGitConfirm = null
  }}
/>
