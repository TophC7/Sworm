<script lang="ts">
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import {
    fetchFolder,
    forcePushWithLease,
    ignoreFiles,
    popLatestStash,
    promptStash,
    publishBranch,
    pullFolder,
    pushFolder,
    stageAllFolder,
    syncFolder,
    undoLastCommit,
    unstageAllFolder
  } from '$lib/features/git/actions.svelte'
  import {
    canOpenHead,
    canRevealInFileManager,
    confirmDiscardAll,
    confirmDiscardPaths,
    copyFullPatch,
    copyPatch,
    copyPath,
    copyRelativePath,
    describeTarget,
    openActualFile,
    openHeadFile,
    revealInExplorerView,
    revealInFileManager,
    runPathsAction,
    type GitTreeMenuTarget
  } from '$lib/features/git/treeActions'
  import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import {
    ArrowDown,
    ArrowDownUpIcon,
    ArrowUp,
    ClipboardIcon,
    CloudUploadIcon,
    Eye,
    EyeOff,
    FileDiff,
    FolderOpenIcon,
    ListTreeIcon,
    MinusCircle,
    PackageIcon,
    PackageOpenIcon,
    PlusCircle,
    RefreshCwIcon,
    RotateCw,
    SquareArrowOutUpRight,
    Trash2,
    Undo2Icon
  } from '$lib/icons/lucideExports'
  import type { GitSummary } from '$lib/types/backend'
  import type { Snippet } from 'svelte'

  let {
    folderPath,
    summary,
    target,
    onResetTarget,
    children
  }: {
    folderPath: string
    summary: GitSummary
    target: GitTreeMenuTarget | null
    onResetTarget: () => void
    children: Snippet
  } = $props()

  let staged = $derived(target?.section === 'staged')
  let remoteOk = $derived(canRevealInFileManager(folderPath))
  let trackedFiles = $derived.by(() => {
    if (!target || target.kind === 'file') return []
    const tracked = new Set(
      summary.changes.filter((change) => !change.staged && change.status !== '?').map((change) => change.path)
    )
    return target.files.filter((path) => tracked.has(path))
  })
</script>

{#snippet revealEntries(path: string)}
  <ContextMenu.Item onSelect={() => revealInExplorerView(folderPath, path)}>
    <ListTreeIcon size={14} />
    <span>Reveal in Explorer</span>
  </ContextMenu.Item>
  {#if remoteOk}
    <ContextMenu.Item onSelect={() => revealInFileManager(folderPath, path)}>
      <FolderOpenIcon size={14} />
      <span>Reveal in File Manager</span>
    </ContextMenu.Item>
  {/if}
{/snippet}

{#snippet copyEntries(path: string, files: string[], showPatch: boolean)}
  <ContextMenu.Item onSelect={() => copyPath(folderPath, path)}>
    <ClipboardIcon size={14} />
    <span>Copy Path</span>
  </ContextMenu.Item>
  <ContextMenu.Item onSelect={() => copyRelativePath(path)}>
    <ClipboardIcon size={14} />
    <span>Copy Relative Path</span>
  </ContextMenu.Item>
  {#if showPatch}
    <ContextMenu.Item onSelect={() => copyPatch(folderPath, files, staged, path)}>
      <ClipboardIcon size={14} />
      <span>Copy Patch</span>
    </ContextMenu.Item>
  {/if}
{/snippet}

<ContextMenu.Root>
  <ContextMenu.Trigger class="flex flex-1 flex-col" oncontextmenucapture={onResetTarget}>
    {@render children()}
  </ContextMenu.Trigger>

  <ContextMenu.Content>
    {#if target?.kind === 'file'}
      {@const change = target.change}
      {@const path = change.path}
      <ContextMenu.Item onSelect={() => openWorkingTreeDiff(folderPath, staged, path, path, { temporary: false })}>
        <FileDiff size={14} />
        <span>Open Changes</span>
      </ContextMenu.Item>
      {#if change.status !== 'D'}
        <ContextMenu.Item onSelect={() => openActualFile(folderPath, path)}>
          <SquareArrowOutUpRight size={14} />
          <span>Open File</span>
        </ContextMenu.Item>
      {/if}
      {#if canOpenHead(change)}
        <ContextMenu.Item onSelect={() => openHeadFile(folderPath, path)}>
          <Eye size={14} />
          <span>Open File (HEAD)</span>
        </ContextMenu.Item>
      {/if}

      <ContextMenu.Separator />
      {#if staged}
        <ContextMenu.Item onSelect={() => runPathsAction(folderPath, 'unstage', [path], path)}>
          <MinusCircle size={14} />
          <span>Unstage Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onSelect={() => runPathsAction(folderPath, 'stage', [path], path)}>
          <PlusCircle size={14} />
          <span>Stage Changes</span>
        </ContextMenu.Item>
        <ContextMenu.Item destructive onSelect={() => confirmDiscardPaths(folderPath, [path], path)}>
          <Trash2 size={14} />
          <span>Discard Changes</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={() => promptStash(folderPath, { kind: 'paths', paths: [path] })}>
        <PackageIcon size={14} />
        <span>Stash Changes…</span>
      </ContextMenu.Item>
      {#if !staged}
        <ContextMenu.Item onSelect={() => ignoreFiles(folderPath, [path])}>
          <EyeOff size={14} />
          <span>Add to .gitignore</span>
        </ContextMenu.Item>
      {/if}

      <ContextMenu.Separator />
      {@render revealEntries(path)}
      <ContextMenu.Separator />
      {@render copyEntries(path, [path], staged || change.status !== '?')}
    {:else if target?.kind === 'folder'}
      {@const folder = target}
      {#if staged}
        <ContextMenu.Item onSelect={() => runPathsAction(folderPath, 'unstage', folder.files, describeTarget(folder))}>
          <MinusCircle size={14} />
          <span>Unstage Changes</span>
        </ContextMenu.Item>
        <ContextMenu.Item
          onSelect={() => openWorkingTreeDiff(folderPath, true, folder.path, null, { temporary: false })}
        >
          <FileDiff size={14} />
          <span>Open Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onSelect={() => runPathsAction(folderPath, 'stage', folder.files, describeTarget(folder))}>
          <PlusCircle size={14} />
          <span>Stage Changes</span>
        </ContextMenu.Item>
        <ContextMenu.Item
          destructive
          onSelect={() => confirmDiscardPaths(folderPath, folder.files, describeTarget(folder))}
        >
          <Trash2 size={14} />
          <span>Discard Changes</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Item onSelect={() => promptStash(folderPath, { kind: 'paths', paths: [folder.path] })}>
        <PackageIcon size={14} />
        <span>Stash Changes…</span>
      </ContextMenu.Item>
      {#if !staged}
        <ContextMenu.Item onSelect={() => ignoreFiles(folderPath, [folder.path])}>
          <EyeOff size={14} />
          <span>Add to .gitignore</span>
        </ContextMenu.Item>
        <ContextMenu.Item
          onSelect={() => openWorkingTreeDiff(folderPath, false, folder.path, null, { temporary: false })}
        >
          <FileDiff size={14} />
          <span>Open Changes</span>
        </ContextMenu.Item>
      {/if}

      <ContextMenu.Separator />
      {@render revealEntries(folder.path)}
      <ContextMenu.Separator />
      {@render copyEntries(folder.path, staged ? folder.files : trackedFiles, true)}
    {:else if target?.kind === 'section'}
      {@const section = target}
      {@const empty = section.files.length === 0}
      {#if staged}
        <ContextMenu.Item disabled={empty} onSelect={() => unstageAllFolder(folderPath)}>
          <MinusCircle size={14} />
          <span>Unstage All Changes</span>
        </ContextMenu.Item>
        <ContextMenu.Item disabled={empty} onSelect={() => promptStash(folderPath, { kind: 'staged' })}>
          <PackageIcon size={14} />
          <span>Stash Staged…</span>
        </ContextMenu.Item>
        <ContextMenu.Item
          disabled={empty}
          onSelect={() => openWorkingTreeDiff(folderPath, true, null, null, { temporary: false })}
        >
          <FileDiff size={14} />
          <span>Open Staged Changes</span>
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item disabled={empty} onSelect={() => stageAllFolder(folderPath)}>
          <PlusCircle size={14} />
          <span>Stage All Changes</span>
        </ContextMenu.Item>
        <ContextMenu.Item disabled={empty} destructive onSelect={() => confirmDiscardAll(folderPath)}>
          <Trash2 size={14} />
          <span>Discard All Changes</span>
        </ContextMenu.Item>
        <ContextMenu.Item
          disabled={empty}
          onSelect={() => promptStash(folderPath, { kind: 'all', includeUntracked: true })}
        >
          <PackageIcon size={14} />
          <span>Stash (Include Untracked)…</span>
        </ContextMenu.Item>
        <ContextMenu.Item
          disabled={empty}
          onSelect={() => openWorkingTreeDiff(folderPath, false, null, null, { temporary: false })}
        >
          <FileDiff size={14} />
          <span>Open Changes</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Separator />
      <ContextMenu.Item
        disabled={empty}
        onSelect={() => copyPatch(folderPath, staged ? section.files : trackedFiles, staged, 'this section')}
      >
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {:else}
      {@const noUnstaged = summary.unstaged_count + summary.untracked_count === 0}
      <ContextMenu.Item onSelect={() => syncFolder(folderPath)}>
        <RefreshCwIcon size={14} />
        <span>Sync</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={() => pullFolder(folderPath)}>
        <ArrowDown size={14} />
        <span>Pull</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={() => pushFolder(folderPath)}>
        <ArrowUp size={14} />
        <span>Push</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={() => fetchFolder(folderPath)}>
        <RotateCw size={14} />
        <span>Fetch</span>
      </ContextMenu.Item>
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger>
          <ArrowDownUpIcon size={14} />
          <span>Pull, Push</span>
        </ContextMenu.SubTrigger>
        <ContextMenu.SubContent>
          <ContextMenu.Item onSelect={() => syncFolder(folderPath, true)}>
            <RefreshCwIcon size={14} />
            <span>Sync (Rebase)</span>
          </ContextMenu.Item>
          <ContextMenu.Item onSelect={() => pullFolder(folderPath, true)}>
            <ArrowDown size={14} />
            <span>Pull (Rebase)</span>
          </ContextMenu.Item>
          <ContextMenu.Item destructive onSelect={() => forcePushWithLease(folderPath)}>
            <ArrowUp size={14} />
            <span>Force Push (with lease)</span>
          </ContextMenu.Item>
          <ContextMenu.Separator />
          <ContextMenu.Item
            disabled={!summary.branch || summary.detached || summary.unborn || !!summary.upstream}
            onSelect={() => publishBranch(folderPath)}
          >
            <CloudUploadIcon size={14} />
            <span>Publish Branch…</span>
          </ContextMenu.Item>
        </ContextMenu.SubContent>
      </ContextMenu.Sub>

      <ContextMenu.Separator />
      <ContextMenu.Item disabled={noUnstaged} onSelect={() => stageAllFolder(folderPath)}>
        <PlusCircle size={14} />
        <span>Stage All Changes</span>
      </ContextMenu.Item>
      <ContextMenu.Item disabled={summary.staged_count === 0} onSelect={() => unstageAllFolder(folderPath)}>
        <MinusCircle size={14} />
        <span>Unstage All Changes</span>
      </ContextMenu.Item>
      <ContextMenu.Item disabled={noUnstaged} destructive onSelect={() => confirmDiscardAll(folderPath)}>
        <Trash2 size={14} />
        <span>Discard All Changes</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />
      <ContextMenu.Item
        disabled={summary.staged_count + summary.unstaged_count === 0}
        onSelect={() => promptStash(folderPath, { kind: 'all', includeUntracked: false })}
      >
        <PackageIcon size={14} />
        <span>Stash…</span>
      </ContextMenu.Item>
      <ContextMenu.Item
        disabled={summary.changes.length === 0}
        onSelect={() => promptStash(folderPath, { kind: 'all', includeUntracked: true })}
      >
        <PackageIcon size={14} />
        <span>Stash (Include Untracked)…</span>
      </ContextMenu.Item>
      <ContextMenu.Item
        disabled={summary.staged_count === 0}
        onSelect={() => promptStash(folderPath, { kind: 'staged' })}
      >
        <PackageIcon size={14} />
        <span>Stash Staged…</span>
      </ContextMenu.Item>
      <ContextMenu.Item onSelect={() => popLatestStash(folderPath)}>
        <PackageOpenIcon size={14} />
        <span>Pop Latest Stash</span>
      </ContextMenu.Item>

      <ContextMenu.Separator />
      <ContextMenu.Item
        disabled={noUnstaged}
        onSelect={() => openWorkingTreeDiff(folderPath, false, null, null, { temporary: false })}
      >
        <FileDiff size={14} />
        <span>Open Changes</span>
      </ContextMenu.Item>
      {#if !summary.unborn && summary.operation === null}
        <ContextMenu.Item destructive onSelect={() => undoLastCommit(folderPath)}>
          <Undo2Icon size={14} />
          <span>Undo Last Commit</span>
        </ContextMenu.Item>
      {/if}
      <ContextMenu.Separator />
      <ContextMenu.Item onSelect={() => copyFullPatch(folderPath)}>
        <ClipboardIcon size={14} />
        <span>Copy Patch</span>
      </ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
