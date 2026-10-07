<script lang="ts">
  import { onMount } from 'svelte'
  import { Eye, EyeOff } from '$lib/icons/lucideExports'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import { buildTreeFilter } from '$lib/utils/fileTreeFilter'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import TreeFilterInput from '$lib/components/file-tree/TreeFilterInput.svelte'
  import ImportCollisionDialog from '$lib/features/files/ImportCollisionDialog.svelte'
  import FileContextMenu from '$lib/features/files/FileContextMenu.svelte'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import { withTrashFallback } from '$lib/features/files/trash'
  import PromptDialog from '$lib/components/dialogs/PromptDialog.svelte'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { backend } from '$lib/api/backend'
  import { platform } from '$lib/platform'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import {
    ensureProjectFiles,
    getProjectFilePaths,
    isProjectFilesLoading,
    isProjectFilesStale,
    isProjectFilesTruncated,
    markProjectFilesStale
  } from '$lib/features/files/projectFiles.svelte'
  import {
    dimmedPathsFor,
    ensureFileTreeListeners,
    expandDir,
    getFolderTree,
    invalidateDirs,
    isExpanded,
    loadDir,
    nodesFor,
    revealPath,
    setShowHidden,
    toggleDir
  } from '$lib/features/files/explorer.svelte'
  import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import type { TabId } from '$lib/features/workbench/model'
  import { deleteTextPath, openTextFile, renameTextPath } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { getActiveTab, promoteTabWhenReady } from '$lib/features/workbench/state.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import type { DragPayload } from '$lib/features/dnd/payload'
  import type { FilePasteCollision, FilePasteMapping } from '$lib/types/backend'
  import {
    fileTreeDirectoryDropTarget,
    fileTreeDragSource,
    isFileTreeDropActive
  } from '$lib/features/dnd/adapters/file-tree.svelte'
  import {
    basename,
    dirname,
    isEqualOrParent,
    normalizeAbsolutePath,
    resolveProjectFile,
    splitRemotePath,
    toProjectRelativePath
  } from '$lib/utils/paths'

  let { folderPath }: { folderPath: string } = $props()
  let folderName = $derived(basename(normalizeAbsolutePath(folderPath)) || folderPath || 'Files')
  let remoteFolder = $derived(splitRemotePath(normalizeAbsolutePath(folderPath)))

  let filterQuery = $state('')
  let filterActive = $derived(filterQuery.trim().length > 0)

  // The tree itself loads one directory per expand. The filter box instead
  // needs whole-project reach, so a non-empty query switches to the flat,
  // ignore-aware path list shared with the Quick Open palette.
  let tree = $derived(getFolderTree(folderPath))
  let lazyNodes = $derived(nodesFor(folderPath))
  let flatPaths = $derived(getProjectFilePaths(folderPath, tree.showHidden))
  let flatNodes = $derived<FileTreeNode<{ path: string }>[]>(buildFileTree(flatPaths.map((path) => ({ path }))))
  let fileTree = $derived(filterActive ? flatNodes : lazyNodes)
  let dimmedPaths = $derived(dimmedPathsFor(folderPath))
  let loading = $derived(
    filterActive ? isProjectFilesLoading(folderPath, tree.showHidden) && flatPaths.length === 0 : !tree.children.has('')
  )
  let treeFilter = $derived(buildTreeFilter(fileTree, filterQuery))

  // Only pay for the flat list once the filter is actually in use. Reading the
  // stale flag keeps an open filter box current after a mutation without
  // making every mutation re-walk the project.
  $effect(() => {
    if (!filterActive) return
    isProjectFilesStale(folderPath, tree.showHidden)
    void ensureProjectFiles(folderPath, tree.showHidden)
  })

  let contextFilePath = $state<string | null>(null)
  let contextTargetType = $state<'file' | 'directory' | null>(null)

  let renameFilePath = $state<string | null>(null)
  let renameValue = $state('')

  let newItemKind = $state<'file' | 'folder' | null>(null)
  let newItemName = $state('')

  let pendingTransfer = $state<{
    op: 'copy' | 'cut'
    targetDir: string
    sources: string[]
    index: number
    created: FilePasteMapping[]
    collisionDestinations: Record<string, string>
  } | null>(null)
  let activeCollision = $state<FilePasteCollision | null>(null)
  let collisionRenameValue = $state('')
  let pendingFileOpen = $state<Promise<TabId> | null>(null)

  const sourceAttachmentCache = new Map<string, ReturnType<typeof fileTreeDragSource>>()
  const directoryAttachmentCache = new Map<string, ReturnType<typeof fileTreeDirectoryDropTarget>>()

  /**
   * Re-read the listings a mutation touched. Own mutations refresh immediately
   * instead of waiting on the watcher's debounce. The flat search list is only
   * marked stale: re-walking the whole project for one file is work nobody
   * asked for, so the next reader pays for it.
   */
  async function invalidate(...dirs: string[]) {
    await invalidateDirs(
      folderPath,
      dirs.map((dir) => (dir === '.' ? '' : dir))
    )
    markProjectFilesStale(folderPath)
  }

  function sourceAttachmentKey(path: string, type: 'file' | 'directory'): string {
    return `${folderPath}:src:${type}:${path}`
  }

  function directoryAttachmentKey(path: string): string {
    return `${folderPath}:dir:${path}`
  }

  function dndSourceAttachment(node: FileTreeNode<{ path: string }>) {
    const key = sourceAttachmentKey(node.path, node.type)
    const cached = sourceAttachmentCache.get(key)
    if (cached) return cached
    const attachment = fileTreeDragSource({ folderPath, node })
    sourceAttachmentCache.set(key, attachment)
    return attachment
  }

  function dndDirectoryAttachment(node: FileTreeNode<{ path: string }>) {
    if (node.type !== 'directory') return null
    const key = directoryAttachmentKey(node.path)
    const cached = directoryAttachmentCache.get(key)
    if (cached) return cached
    const attachment = fileTreeDirectoryDropTarget({
      folderPath,
      directoryPath: node.path,
      acceptOsFiles: !remoteFolder,
      onHoverExpand: () => expandDir(folderPath, node.path),
      onDrop: (payload) => handleDirectoryDrop(node.path, payload)
    })
    directoryAttachmentCache.set(key, attachment)
    return attachment
  }

  function rootDndAttachment() {
    const key = directoryAttachmentKey('.')
    const cached = directoryAttachmentCache.get(key)
    if (cached) return cached
    const attachment = fileTreeDirectoryDropTarget({
      folderPath,
      directoryPath: '.',
      acceptOsFiles: !remoteFolder,
      onDrop: (payload) => handleDirectoryDrop('.', payload)
    })
    directoryAttachmentCache.set(key, attachment)
    return attachment
  }

  async function handleDirectoryDrop(targetDir: string, payload: DragPayload): Promise<void> {
    try {
      const internal: string[] = []
      const external: string[] = []
      for (const item of payload.items) {
        if (item.kind === 'file') {
          if (item.path === targetDir) {
            notify.info('Cannot move item', 'Destination is the same item.')
            continue
          }
          if (isEqualOrParent(item.path, targetDir)) {
            notify.warning('Cannot move item', 'A folder cannot be moved into itself or one of its children.')
            continue
          }
          if ((dirname(item.path) || '.') === targetDir) continue
          internal.push(resolveProjectFile(folderPath, item.path))
        } else if (item.kind === 'os-files' && !remoteFolder) {
          external.push(...item.paths)
        }
      }
      if (internal.length) await runTransferWithCollisionHandling('cut', targetDir, internal)
      else if (external.length) await runTransferWithCollisionHandling('copy', targetDir, external)
    } catch (error) {
      notify.error('Drop failed', getErrorMessage(error))
    }
  }

  async function runTransferWithCollisionHandling(
    op: 'copy' | 'cut',
    targetDir: string,
    sources: string[]
  ): Promise<void> {
    if (pendingTransfer) {
      notify.info('Transfer in progress', 'Resolve the current collision prompt before starting another transfer.')
      return
    }
    const uniqueSources = Array.from(new Set(sources))
    if (uniqueSources.length === 0) return

    const collisions = await backend.files.pasteCollisions(folderPath, targetDir, uniqueSources)
    const collisionDestinations: Record<string, string> = {}
    for (const collision of collisions) {
      collisionDestinations[collision.source] = collision.destination
    }

    pendingTransfer = {
      op,
      targetDir,
      sources: uniqueSources,
      index: 0,
      created: [],
      collisionDestinations
    }
    await continuePendingTransfer()
  }

  async function continuePendingTransfer(): Promise<void> {
    try {
      while (pendingTransfer && pendingTransfer.index < pendingTransfer.sources.length) {
        const source = pendingTransfer.sources[pendingTransfer.index]
        const destination = pendingTransfer.collisionDestinations[source]
        if (destination) {
          activeCollision = { source, destination }
          collisionRenameValue = basename(source)
          return
        }

        await transferSourceWithPolicy(source, 'auto_rename')
        pendingTransfer.index += 1
      }

      await finalizePendingTransfer()
    } catch (error) {
      notify.error('Transfer failed', getErrorMessage(error))
      abortPendingTransfer()
    }
  }

  async function transferSourceWithPolicy(
    source: string,
    policy: 'auto_rename' | 'replace' | 'skip' | 'rename',
    renameTo?: string
  ): Promise<void> {
    if (!pendingTransfer) return
    const transfer = pendingTransfer
    const renameMap = policy === 'rename' && renameTo ? { [source]: renameTo } : undefined
    let created: FilePasteMapping[] = []
    await withTrashFallback(async (permanent) => {
      created = await backend.files.paste(
        folderPath,
        transfer.targetDir,
        transfer.op,
        [source],
        policy,
        renameMap,
        permanent
      )
    })
    transfer.created.push(...created)
  }

  async function resolveCollision(action: 'replace' | 'skip' | 'rename'): Promise<void> {
    if (!pendingTransfer || !activeCollision) return
    const source = activeCollision.source

    try {
      if (action === 'rename') {
        const nextName = collisionRenameValue.trim()
        if (!nextName) {
          notify.warning('Rename required', 'Provide a new name before continuing.')
          return
        }
        await transferSourceWithPolicy(source, 'rename', nextName)
      } else {
        await transferSourceWithPolicy(source, action)
      }

      pendingTransfer.index += 1
      delete pendingTransfer.collisionDestinations[source]
      activeCollision = null
      collisionRenameValue = ''
      await continuePendingTransfer()
    } catch (error) {
      notify.error('Transfer failed', getErrorMessage(error))
      abortPendingTransfer()
    }
  }

  function abortPendingTransfer() {
    pendingTransfer = null
    activeCollision = null
    collisionRenameValue = ''
  }

  async function finalizePendingTransfer(): Promise<void> {
    if (!pendingTransfer) return
    const { op, created, targetDir } = pendingTransfer
    const createdCount = created.length
    abortPendingTransfer()
    const destinations = created.map((mapping) => dirname(mapping.destination) || '')
    const emptied =
      op === 'cut'
        ? created.flatMap((mapping) => {
            const source = toProjectRelativePath(folderPath, mapping.source)
            return source == null ? [] : [dirname(source) || '']
          })
        : []
    await invalidate(targetDir, ...destinations, ...emptied)

    if (createdCount === 0) {
      notify.info('Nothing transferred', 'All colliding items were skipped.')
      return
    }

    const verb = op === 'cut' ? 'Moved' : 'Pasted'
    notify.success(`${verb} ${createdCount} file${createdCount === 1 ? '' : 's'}`)
  }

  function handleFileClick(filePath: string) {
    pendingFileOpen = openTextFile(folderPath, filePath)
  }

  // Active file from the global active editor tab.
  let activeTab = $derived(getActiveTab())
  let activeFilePath = $derived(activeTab?.kind === 'text' ? activeTab.filePath : null)

  // Reveal the active file by loading and expanding its ancestors. Depends on
  // the root listing so that on mount we reveal AFTER the root's
  // first load — otherwise the user opens a tab from (say) the git diff
  // sidebar, switches back to Files, and finds the tree collapsed despite a
  // file being focused. Expanding is idempotent; user-collapsed dirs only
  // re-expand when the active path itself changes, mirroring VS Code's
  // "reveal in explorer".
  $effect(() => {
    tree.children.has('')
    const path = activeFilePath
    if (path) void revealPath(folderPath, path)
  })

  // Path -> status letter lookup from git state (prefer unstaged over staged)
  let gitSummary = $derived(getGitSummary(folderPath))
  let gitStatusMap = $derived.by(() => {
    const map = new Map<string, string>()
    if (!gitSummary?.changes) return map
    for (const change of gitSummary.changes) {
      if (!map.has(change.path) || !change.staged) {
        map.set(change.path, change.status)
      }
    }
    return map
  })

  // Directories that contain changed files (any ancestor of a changed path).
  let dirsWithChanges = $derived.by(() => {
    const dirs = new Set<string>()
    for (const filePath of gitStatusMap.keys()) {
      const parts = filePath.split('/')
      for (let i = 1; i < parts.length; i++) {
        dirs.add(parts.slice(0, i).join('/'))
      }
    }
    return dirs
  })

  onMount(() => {
    ensureFileTreeListeners()
    void loadDir(folderPath, '')
  })

  function handleFileContextMenu(_e: MouseEvent, node: FileTreeNode<{ path: string }>) {
    contextFilePath = node.change?.path ?? node.path
    contextTargetType = node.type
  }

  // Fires on capture phase, before bubble handlers on file/folder buttons.
  // If empty space was right-clicked, nothing sets it back, so it stays null.
  function resetContextTarget() {
    contextFilePath = null
    contextTargetType = null
  }

  async function handleRevealInFolder() {
    const native = platform.native
    if (!contextFilePath || remoteFolder || !native) return
    await native.files.reveal(resolveProjectFile(folderPath, contextFilePath))
  }

  function handleOpenInEditor() {
    if (!contextFilePath) return
    openTextFile(folderPath, contextFilePath)
  }

  function handleOpenDiff() {
    if (!contextFilePath) return
    openWorkingTreeDiff(folderPath, false, contextFilePath, contextFilePath, { temporary: false })
  }

  async function writeFileClipboard(op: 'copy' | 'cut') {
    const native = platform.native
    if (!contextFilePath || !native) return
    const source = resolveProjectFile(folderPath, contextFilePath)
    try {
      await native.files.clipboardCopyFiles([source], op)
    } catch (error) {
      notify.error(`${op === 'cut' ? 'Cut' : 'Copy'} failed`, getErrorMessage(error))
    }
  }

  function handleCut() {
    return writeFileClipboard('cut')
  }

  function handleCopy() {
    return writeFileClipboard('copy')
  }

  async function handlePaste() {
    const native = platform.native
    if (!native) return
    const targetDir = contextTargetType === 'directory' && contextFilePath ? contextFilePath : '.'
    try {
      const clip = await native.files.clipboardReadFiles()
      if (!clip || clip.paths.length === 0) {
        notify.info('Nothing to paste', 'No files on the clipboard.')
        return
      }
      await runTransferWithCollisionHandling(clip.op, targetDir, clip.paths)
    } catch (error) {
      notify.error('Paste failed', getErrorMessage(error))
    }
  }

  async function handleCopyPath() {
    if (!contextFilePath) return
    const absolutePath = resolveProjectFile(folderPath, contextFilePath)
    await platform.clipboard.writeText(splitRemotePath(absolutePath)?.path ?? absolutePath)
  }

  async function handleCopyRelativePath() {
    if (!contextFilePath) return
    await platform.clipboard.writeText(contextFilePath)
  }

  function handleRename() {
    if (!contextFilePath) return
    renameFilePath = contextFilePath
    renameValue = contextFilePath
  }

  async function confirmRename() {
    if (!renameFilePath || !renameValue || renameValue === renameFilePath) {
      renameFilePath = null
      return
    }
    try {
      await backend.files.rename(folderPath, renameFilePath, renameValue)
      await renameTextPath(folderPath, renameFilePath, renameValue)
      await invalidate(dirname(renameFilePath) || '', dirname(renameValue) || '')
    } catch (e) {
      notify.error('Rename failed', getErrorMessage(e))
    } finally {
      renameFilePath = null
    }
  }

  async function handleDelete() {
    if (!contextFilePath) return
    const deleteFilePath = contextFilePath
    const proceed = await confirmAsync({
      title: 'Delete File',
      message: `Move ${deleteFilePath} to the trash?`,
      confirmLabel: 'Move to Trash'
    })
    if (!proceed) return
    try {
      if (!(await withTrashFallback((permanent) => backend.files.delete(folderPath, deleteFilePath, permanent)))) return
      await deleteTextPath(folderPath, deleteFilePath)
      await invalidate(dirname(deleteFilePath) || '')
    } catch (e) {
      notify.error('Delete failed', getErrorMessage(e))
    }
  }

  function handleNewFile() {
    newItemKind = 'file'
    newItemName = ''
  }

  function handleNewFolder() {
    newItemKind = 'folder'
    newItemName = ''
  }

  async function confirmNewItem() {
    if (!newItemName.trim()) {
      newItemKind = null
      return
    }
    const kind = newItemKind
    try {
      const name = newItemName.trim()
      if (kind === 'file') {
        await backend.files.write(folderPath, name, '')
        await invalidate(dirname(name) || '')
        openTextFile(folderPath, name)
      } else if (kind === 'folder') {
        await backend.files.createDir(folderPath, name)
        await invalidate(dirname(name) || '')
      }
    } catch (e) {
      notify.error(`Failed to create ${kind ?? 'item'}`, getErrorMessage(e))
    } finally {
      newItemKind = null
      newItemName = ''
    }
  }

  function handleOpenExternal() {
    const native = platform.native
    if (remoteFolder || !native) return
    void native.files.reveal(folderPath)
  }

  async function handleCopyFolderPath() {
    await platform.clipboard.writeText(remoteFolder?.path ?? folderPath)
  }
</script>

<SidebarPanel title={folderName}>
  {#snippet headerActions()}
    <IconButton
      tooltip={tree.showHidden ? 'Hide hidden & ignored files' : 'Show hidden & ignored files'}
      onclick={() => setShowHidden(folderPath, !tree.showHidden)}
    >
      {#if tree.showHidden}
        <EyeOff size={12} />
      {:else}
        <Eye size={12} />
      {/if}
    </IconButton>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <TreeFilterInput bind:value={filterQuery} placeholder="Filter files..." ariaLabel="Filter files" />
    {#if filterActive && isProjectFilesTruncated(folderPath, tree.showHidden)}
      <div class="px-2.5 pb-1 text-xs text-subtle">Showing the first {flatPaths.length.toLocaleString()} files</div>
    {/if}
    <div
      class="min-h-0 flex-1 overflow-y-auto text-base {isFileTreeDropActive(folderPath, '.') ? 'bg-accent/6' : ''}"
      {@attach rootDndAttachment()}
    >
      <FileContextMenu
        filePath={contextFilePath}
        targetType={contextTargetType}
        canRevealInFileManager={!remoteFolder && platform.native !== null}
        canNativeFileClipboard={platform.native !== null && !remoteFolder}
        onRevealInFolder={handleRevealInFolder}
        onOpenInEditor={handleOpenInEditor}
        onOpenDiff={handleOpenDiff}
        onCut={handleCut}
        onCopy={handleCopy}
        onPaste={handlePaste}
        onCopyPath={handleCopyPath}
        onCopyRelativePath={handleCopyRelativePath}
        onRename={handleRename}
        onDelete={handleDelete}
        onNewFile={handleNewFile}
        onNewFolder={handleNewFolder}
        onOpenExternal={handleOpenExternal}
        onCopyFolderPath={handleCopyFolderPath}
        onResetTarget={resetContextTarget}
      >
        {#if loading}
          <div class="px-2.5 py-3 text-sm text-subtle">Loading files&hellip;</div>
        {:else if tree.error}
          <div class="px-2.5 py-3 text-sm text-danger">{tree.error}</div>
        {:else if fileTree.length === 0}
          <div class="px-2.5 py-3 text-sm text-subtle">No files found.</div>
        {:else}
          <FileTreeItems
            nodes={fileTree}
            isCollapsed={(node) => {
              if (filterActive) return !treeFilter.shouldExpand(node)
              return !isExpanded(folderPath, node.path)
            }}
            isActive={(path) => path === activeFilePath}
            isDimmed={filterActive ? (node) => !treeFilter.isMatch(node) : (node) => dimmedPaths.has(node.path)}
            hasDirChanges={(path) => dirsWithChanges.has(path)}
            onToggleDir={(path) => toggleDir(folderPath, path)}
            onFileClick={(node) => {
              if (node.change?.path) handleFileClick(node.change.path)
            }}
            onFileDblClick={() => promoteTabWhenReady(pendingFileOpen)}
            onFileContextMenu={handleFileContextMenu}
            {dndSourceAttachment}
            {dndDirectoryAttachment}
            dndIsDropActive={(path) => isFileTreeDropActive(folderPath, path)}
          >
            {#snippet fileTrailing(node)}
              {@const status = gitStatusMap.get(node.path)}
              {#if status}
                <GitStatusBadge {status} />
              {/if}
            {/snippet}
          </FileTreeItems>
        {/if}
      </FileContextMenu>
    </div>
  </div>
</SidebarPanel>

<ImportCollisionDialog
  open={activeCollision !== null}
  sourceName={activeCollision ? basename(activeCollision.source) : ''}
  destinationPath={activeCollision?.destination ?? ''}
  renameValue={collisionRenameValue}
  onRenameValueChange={(value) => {
    collisionRenameValue = value
  }}
  onReplace={() => {
    void resolveCollision('replace')
  }}
  onSkip={() => {
    void resolveCollision('skip')
  }}
  onRename={() => {
    void resolveCollision('rename')
  }}
  onCancel={abortPendingTransfer}
/>

<PromptDialog
  open={newItemKind !== null}
  title={newItemKind === 'folder' ? 'New Folder' : 'New File'}
  bind:value={newItemName}
  placeholder={newItemKind === 'file' ? 'path/to/file.ts' : 'path/to/folder'}
  confirmLabel="Create"
  onConfirm={confirmNewItem}
  onCancel={() => {
    newItemKind = null
    newItemName = ''
  }}
/>

<PromptDialog
  open={renameFilePath !== null}
  title="Rename"
  bind:value={renameValue}
  confirmLabel="Rename"
  onConfirm={confirmRename}
  onCancel={() => (renameFilePath = null)}
/>

