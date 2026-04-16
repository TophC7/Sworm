<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import FileContextMenu from '$lib/components/files/FileContextMenu.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import PromptDialog from '$lib/components/PromptDialog.svelte'
  import SidebarPanel from '$lib/components/SidebarPanel.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { backend } from '$lib/api/backend'
  import { getGitSummary } from '$lib/stores/git.svelte'
  import GitStatusBadge from '$lib/components/git/GitStatusBadge.svelte'
  import { RotateCw } from '$lib/icons/lucideExports'
  import { openFile, ensureFreshSession } from '$lib/utils/openFile'
  import { addChangesTab } from '$lib/stores/workspace.svelte'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { getFocusedTab } from '$lib/stores/workspace.svelte'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import { notify } from '$lib/stores/notifications.svelte'
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
  let loading = $state(true)
  let error = $state<string | null>(null)
  let fileTree = $state<FileTreeNode<{ path: string }>[]>([])

  let contextFilePath = $state<string | null>(null)
  let contextTargetType = $state<'file' | 'directory' | null>(null)

  let renameFilePath = $state<string | null>(null)
  let renameValue = $state('')

  let newItemKind = $state<'file' | 'folder' | null>(null)
  let newItemName = $state('')

  let deleteConfirmOpen = $state(false)
  let deleteFilePath = $state<string | null>(null)

  async function loadFiles() {
    loading = true
    error = null
    try {
      const paths = await backend.files.listAll(projectPath)
      fileTree = buildFileTree(paths.map((p) => ({ path: p })))
    } catch (e) {
      console.error('Failed to load files:', e)
      error = e instanceof Error ? e.message : String(e)
    } finally {
      loading = false
    }
  }

  function toggleDir(path: string) {
    if (expandedDirs.has(path)) expandedDirs.delete(path)
    else expandedDirs.add(path)
  }

  function handleFileClick(filePath: string) {
    openFile(projectId, projectPath, filePath)
  }

  // Active file from the focused editor tab
  let focusedTab = $derived(getFocusedTab(projectId))
  let activeFilePath = $derived(focusedTab?.kind === 'editor' ? focusedTab.filePath : null)

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
      loadFiles()
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

  function handleOpenInMonaco() {
    if (!contextFilePath) return
    openFile(projectId, projectPath, contextFilePath)
  }

  async function handleOpenInFresh() {
    if (!contextFilePath) return
    await ensureFreshSession(projectId)
  }

  function handleOpenDiff() {
    if (!contextFilePath) return
    addChangesTab(projectId, false, contextFilePath)
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
      const created = await backend.files.paste(projectPath, targetDir, clip.op, clip.paths)
      await loadFiles()
      const verb = clip.op === 'cut' ? 'Moved' : 'Pasted'
      notify.success(`${verb} ${created.length} file${created.length === 1 ? '' : 's'}`)
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
        openFile(projectId, projectPath, newItemName.trim())
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

  <div class="h-full overflow-y-auto text-[0.78rem]">
    <FileContextMenu
      filePath={contextFilePath}
      targetType={contextTargetType}
      onRevealInFolder={handleRevealInFolder}
      onOpenInMonaco={handleOpenInMonaco}
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
        <div class="px-2.5 py-3 text-[0.75rem] text-subtle">Loading files&hellip;</div>
      {:else if error}
        <div class="px-2.5 py-3 text-[0.75rem] text-danger">{error}</div>
      {:else if fileTree.length === 0}
        <div class="px-2.5 py-3 text-[0.75rem] text-subtle">No files found.</div>
      {:else}
        <FileTreeItems
          nodes={fileTree}
          isCollapsed={(path) => !expandedDirs.has(path)}
          isActive={(path) => path === activeFilePath}
          hasDirChanges={(path) => dirsWithChanges.has(path)}
          onToggleDir={toggleDir}
          onFileClick={(node) => {
            if (node.change?.path) handleFileClick(node.change.path)
          }}
          onFileContextMenu={handleFileContextMenu}
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
</SidebarPanel>

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
