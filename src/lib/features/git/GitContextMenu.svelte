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
      <ContextMenu.Item onclick={onOpenChanges}>
        <FileDiff size={14} />
        <span>Open Changes</span>
      </ContextMenu.Item>
      {#if canOpenFile}
        <ContextMenu.Item onclick={onOpenFile}>
          <SquareArrowOutUpRight size={14} />
          <span>Open File</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onclick={onOpenFileHead}>
        <Eye size={14} />
        <span>Open File (HEAD)</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      {#if !isStaged}
        <ContextMenu.Item destructive onclick={onDiscard}>
          <Trash2 size={14} />
          <span>Discard Changes</span>
        </ContextMenu.Item>
      {/if}
      {#if isStaged}
        <ContextMenu.Item onclick={onUnstage}>
          <MinusCircle size={14} />
          <span>Unstage Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onclick={onStage}>
          <PlusCircle size={14} />
          <span>Stage Changes</span>
        </ContextMenu.Item>
      {/if}

      {#if canRevealInFileManager}
        <ContextMenu.Separator />

        <ContextMenu.Item onclick={onRevealInFolder}>
          <FolderOpenIcon size={14} />
          <span>Reveal in File Manager</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Separator />

      <ContextMenu.Item onclick={onCopyPath}>
        <ClipboardIcon size={14} />
        <span>Copy Path</span>
      </ContextMenu.Item>
      <ContextMenu.Item onclick={onCopyRelativePath}>
        <ClipboardIcon size={14} />
        <span>Copy Relative Path</span>
      </ContextMenu.Item>
      <ContextMenu.Item onclick={onCopyPatch}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {:else if filePath && targetType === 'directory'}
      <!-- ── Folder menu ── -->
      {#if !isStaged}
        <ContextMenu.Item destructive onclick={onDiscard}>
          <Trash2 size={14} />
          <span>Discard Changes</span>
        </ContextMenu.Item>
      {/if}
      {#if isStaged}
        <ContextMenu.Item onclick={onUnstage}>
          <MinusCircle size={14} />
          <span>Unstage Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onclick={onStage}>
          <PlusCircle size={14} />
          <span>Stage Changes</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onclick={onOpenChanges}>
        <FileDiff size={14} />
        <span>Open Changes</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onclick={onCopyFolderPatch}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {:else}
      <!-- ── Empty space menu ── -->
      <ContextMenu.Item onclick={onPush}>
        <ArrowUp size={14} />
        <span>Push</span>
      </ContextMenu.Item>
      <ContextMenu.Item onclick={onPull}>
        <ArrowDown size={14} />
        <span>Pull</span>
      </ContextMenu.Item>
      <ContextMenu.Item onclick={onFetch}>
        <RotateCw size={14} />
        <span>Fetch</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />

      <ContextMenu.Item onclick={onCopyFullPatch}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
