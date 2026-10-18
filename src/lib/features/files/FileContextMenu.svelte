<script lang="ts">
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import { ignoreFiles } from '$lib/features/git/actions.svelte'
  import type { PathGitState } from '$lib/features/git/gitStatus'
  import {
    confirmDiscardPaths,
    describeFolder,
    openHeadFile,
    revealInSourceControl,
    runPathsAction
  } from '$lib/features/git/treeActions'
  import type { CompareFileSide } from '$lib/features/workbench/model'
  import {
    canCompareWithSelected,
    compareWithClipboard,
    compareWithSelected,
    selectForCompare
  } from '$lib/features/workbench/surfaces/diff/compare.svelte'
  import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import {
    FolderOpenIcon,
    FileCodeIcon,
    FileDiff,
    Eye,
    EyeOff,
    PlusCircle,
    MinusCircle,
    GitBranchIcon,
    GitCompareIcon,
    ScissorsIcon,
    CopyIcon,
    ClipboardIcon,
    PencilIcon,
    Trash2,
    Plus,
    SquareArrowOutUpRight,
    ClipboardPasteIcon
  } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'

  let {
    folderPath,
    filePath,
    targetType,
    git,
    children,
    canRevealInFileManager,
    canNativeFileClipboard,
    onRevealInFolder,
    onOpenInEditor,
    onCut,
    onCopy,
    onPaste,
    onCopyPath,
    onCopyRelativePath,
    onRename,
    onDelete,
    onNewFile,
    onNewFolder,
    onOpenExternal,
    onCopyFolderPath,
    onResetTarget
  }: {
    folderPath: string
    filePath: string | null
    targetType: 'file' | 'directory' | null
    git: PathGitState | null
    children: Snippet
    canRevealInFileManager: boolean
    canNativeFileClipboard: boolean
    onRevealInFolder: () => void
    onOpenInEditor: () => void
    onCut: () => void
    onCopy: () => void
    onPaste: () => void
    onCopyPath: () => void
    onCopyRelativePath: () => void
    onRename: () => void
    onDelete: () => void
    onNewFile: () => void
    onNewFolder: () => void
    onOpenExternal: () => void
    onCopyFolderPath: () => void
    onResetTarget: () => void
  } = $props()

  const file = $derived<CompareFileSide | null>(
    filePath && targetType === 'file' ? { kind: 'file', folderPath, path: filePath } : null
  )
  const changed = $derived(!!git && git.staged.length + git.unstaged.length > 0)
</script>

<ContextMenu.Root>
  <!-- svelte-ignore event_directive_deprecated -->
  <ContextMenu.Trigger class="block min-h-full" oncontextmenucapture={onResetTarget}>
    {@render children()}
  </ContextMenu.Trigger>

  <ContextMenu.Content>
    {#if filePath}
      {@const path = filePath}
      {@const describe = (files: string[]) => (targetType === 'file' ? path : describeFolder(path, files.length))}
      <!-- ── File/folder-targeted menu ── -->
      {#if canRevealInFileManager}
        <ContextMenu.Item onSelect={onRevealInFolder}>
          <FolderOpenIcon size={14} />
          <span>Reveal in File Manager</span>
        </ContextMenu.Item>
      {/if}

      {#if targetType === 'file'}
        {#if canRevealInFileManager}
          <ContextMenu.Separator />
        {/if}

        <ContextMenu.Item onSelect={onOpenInEditor}>
          <FileCodeIcon size={14} />
          <span>Open in Editor</span>
        </ContextMenu.Item>

        {#if git && changed}
          {@const state = git}
          <ContextMenu.Item
            onSelect={() =>
              openWorkingTreeDiff(folderPath, state.unstaged.length === 0, path, path, { temporary: false })}
          >
            <FileDiff size={14} />
            <span>Open Changes</span>
          </ContextMenu.Item>
        {/if}
        {#if git?.inHead}
          <ContextMenu.Item onSelect={() => openHeadFile(folderPath, path)}>
            <Eye size={14} />
            <span>Open File (HEAD)</span>
          </ContextMenu.Item>
        {/if}
      {/if}

      {#if git}
        {@const state = git}
        {#if canRevealInFileManager || targetType === 'file'}
          <ContextMenu.Separator />
        {/if}
        {#if targetType === 'directory' && changed}
          <ContextMenu.Item
            onSelect={() =>
              openWorkingTreeDiff(folderPath, state.unstaged.length === 0, path, null, { temporary: false })}
          >
            <FileDiff size={14} />
            <span>Open Changes</span>
          </ContextMenu.Item>
        {/if}
        {#if state.unstaged.length > 0}
          <ContextMenu.Item
            onSelect={() => runPathsAction(folderPath, 'stage', state.unstaged, describe(state.unstaged))}
          >
            <PlusCircle size={14} />
            <span>Stage Changes</span>
          </ContextMenu.Item>
        {/if}
        {#if state.staged.length > 0}
          <ContextMenu.Item
            onSelect={() => runPathsAction(folderPath, 'unstage', state.staged, describe(state.staged))}
          >
            <MinusCircle size={14} />
            <span>Unstage Changes</span>
          </ContextMenu.Item>
        {/if}
        {#if state.unstaged.length > 0}
          <ContextMenu.Item
            destructive
            onSelect={() => confirmDiscardPaths(folderPath, state.unstaged, describe(state.unstaged))}
          >
            <Trash2 size={14} />
            <span>Discard Changes</span>
          </ContextMenu.Item>
          <ContextMenu.Item onSelect={() => ignoreFiles(folderPath, [path])}>
            <EyeOff size={14} />
            <span>Add to .gitignore</span>
          </ContextMenu.Item>
        {/if}
        <ContextMenu.Item onSelect={() => revealInSourceControl(folderPath, path)}>
          <GitBranchIcon size={14} />
          <span>Reveal in Source Control</span>
        </ContextMenu.Item>
      {/if}

      {#if file}
        {@const compareFile = file}
        <ContextMenu.Separator />
        <ContextMenu.Item onSelect={() => selectForCompare(compareFile)}>
          <GitCompareIcon size={14} />
          <span>Select for Compare</span>
        </ContextMenu.Item>
        {#if canCompareWithSelected(compareFile)}
          <ContextMenu.Item onSelect={() => compareWithSelected(compareFile)}>
            <GitCompareIcon size={14} />
            <span>Compare with Selected</span>
          </ContextMenu.Item>
        {/if}
        <ContextMenu.Item onSelect={() => void compareWithClipboard(compareFile)}>
          <ClipboardPasteIcon size={14} />
          <span>Compare with Clipboard</span>
        </ContextMenu.Item>
      {/if}

      {#if (canRevealInFileManager || targetType === 'file' || git !== null) && canNativeFileClipboard}
        <ContextMenu.Separator />
      {/if}

      {#if canNativeFileClipboard}
        <ContextMenu.Item onSelect={onCut}>
          <ScissorsIcon size={14} />
          <span>Cut</span>
        </ContextMenu.Item>
        <ContextMenu.Item onSelect={onCopy}>
          <CopyIcon size={14} />
          <span>Copy</span>
        </ContextMenu.Item>
        {#if targetType === 'directory'}
          <ContextMenu.Item onSelect={onPaste}>
            <ClipboardPasteIcon size={14} />
            <span>Paste</span>
          </ContextMenu.Item>
        {/if}
      {/if}

      {#if canNativeFileClipboard}<ContextMenu.Separator />{/if}

      <ContextMenu.Item onSelect={onCopyPath}>
        <ClipboardIcon size={14} />
        <span>Copy Path</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={onCopyRelativePath}>
        <ClipboardIcon size={14} />
        <span>Copy Relative Path</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onSelect={onRename}>
        <PencilIcon size={14} />
        <span>Rename</span>
      </ContextMenu.Item>
      <ContextMenu.Item destructive onSelect={onDelete}>
        <Trash2 size={14} />
        <span>Delete</span>
      </ContextMenu.Item>
    {:else}
      <!-- ── Empty space menu ── -->
      <ContextMenu.Item onSelect={onNewFile}>
        <Plus size={14} />
        <span>New File</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={onNewFolder}>
        <FolderOpenIcon size={14} />
        <span>New Folder</span>
      </ContextMenu.Item>
      <ContextMenu.Separator />

      {#if canNativeFileClipboard}
        <ContextMenu.Item onSelect={onPaste}>
          <ClipboardPasteIcon size={14} />
          <span>Paste</span>
        </ContextMenu.Item>
      {/if}

      {#if canNativeFileClipboard}<ContextMenu.Separator />{/if}

      {#if canRevealInFileManager}
        <ContextMenu.Item onSelect={onOpenExternal}>
          <SquareArrowOutUpRight size={14} />
          <span>Reveal in File Manager</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={onCopyFolderPath}>
        <ClipboardIcon size={14} />
        <span>Copy Path</span>
      </ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
