<script lang="ts">
  import type { TabId } from '$lib/features/workbench/model'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import GitContextMenu from '$lib/features/git/GitContextMenu.svelte'
  import GitCommitArea from '$lib/features/git/GitCommitArea.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import DropOverlay from '$lib/features/dnd/DropOverlay.svelte'
  import { gitChangeDragSource, gitDropZone, isGitDropZoneActive } from '$lib/features/dnd/adapters/git.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import {
    FileDiff,
    MinusCircle,
    PackageIcon,
    PlusCircle,
    SquareArrowOutUpRight,
    Trash2,
    Undo2Icon
  } from '$lib/icons/lucideExports'
  import { stageFiles, unstageFiles } from '$lib/features/git/state.svelte'
  import { stageAllFolder, stashFolder, unstageAllFolder } from '$lib/features/git/actions.svelte'
  import {
    confirmDiscardAll,
    confirmDiscardPaths,
    openActualFile,
    runPathsAction,
    type GitTreeMenuTarget
  } from '$lib/features/git/treeActions'
  import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { promoteTabWhenReady } from '$lib/features/workbench/state.svelte'
  import type { GitChange, GitSummary } from '$lib/types/backend'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
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

  function handleContextMenu(_e: MouseEvent, node: FileTreeNode<GitChange>, staged: boolean) {
    const section = staged ? 'staged' : 'changes'
    if (node.type === 'file' && node.change) {
      menuTarget = { kind: 'file', section, change: node.change }
    } else if (node.type === 'directory') {
      menuTarget = { kind: 'folder', section, path: node.path, files: getFilesUnderPath(node.path, staged) }
    }
  }

  function resetContextTarget() {
    menuTarget = null
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

  function describeFileCount(count: number): string {
    return `${count} file${count === 1 ? '' : 's'}`
  }

  function getActionFiles(target: GitTreeTarget, action: GitTreeActionKind): string[] {
    if (target.type === 'file') return [target.path]
    return getFilesUnderPath(target.path, action === 'unstage')
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
          {@const description =
            target.type === 'file' ? target.path : `${describeFileCount(files.length)} under ${target.path}`}
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

  <GitContextMenu {folderPath} {summary} target={menuTarget} onResetTarget={resetContextTarget}>
    {#if !hasChanges}
      <div class="px-2.5 py-2 text-sm text-subtle">No changes.</div>
    {/if}

    {@render fileGroup('Staged', stagedTree, 'staged', true, stagedFiles.length)}
    {@render fileGroup('Changes', unstagedTree, 'unstaged', false, unstagedFiles.length)}
  </GitContextMenu>
</div>
