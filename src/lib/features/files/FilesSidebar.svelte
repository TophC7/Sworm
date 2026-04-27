<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import { buildTreeFilter } from '$lib/utils/fileTreeFilter'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import TreeFilterInput from '$lib/components/file-tree/TreeFilterInput.svelte'
  import ImportCollisionDialog from '$lib/features/files/ImportCollisionDialog.svelte'
  import FileContextMenu from '$lib/features/files/FileContextMenu.svelte'
  import ConfirmDialog from '$lib/components/dialogs/ConfirmDialog.svelte'
  import PromptDialog from '$lib/components/dialogs/PromptDialog.svelte'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { backend } from '$lib/api/backend'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import { RotateCw } from '$lib/icons/lucideExports'
  import {
    ensureProjectFiles,
    getProjectFilePaths,
    isProjectFilesLoading,
    refreshProjectFiles
  } from '$lib/features/files/projectFiles.svelte'
  import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import type { TabId } from '$lib/features/workbench/model'
  import {
    deleteTextPath,
    openTextFile,
    openTextInFresh,
    renameTextPath
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { promoteTabWhenReady } from '$lib/features/workbench/state.svelte'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { getFocusedTab } from '$lib/features/workbench/state.svelte'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import { notify } from '$lib/features/notifications/state.svelte'
  import type { DragPayload } from '$lib/features/dnd/payload'
  import type { FilePasteCollision } from '$lib/types/backend'
  import {
    fileTreeDirectoryDropTarget,
    fileTreeDragSource,
    isFileTreeDropActive
  } from '$lib/features/dnd/adapters/file-tree.svelte'
  import { basename, dirname, isEqualOrParent } from '$lib/utils/paths'
  import { join } from '@tauri-apps/api/path'

  function errMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e)
  }

  let {
    projectId,
    projectPath
  }: {
    projectId: string
    projectPath: string
  } = $props()

  let expandedDirs = new SvelteSet<string>()
  let filterQuery = $state('')

  // Source-of-truth file paths come from the shared projectFiles
  // store so the command palette's `/` mode and this sidebar see the
  // same data and any mutation here invalidates both surfaces.
  let paths = $derived(getProjectFilePaths(projectId))
  let loading = $derived(isProjectFilesLoading(projectId) && paths.length === 0)
  let error = $state<string | null>(null)
  let fileTree = $derived<FileTreeNode<{ path: string }>[]>(buildFileTree(paths.map((p) => ({ path: p }))))
  let treeFilter = $derived(buildTreeFilter(fileTree, filterQuery))
  let filterActive = $derived(filterQuery.trim().length > 0)

  let contextFilePath = $state<string | null>(null)
  let contextTargetType = $state<'file' | 'directory' | null>(null)

  let renameFilePath = $state<string | null>(null)
  let renameValue = $state('')

  let newItemKind = $state<'file' | 'folder' | null>(null)
  let newItemName = $state('')

  let deleteConfirmOpen = $state(false)
  let deleteFilePath = $state<string | null>(null)
  let pendingTransfer = $state<{
    op: 'copy' | 'cut'
    targetDir: string
    sources: string[]
    index: number
    created: string[]
    collisionDestinations: Record<string, string>
  } | null>(null)
  let activeCollision = $state<FilePasteCollision | null>(null)
  let collisionRenameValue = $state('')
  let pendingFileOpen = $state<Promise<TabId> | null>(null)

  const sourceAttachmentCache = new Map<string, ReturnType<typeof fileTreeDragSource>>()
  const directoryAttachmentCache = new Map<string, ReturnType<typeof fileTreeDirectoryDropTarget>>()

  async function loadFiles() {
    error = null
    try {
      await refreshProjectFiles(projectId, projectPath)
    } catch (e) {
      console.error('Failed to load files:', e)
      error = e instanceof Error ? e.message : String(e)
    }
  }

  function clearAttachmentCaches() {
    sourceAttachmentCache.clear()
    directoryAttachmentCache.clear()
  }

  function sourceAttachmentKey(path: string, type: 'file' | 'directory'): string {
    return `${projectId}:src:${type}:${path}`
  }

  function directoryAttachmentKey(path: string): string {
    return `${projectId}:dir:${path}`
  }

  function dndSourceAttachment(node: FileTreeNode<{ path: string }>) {
    const key = sourceAttachmentKey(node.path, node.type)
    const cached = sourceAttachmentCache.get(key)
    if (cached) return cached
    const attachment = fileTreeDragSource({ projectId, node })
    sourceAttachmentCache.set(key, attachment)
    return attachment
  }

  function dndDirectoryAttachment(node: FileTreeNode<{ path: string }>) {
    if (node.type !== 'directory') return null
    const key = directoryAttachmentKey(node.path)
    const cached = directoryAttachmentCache.get(key)
    if (cached) return cached
    const attachment = fileTreeDirectoryDropTarget({
      projectId,
      directoryPath: node.path,
      onHoverExpand: () => {
        expandedDirs.add(node.path)
      },
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
      projectId,
      directoryPath: '.',
      onDrop: (payload) => handleDirectoryDrop('.', payload)
    })
    directoryAttachmentCache.set(key, attachment)
    return attachment
  }

  async function handleDirectoryDrop(targetDir: string, payload: DragPayload): Promise<void> {
    try {
      const externalSources: string[] = []
      let movedCount = 0

      for (const item of payload.items) {
        if (item.kind === 'file') {
          if (item.projectId !== projectId) continue
          const moved = await moveTreeItemToDirectory(item.path, targetDir)
          movedCount += moved ? 1 : 0
          if (pendingTransfer) return
        } else if (item.kind === 'os-files') {
          externalSources.push(...item.paths)
        }
      }

      if (externalSources.length > 0) {
        await runTransferWithCollisionHandling('copy', targetDir, externalSources)
        return
      }

      if (movedCount > 0) {
        await loadFiles()
        notify.success(`Moved ${movedCount} item${movedCount === 1 ? '' : 's'}`)
      }
    } catch (error) {
      notify.error('Drop failed', errMessage(error))
    }
  }

  async function moveTreeItemToDirectory(sourcePath: string, targetDir: string): Promise<boolean> {
    if (sourcePath === targetDir) {
      notify.info('Cannot move item', 'Destination is the same item.')
      return false
    }
    if (isEqualOrParent(sourcePath, targetDir)) {
      notify.warning('Cannot move item', 'A folder cannot be moved into itself or one of its children.')
      return false
    }
    const sourceParent = dirname(sourcePath) || '.'
    if (sourceParent === targetDir) {
      return false
    }

    const sourceAbs = await join(projectPath, sourcePath)
    const collisions = await backend.files.pasteCollisions(projectPath, targetDir, [sourceAbs])
    if (collisions.length > 0) {
      await runTransferWithCollisionHandling('cut', targetDir, [sourceAbs])
      return false
    }

    const nextPath = targetDir === '.' ? basename(sourcePath) : `${targetDir}/${basename(sourcePath)}`
    await backend.files.rename(projectPath, sourcePath, nextPath)
    return true
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

    const collisions = await backend.files.pasteCollisions(projectPath, targetDir, uniqueSources)
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
  }

  async function transferSourceWithPolicy(
    source: string,
    policy: 'auto_rename' | 'replace' | 'skip' | 'rename',
    renameTo?: string
  ): Promise<void> {
    if (!pendingTransfer) return
    const renameMap = policy === 'rename' && renameTo ? { [source]: renameTo } : undefined
    const created = await backend.files.paste(
      projectPath,
      pendingTransfer.targetDir,
      pendingTransfer.op,
      [source],
      policy,
      renameMap
    )
    pendingTransfer.created.push(...created)
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
      notify.error('Transfer failed', errMessage(error))
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
    const { op, created } = pendingTransfer
    const createdCount = created.length
    abortPendingTransfer()
    await loadFiles()

    if (createdCount === 0) {
      notify.info('Nothing transferred', 'All colliding items were skipped.')
      return
    }

    const verb = op === 'cut' ? 'Moved' : 'Pasted'
    notify.success(`${verb} ${createdCount} file${createdCount === 1 ? '' : 's'}`)
  }

  function toggleDir(path: string) {
    if (expandedDirs.has(path)) expandedDirs.delete(path)
    else expandedDirs.add(path)
  }

  function handleFileClick(filePath: string) {
    pendingFileOpen = openTextFile(projectId, filePath)
  }

  // Active file from the focused editor tab
  let focusedTab = $derived(getFocusedTab(projectId))
  let activeFilePath = $derived(focusedTab?.kind === 'text' ? focusedTab.filePath : null)

  // Reveal the active file in the tree by expanding every ancestor
  // directory. The effect deliberately depends on `loading` so that on
  // mount we re-expand AFTER the project effect has cleared the set
  // and loadFiles() finished — otherwise the user opens a tab from
  // (say) the git diff sidebar, switches back to Files, and finds the
  // tree collapsed despite a file being focused. Adding to a set is
  // idempotent; user-collapsed dirs only re-expand when the active
  // path itself changes, which mirrors VS Code's "reveal in explorer"
  // behaviour.
  $effect(() => {
    loading
    const path = activeFilePath
    if (!path) return
    const parts = path.split('/')
    for (let i = 1; i < parts.length; i++) {
      expandedDirs.add(parts.slice(0, i).join('/'))
    }
  })

  // Path -> status letter lookup from git state (prefer unstaged over staged)
  let gitSummary = $derived(getGitSummary(projectId))
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

  // Reload file list when project changes
  let prevProjectPath = ''
  $effect(() => {
    if (projectPath !== prevProjectPath) {
      prevProjectPath = projectPath
      expandedDirs.clear()
      filterQuery = ''
      clearAttachmentCaches()
      abortPendingTransfer()
      void ensureProjectFiles(projectId, projectPath)
    }
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
    if (!contextFilePath) return
    const absPath = await join(projectPath, contextFilePath)
    await revealItemInDir(absPath)
  }

  function handleOpenInEditor() {
    if (!contextFilePath) return
    openTextFile(projectId, contextFilePath)
  }

  async function handleOpenInFresh() {
    if (!contextFilePath) return
    await openTextInFresh(projectId, projectPath, contextFilePath)
  }

  function handleOpenDiff() {
    if (!contextFilePath) return
    openWorkingTreeDiff(projectId, false, contextFilePath, contextFilePath, { temporary: false })
  }

  async function handleCut() {
    if (!contextFilePath) return
    const absPath = await join(projectPath, contextFilePath)
    try {
      await backend.app.clipboardCopyFiles([absPath], 'cut')
    } catch (e) {
      notify.error('Cut failed', errMessage(e))
    }
  }

  async function handleCopy() {
    if (!contextFilePath) return
    const absPath = await join(projectPath, contextFilePath)
    try {
      await backend.app.clipboardCopyFiles([absPath], 'copy')
    } catch (e) {
      notify.error('Copy failed', errMessage(e))
    }
  }

  async function handlePaste() {
    const targetDir = contextTargetType === 'directory' && contextFilePath ? contextFilePath : '.'
    try {
      const clip = await backend.app.clipboardReadFiles()
      if (!clip || clip.paths.length === 0) {
        notify.info('Nothing to paste', 'No files on the clipboard.')
        return
      }
      await runTransferWithCollisionHandling(clip.op, targetDir, clip.paths)
    } catch (e) {
      notify.error('Paste failed', errMessage(e))
    }
  }

  async function handleCopyPath() {
    if (!contextFilePath) return
    const absPath = await join(projectPath, contextFilePath)
    await copyToClipboard(absPath)
  }

  async function handleCopyRelativePath() {
    if (!contextFilePath) return
    await copyToClipboard(contextFilePath)
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
      await backend.files.rename(projectPath, renameFilePath, renameValue)
      await renameTextPath(projectId, projectPath, renameFilePath, renameValue)
      await loadFiles()
    } catch (e) {
      notify.error('Rename failed', errMessage(e))
    } finally {
      renameFilePath = null
    }
  }

  function handleDelete() {
    if (!contextFilePath) return
    deleteFilePath = contextFilePath
    deleteConfirmOpen = true
  }

  async function confirmDelete() {
    if (!deleteFilePath) return
    try {
      await backend.files.delete(projectPath, deleteFilePath)
      await deleteTextPath(projectId, deleteFilePath)
      await loadFiles()
    } catch (e) {
      notify.error('Delete failed', errMessage(e))
    } finally {
      deleteConfirmOpen = false
      deleteFilePath = null
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
      if (kind === 'file') {
        await backend.files.write(projectPath, newItemName.trim(), '')
        await loadFiles()
        openTextFile(projectId, newItemName.trim())
      } else if (kind === 'folder') {
        await backend.files.createDir(projectPath, newItemName.trim())
        await loadFiles()
      }
    } catch (e) {
      notify.error(`Failed to create ${kind ?? 'item'}`, errMessage(e))
    } finally {
      newItemKind = null
      newItemName = ''
    }
  }

  function handleOpenExternal() {
    revealItemInDir(projectPath)
  }

  async function handleCopyProjectPath() {
    await copyToClipboard(projectPath)
  }
</script>

<SidebarPanel title="Files">
  {#snippet headerActions()}
    <IconButton tooltip="Refresh files" onclick={loadFiles}>
      <RotateCw size={11} />
    </IconButton>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <TreeFilterInput bind:value={filterQuery} placeholder="Filter files..." ariaLabel="Filter files" />
    <div
      class="min-h-0 flex-1 overflow-y-auto text-base {isFileTreeDropActive(projectId, '.') ? 'bg-accent/6' : ''}"
      {@attach rootDndAttachment()}
    >
      <FileContextMenu
        filePath={contextFilePath}
        targetType={contextTargetType}
        onRevealInFolder={handleRevealInFolder}
        onOpenInEditor={handleOpenInEditor}
        onOpenInFresh={handleOpenInFresh}
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
        onCopyProjectPath={handleCopyProjectPath}
        onResetTarget={resetContextTarget}
      >
        {#if loading}
          <div class="px-2.5 py-3 text-sm text-subtle">Loading files&hellip;</div>
        {:else if error}
          <div class="px-2.5 py-3 text-sm text-danger">{error}</div>
        {:else if fileTree.length === 0}
          <div class="px-2.5 py-3 text-sm text-subtle">No files found.</div>
        {:else}
          <FileTreeItems
            nodes={fileTree}
            isCollapsed={(path) => {
              if (filterActive && treeFilter.expand.has(path)) return false
              return !expandedDirs.has(path)
            }}
            isActive={(path) => path === activeFilePath}
            isDimmed={filterActive ? (node) => !treeFilter.matched.has(node.path) : undefined}
            hasDirChanges={(path) => dirsWithChanges.has(path)}
            onToggleDir={toggleDir}
            onFileClick={(node) => {
              if (node.change?.path) handleFileClick(node.change.path)
            }}
            onFileDblClick={() => promoteTabWhenReady(projectId, pendingFileOpen)}
            onFileContextMenu={handleFileContextMenu}
            dndEnabled={true}
            {dndSourceAttachment}
            {dndDirectoryAttachment}
            dndIsDropActive={(path) => isFileTreeDropActive(projectId, path)}
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

<ConfirmDialog
  open={deleteConfirmOpen}
  title="Delete File"
  message="Are you sure you want to delete {deleteFilePath}? This cannot be undone."
  confirmLabel="Delete"
  onCancel={() => {
    deleteConfirmOpen = false
    deleteFilePath = null
  }}
  onConfirm={confirmDelete}
/>
