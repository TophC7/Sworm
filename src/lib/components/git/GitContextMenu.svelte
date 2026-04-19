<script lang="ts">
  import {
    ContextMenuRoot,
    ContextMenuTrigger,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuSeparator
  } from '$lib/components/ui/context-menu'
  import {
    FileDiff,
    Eye,
    FolderOpen,
    ClipboardIcon,
    PlusCircle,
    MinusCircle,
    Trash2,
    PackageIcon,
    ArrowUp,
    ArrowDown,
    RotateCw
  } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'

  let {
    filePath,
    targetType,
    isStaged,
    children,
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
    children: Snippet
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

<ContextMenuRoot>
  <ContextMenuTrigger class="flex flex-1 flex-col" oncontextmenucapture={onResetTarget}>
    {@render children()}
  </ContextMenuTrigger>

  <ContextMenuContent>
    {#if filePath && targetType === 'file'}
      <!-- ── File menu ── -->
      <ContextMenuItem disabled>
        <FileDiff size={14} class="shrink-0" />
        <span>Open Changes</span>
        <span class="ml-auto text-2xs text-subtle">WIP</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onOpenFile}>
        <Eye size={14} class="shrink-0 text-muted" />
        <span>Open File</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onOpenFileHead}>
        <Eye size={14} class="shrink-0 text-muted" />
        <span>Open File (HEAD)</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem destructive onclick={onDiscard}>
        <Trash2 size={14} class="shrink-0" />
        <span>Discard Changes</span>
      </ContextMenuItem>
      {#if isStaged}
        <ContextMenuItem onclick={onUnstage}>
          <MinusCircle size={14} class="shrink-0 text-muted" />
          <span>Unstage Changes</span>
        </ContextMenuItem>
      {:else}
        <ContextMenuItem onclick={onStage}>
          <PlusCircle size={14} class="shrink-0 text-muted" />
          <span>Stage Changes</span>
        </ContextMenuItem>
      {/if}
      <ContextMenuItem disabled>
        <PackageIcon size={14} class="shrink-0" />
        <span>Stash Changes</span>
        <span class="ml-auto text-2xs text-subtle">WIP</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onRevealInFolder}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>Reveal in File Manager</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onCopyPath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Path</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onCopyRelativePath}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Relative Path</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onCopyPatch}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Patch</span>
      </ContextMenuItem>
    {:else if filePath && targetType === 'directory'}
      <!-- ── Folder menu ── -->
      <ContextMenuItem destructive onclick={onDiscard}>
        <Trash2 size={14} class="shrink-0" />
        <span>Discard Changes</span>
      </ContextMenuItem>
      {#if isStaged}
        <ContextMenuItem onclick={onUnstage}>
          <MinusCircle size={14} class="shrink-0 text-muted" />
          <span>Unstage Changes</span>
        </ContextMenuItem>
      {:else}
        <ContextMenuItem onclick={onStage}>
          <PlusCircle size={14} class="shrink-0 text-muted" />
          <span>Stage Changes</span>
        </ContextMenuItem>
      {/if}
      <ContextMenuItem disabled>
        <FileDiff size={14} class="shrink-0" />
        <span>Open Changes</span>
        <span class="ml-auto text-2xs text-subtle">WIP</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onCopyFolderPatch}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Patch</span>
      </ContextMenuItem>
    {:else}
      <!-- ── Empty space menu ── -->
      <ContextMenuItem onclick={onPush}>
        <ArrowUp size={14} class="shrink-0 text-muted" />
        <span>Push</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onPull}>
        <ArrowDown size={14} class="shrink-0 text-muted" />
        <span>Pull</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={onFetch}>
        <RotateCw size={14} class="shrink-0 text-muted" />
        <span>Fetch</span>
      </ContextMenuItem>

      <ContextMenuSeparator />

      <ContextMenuItem onclick={onCopyFullPatch}>
        <ClipboardIcon size={14} class="shrink-0 text-muted" />
        <span>Copy Patch</span>
      </ContextMenuItem>
    {/if}
  </ContextMenuContent>
</ContextMenuRoot>
