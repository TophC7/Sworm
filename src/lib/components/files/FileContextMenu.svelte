<script lang="ts">
  import {
    ContextMenuRoot,
    ContextMenuTrigger,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuSeparator,
    ContextMenuSub,
    ContextMenuSubTrigger,
    ContextMenuSubContent
  } from '$lib/components/ui/context-menu'
  import {
    FolderOpen,
    FileCodeIcon,
    TerminalIcon,
    FileDiff,
    ScissorsIcon,
    CopyIcon,
    ClipboardIcon,
    PencilIcon,
    Trash2,
    ChevronRight,
    Plus,
    SquareArrowOutUpRight,
    ClipboardPasteIcon
  } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'

  let {
    filePath,
    targetType,
    children,
    onRevealInFolder,
    onOpenInMonaco,
    onOpenInFresh,
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
    onCopyProjectPath,
    onResetTarget
  }: {
    filePath: string | null
    targetType: 'file' | 'directory' | null
    children: Snippet
    onRevealInFolder: () => void
    onOpenInMonaco: () => void
    onOpenInFresh: () => void
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
    onCopyProjectPath: () => void
    onResetTarget: () => void
  } = $props()
</script>

<ContextMenuRoot>
  <!-- svelte-ignore event_directive_deprecated -->
  <ContextMenuTrigger class="block min-h-full" oncontextmenucapture={onResetTarget}>
    {@render children()}
  </ContextMenuTrigger>

  <ContextMenuContent>
    {#if filePath}
      <!-- ── File/folder-targeted menu ── -->
      <ContextMenuItem onclick={onRevealInFolder}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>Reveal in File Manager</span>
      </ContextMenuItem>

      {#if targetType === 'file'}
        <ContextMenuSeparator />

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <FileCodeIcon size={14} class="shrink-0 text-muted" />
            <span class="flex-1">Open With</span>
            <ChevronRight size={12} class="shrink-0 text-subtle" />
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuItem onclick={onOpenInMonaco}>
              <FileCodeIcon size={14} class="shrink-0 text-muted" />
              <span>Monaco Editor</span>
            </ContextMenuItem>
            <ContextMenuItem onclick={onOpenInFresh}>
              <TerminalIcon size={14} class="shrink-0 text-muted" />
              <span>Fresh</span>
            </ContextMenuItem>
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuItem onclick={onOpenDiff}>
          <FileDiff size={14} class="shrink-0 text-muted" />
          <span>Open Diff</span>
        </ContextMenuItem>
      {/if}

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onCut}>
        <ScissorsIcon size={14} class="shrink-0 text-muted" />
        <span>Cut</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onCopy}>
        <CopyIcon size={14} class="shrink-0 text-muted" />
        <span>Copy</span>
      </ContextMenuItem>
      {#if targetType === 'directory'}
        <ContextMenuItem onclick={onPaste}>
          <ClipboardPasteIcon size={14} class="shrink-0 text-muted" />
          <span>Paste</span>
        </ContextMenuItem>
      {/if}

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onCopyPath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Path</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onCopyRelativePath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Relative Path</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onRename}>
        <PencilIcon size={14} class="shrink-0 text-muted" />
        <span>Rename</span>
      </ContextMenuItem>
      <ContextMenuItem destructive onclick={onDelete}>
        <Trash2 size={14} class="shrink-0" />
        <span>Delete</span>
      </ContextMenuItem>
    {:else}
      <!-- ── Empty space menu ── -->
      <ContextMenuItem onclick={onNewFile}>
        <Plus size={14} class="shrink-0 text-muted" />
        <span>New File</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onNewFolder}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>New Folder</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onPaste}>
        <ClipboardPasteIcon size={14} class="shrink-0 text-muted" />
        <span>Paste</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onOpenExternal}>
        <SquareArrowOutUpRight size={14} class="shrink-0 text-muted" />
        <span>Reveal in File Manager</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onCopyProjectPath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Path</span>
      </ContextMenuItem>
    {/if}
  </ContextMenuContent>
</ContextMenuRoot>
