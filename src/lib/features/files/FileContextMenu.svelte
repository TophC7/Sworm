<script lang="ts">
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import {
    FolderOpenIcon,
    FileCodeIcon,
    FileDiff,
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
    filePath,
    targetType,
    children,
    canRevealInFileManager,
    canNativeFileClipboard,
    onRevealInFolder,
    onOpenInEditor,
    onOpenDiff,
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
    filePath: string | null
    targetType: 'file' | 'directory' | null
    children: Snippet
    canRevealInFileManager: boolean
    canNativeFileClipboard: boolean
    onRevealInFolder: () => void
    onOpenInEditor: () => void
    onOpenDiff: () => void
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
</script>

<ContextMenu.Root>
  <!-- svelte-ignore event_directive_deprecated -->
  <ContextMenu.Trigger class="block min-h-full" oncontextmenucapture={onResetTarget}>
    {@render children()}
  </ContextMenu.Trigger>

  <ContextMenu.Content>
    {#if filePath}
      <!-- ── File/folder-targeted menu ── -->
      {#if canRevealInFileManager}
        <ContextMenu.Item onclick={onRevealInFolder}>
          <FolderOpenIcon size={14} class="shrink-0 text-muted" />
          <span>Reveal in File Manager</span>
        </ContextMenu.Item>
      {/if}

      {#if targetType === 'file'}
        {#if canRevealInFileManager}
          <ContextMenu.Separator />
        {/if}

        <ContextMenu.Item onclick={onOpenInEditor}>
          <FileCodeIcon size={14} class="shrink-0 text-muted" />
          <span>Open in Editor</span>
        </ContextMenu.Item>

        <ContextMenu.Item onclick={onOpenDiff}>
          <FileDiff size={14} class="shrink-0 text-muted" />
          <span>Open Diff</span>
        </ContextMenu.Item>
      {/if}

      {#if (canRevealInFileManager || targetType === 'file') && canNativeFileClipboard}
        <ContextMenu.Separator />
      {/if}

      {#if canNativeFileClipboard}
        <ContextMenu.Item onclick={onCut}>
          <ScissorsIcon size={14} class="shrink-0 text-muted" />
          <span>Cut</span>
        </ContextMenu.Item>
        <ContextMenu.Item onclick={onCopy}>
          <CopyIcon size={14} class="shrink-0 text-muted" />
          <span>Copy</span>
        </ContextMenu.Item>
        {#if targetType === 'directory'}
          <ContextMenu.Item onclick={onPaste}>
            <ClipboardPasteIcon size={14} class="shrink-0 text-muted" />
            <span>Paste</span>
          </ContextMenu.Item>
        {/if}
      {/if}

      {#if canNativeFileClipboard}<ContextMenu.Separator />{/if}

      <ContextMenu.Item onclick={onCopyPath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Path</span>
      </ContextMenu.Item>
      <ContextMenu.Item onclick={onCopyRelativePath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Relative Path</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onclick={onRename}>
        <PencilIcon size={14} class="shrink-0 text-muted" />
        <span>Rename</span>
      </ContextMenu.Item>
      <ContextMenu.Item destructive onclick={onDelete}>
        <Trash2 size={14} class="shrink-0" />
        <span>Delete</span>
      </ContextMenu.Item>
    {:else}
      <!-- ── Empty space menu ── -->
      <ContextMenu.Item onclick={onNewFile}>
        <Plus size={14} class="shrink-0 text-muted" />
        <span>New File</span>
      </ContextMenu.Item>
      <ContextMenu.Item onclick={onNewFolder}>
        <FolderOpenIcon size={14} class="shrink-0 text-muted" />
        <span>New Folder</span>
      </ContextMenu.Item>
      <ContextMenu.Separator />

      {#if canNativeFileClipboard}
        <ContextMenu.Item onclick={onPaste}>
          <ClipboardPasteIcon size={14} class="shrink-0 text-muted" />
          <span>Paste</span>
        </ContextMenu.Item>
      {/if}

      {#if canNativeFileClipboard}<ContextMenu.Separator />{/if}

      {#if canRevealInFileManager}
        <ContextMenu.Item onclick={onOpenExternal}>
          <SquareArrowOutUpRight size={14} class="shrink-0 text-muted" />
          <span>Reveal in File Manager</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onclick={onCopyFolderPath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Path</span>
      </ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
