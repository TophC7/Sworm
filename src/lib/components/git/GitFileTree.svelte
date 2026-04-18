<script lang="ts">
  import { backend } from '$lib/api/backend'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import GitContextMenu from '$lib/components/git/GitContextMenu.svelte'
  import GitStatusBadge from '$lib/components/git/GitStatusBadge.svelte'
  import DropOverlay from '$lib/dnd/DropOverlay.svelte'
  import { gitChangeDragSource, gitDropZone, isGitDropZoneActive } from '$lib/dnd/adapters/git.svelte'
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
  import { ChevronDown, FileDiff, MinusCircle, PackageIcon, PlusCircle, Trash2 } from '$lib/icons/lucideExports'
  import { runGitAction } from '$lib/stores/git.svelte'
  import { addEditorTab, addReadonlyEditorTab } from '$lib/stores/workspace.svelte'
  import type { GitChange, GitSummary } from '$lib/types/backend'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import { notify } from '$lib/stores/notifications.svelte'

  function errMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e)
  }
  import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree'
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
    onFileClick?: (filePath: string, staged: boolean) => void
    onPersistTab?: () => void
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

  let collapsedDirs = new SvelteSet<string>()
  let committing = $state(false)

  let contextFilePath = $state<string | null>(null)
  let contextTargetType = $state<'file' | 'directory' | null>(null)
  let contextIsStaged = $state(false)

  let discardConfirmOpen = $state(false)
  let discardTarget = $state<string | null>(null)
  let discardTargetType = $state<'file' | 'directory' | null>(null)
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
  }

  function resetContextTarget() {
    contextFilePath = null
    contextTargetType = null
    contextIsStaged = false
  }

  function getFilesUnderPath(dirPath: string, staged: boolean): string[] {
    const changes = staged ? stagedFiles : unstagedFiles
    return changes.filter((c) => c.path.startsWith(dirPath + '/')).map((c) => c.path)
  }

  function handleCtxOpenFile() {
    if (!contextFilePath) return
    addEditorTab(projectId, contextFilePath)
  }

  function handleCtxOpenFileHead() {
    if (!contextFilePath) return
    addReadonlyEditorTab(projectId, contextFilePath, 'HEAD', 'HEAD')
  }

  function handleCtxStage() {
    if (!contextFilePath) return
    const files = contextTargetType === 'directory' ? getFilesUnderPath(contextFilePath, false) : [contextFilePath]
    void runGitAction(projectId, projectPath, (path) => backend.git.stageFiles(path, files))
  }

  function handleCtxUnstage() {
    if (!contextFilePath) return
    const files = contextTargetType === 'directory' ? getFilesUnderPath(contextFilePath, true) : [contextFilePath]
    void runGitAction(projectId, projectPath, (path) => backend.git.unstageFiles(path, files))
  }

  function handleCtxDiscard() {
    if (!contextFilePath) return
    discardTarget = contextFilePath
    discardTargetType = contextTargetType
    discardConfirmOpen = true
  }

  async function confirmDiscard() {
    if (!discardTarget) return
    const files =
      discardTargetType === 'directory' ? getFilesUnderPath(discardTarget, contextIsStaged) : [discardTarget]
    try {
      await runGitAction(projectId, projectPath, (path) => backend.git.discardFiles(path, files))
    } catch (e) {
      notify.error('Discard failed', errMessage(e))
    } finally {
      discardConfirmOpen = false
      discardTarget = null
      discardTargetType = null
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

  function gitSourceAttachment(node: FileTreeNode<GitChange>) {
    if (!node.change) return null
    const key = `${projectId}:${node.change.path}:${node.change.staged}`
    const cached = gitSourceAttachmentCache.get(key)
    if (cached) return cached
    const attachment = gitChangeDragSource({ projectId, change: node.change })
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

<div class="flex min-h-full flex-col text-[0.78rem]">
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
          <DropdownMenuContent class="min-w-[180px] text-[0.72rem]">
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

  {#snippet fileTrailing(node: FileTreeNode<GitChange>)}
    {#if node.change}
      {#if node.change.status !== 'D' && (node.change.additions != null || node.change.deletions != null)}
        {@const net = (node.change.additions ?? 0) - (node.change.deletions ?? 0)}
        {#if net !== 0}
          <span class="shrink-0 pr-2 font-mono {net > 0 ? 'text-success' : 'text-danger'}">
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
    {#if tree.length > 0}
      <div class="py-1">
        <div class="group/hdr relative flex items-center px-2.5 py-1" {@attach gitZoneAttachment(isStaged)}>
          <span class="text-[0.68rem] font-medium tracking-wide text-muted uppercase">
            {label} ({countFiles(tree)})
          </span>
          <div class="ml-auto flex items-center gap-0.5 opacity-0 transition-all group-hover/hdr:opacity-100">
            {#if isStaged && onUnstageAll}
              <IconButton
                tooltip="Unstage all"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onUnstageAll?.()}
              >
                <MinusCircle size={13} />
              </IconButton>
            {/if}
            {#if !isStaged && onStageAll}
              <IconButton
                tooltip="Stage all"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onStageAll?.()}
              >
                <PlusCircle size={13} />
              </IconButton>
            {/if}
            {#if !isStaged && onStashAll}
              <IconButton
                tooltip="Stash all"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onStashAll?.()}
              >
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
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onViewAllChanges?.(isStaged)}
              >
                <FileDiff size={13} />
              </IconButton>
            {/if}
          </div>
          <DropOverlay
            visible={isGitDropZoneActive(projectId, isStaged)}
            zone="merge"
            label={isStaged ? 'Stage' : 'Unstage'}
          />
        </div>
        <FileTreeItems
          nodes={tree}
          isCollapsed={(path) => collapsedDirs.has(getDirKey(keySuffix, path))}
          onToggleDir={(path) => toggleDir(keySuffix, path)}
          onFileClick={(node) => node.change && onFileClick?.(node.change.path, node.change.staged)}
          onFileDblClick={() => onPersistTab?.()}
          onFileContextMenu={(e, node) => handleContextMenu(e, node, isStaged)}
          {fileTrailing}
          dndEnabled={true}
          dndSourceAttachment={gitSourceAttachment}
        />
      </div>
    {/if}
  {/snippet}

  <GitContextMenu
    filePath={contextFilePath}
    targetType={contextTargetType}
    isStaged={contextIsStaged}
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
    {#if stagedTree.length === 0 && unstagedTree.length === 0}
      <div class="px-2.5 py-2 text-[0.75rem] text-subtle">No changes.</div>
    {/if}

    {@render fileGroup('Staged', stagedTree, 'staged', true)}
    {@render fileGroup('Changes', unstagedTree, 'unstaged', false)}
  </GitContextMenu>
</div>

<ConfirmDialog
  open={discardConfirmOpen}
  title="Discard Changes?"
  message="This will permanently discard changes for {discardTarget}. This cannot be undone."
  confirmLabel="Discard"
  onConfirm={confirmDiscard}
  onCancel={() => {
    discardConfirmOpen = false
    discardTarget = null
  }}
/>
