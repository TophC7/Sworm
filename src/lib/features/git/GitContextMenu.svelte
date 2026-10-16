<script lang="ts">
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import {
    FileDiff,
    Eye,
    FolderOpenIcon,
    ClipboardIcon,
    PlusCircle,
    MinusCircle,
    Trash2,
    ArrowUp,
    ArrowDown,
    RotateCw,
    SquareArrowOutUpRight
  } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'

  let {
    filePath,
    targetType,
    isStaged,
    canOpenFile = true,
    canRevealInFileManager = true,
    children,
    onOpenChanges,
    onOpenFile,
    onOpenFileHead,
    onStage,
    onUnstage,
    onDiscard,
    onRevealInFolder,
    onCopyPath,
    onCopyRelativePath,
    onCopyPatch,
    onCopyFolderPatch,
    onPush,
    onPull,
    onFetch,
    onCopyFullPatch,
    onResetTarget
  }: {
    filePath: string | null
    targetType: 'file' | 'directory' | null
    isStaged: boolean
    canOpenFile?: boolean
    canRevealInFileManager?: boolean
    children: Snippet
    onOpenChanges: () => void
    onOpenFile: () => void
    onOpenFileHead: () => void
    onStage: () => void
    onUnstage: () => void
    onDiscard: () => void
    onRevealInFolder: () => void
    onCopyPath: () => void
    onCopyRelativePath: () => void
    onCopyPatch: () => void
    onCopyFolderPatch: () => void
    onPush: () => void
    onPull: () => void
    onFetch: () => void
    onCopyFullPatch: () => void
    onResetTarget: () => void
  } = $props()
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger class="flex flex-1 flex-col" oncontextmenucapture={onResetTarget}>
    {@render children()}
  </ContextMenu.Trigger>

  <ContextMenu.Content>
    {#if filePath && targetType === 'file'}
      <!-- ── File menu ── -->
      <ContextMenu.Item onSelect={onOpenChanges}>
        <FileDiff size={14} />
        <span>Open Changes</span>
      </ContextMenu.Item>
      {#if canOpenFile}
        <ContextMenu.Item onSelect={onOpenFile}>
          <SquareArrowOutUpRight size={14} />
          <span>Open File</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={onOpenFileHead}>
        <Eye size={14} />
        <span>Open File (HEAD)</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      {#if !isStaged}
        <ContextMenu.Item destructive onSelect={onDiscard}>
          <Trash2 size={14} />
          <span>Discard Changes</span>
        </ContextMenu.Item>
      {/if}
      {#if isStaged}
        <ContextMenu.Item onSelect={onUnstage}>
          <MinusCircle size={14} />
          <span>Unstage Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onSelect={onStage}>
          <PlusCircle size={14} />
          <span>Stage Changes</span>
        </ContextMenu.Item>
      {/if}

      {#if canRevealInFileManager}
        <ContextMenu.Separator />

        <ContextMenu.Item onSelect={onRevealInFolder}>
          <FolderOpenIcon size={14} />
          <span>Reveal in File Manager</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Separator />

      <ContextMenu.Item onSelect={onCopyPath}>
        <ClipboardIcon size={14} />
        <span>Copy Path</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={onCopyRelativePath}>
        <ClipboardIcon size={14} />
        <span>Copy Relative Path</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={onCopyPatch}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {:else if filePath && targetType === 'directory'}
      <!-- ── Folder menu ── -->
      {#if !isStaged}
        <ContextMenu.Item destructive onSelect={onDiscard}>
          <Trash2 size={14} />
          <span>Discard Changes</span>
        </ContextMenu.Item>
      {/if}
      {#if isStaged}
        <ContextMenu.Item onSelect={onUnstage}>
          <MinusCircle size={14} />
          <span>Unstage Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onSelect={onStage}>
          <PlusCircle size={14} />
          <span>Stage Changes</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={onOpenChanges}>
        <FileDiff size={14} />
        <span>Open Changes</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onSelect={onCopyFolderPatch}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {:else}
      <!-- ── Empty space menu ── -->
      <ContextMenu.Item onSelect={onPush}>
        <ArrowUp size={14} />
        <span>Push</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={onPull}>
        <ArrowDown size={14} />
        <span>Pull</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={onFetch}>
        <RotateCw size={14} />
        <span>Fetch</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onSelect={onCopyFullPatch}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
