<script lang="ts">
  import GitGraph from '$lib/components/git/GitGraph.svelte'
  import GitFileTree from '$lib/components/git/GitFileTree.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import SidebarPanel from '$lib/components/SidebarPanel.svelte'
  import { Button, IconButton } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { refreshGit, runGitAction } from '$lib/stores/git.svelte'
  import { backend } from '$lib/api/backend'
  import type { GitSummary } from '$lib/types/backend'
  import { GitBranchIcon, RotateCw } from '$lib/icons/lucideExports'
  import {
    getGitActionNotifications,
    gitCommitNotifications,
    type GitActionKind
  } from '$lib/utils/gitActionNotifications'
  import { getErrorMessage, runNotifiedTask } from '$lib/utils/notifiedTask'

  let {
    summary,
    projectId,
    projectPath,
    onRefresh,
    onFileClick,
    onPersistTab,
    onCommitFileClick,
    onStashFileClick,
    onViewAllChanges
  }: {
    summary: GitSummary | null
    projectId: string
    projectPath: string
    onRefresh?: () => void
    onFileClick?: (filePath: string, staged: boolean) => void
    onPersistTab?: () => void
    onCommitFileClick?: (hash: string, shortHash: string, message: string, filePath: string) => void
    onStashFileClick?: (stashIndex: number, message: string, filePath: string) => void
    onViewAllChanges?: (staged: boolean) => void
  } = $props()

  let hasCommits = $derived(!!summary?.branch)
  let isRepo = $derived(summary?.is_repo ?? true)

  // Confirmation dialog state
  let showDiscardConfirm = $state(false)
  let showUndoCommitConfirm = $state(false)

  // Commit-message textarea content, owned here so an undo-commit can
  // push the previous commit message back into the editor without
  // losing child component state.
  let commitMessage = $state('')

  // Init/clone state
  let cloneUrl = $state('')
  let initBusy = $state(false)
  let initError = $state<string | null>(null)

  async function handleInit() {
    initBusy = true
    initError = null
    await runNotifiedTask(
      async () => {
        await backend.git.init(projectPath)
        await refreshGit(projectId, projectPath)
      },
      {
        loading: { title: 'Initializing repository' },
        success: { title: 'Repository initialized' },
        error: {
          title: 'Initialize repository failed',
          description: (error) => {
            const message = getErrorMessage(error)
            initError = message
            return message
          }
        }
      }
    )
    initBusy = false
  }

  async function handleClone() {
    if (!cloneUrl.trim()) return
    initBusy = true
    initError = null
    const targetUrl = cloneUrl.trim()
    let cloneSucceeded = false
    await runNotifiedTask(
      async () => {
        await backend.git.cloneInPlace(projectPath, targetUrl)
        await refreshGit(projectId, projectPath)
        cloneSucceeded = true
      },
      {
        loading: { title: 'Cloning repository', description: targetUrl },
        success: { title: 'Repository cloned', description: targetUrl },
        error: {
          title: 'Clone failed',
          description: (error) => {
            const message = getErrorMessage(error)
            initError = message
            return message
          }
        }
      }
    )
    if (cloneSucceeded) cloneUrl = ''
    initBusy = false
  }

  async function refresh() {
    await refreshGit(projectId, projectPath)
  }

  async function handleGitAction<T = void>(
    kind: GitActionKind,
    fn: (path: string) => Promise<T>
  ): Promise<T | undefined> {
    return runNotifiedTask(() => runGitAction(projectId, projectPath, fn), getGitActionNotifications<T>(kind))
  }

  async function handleCommit(message: string) {
    await runNotifiedTask(
      () => runGitAction(projectId, projectPath, (path) => backend.git.commit(path, message)),
      gitCommitNotifications
    )
  }

  async function handleStageAll() {
    await handleGitAction('stageAll', (path) => backend.git.stageAll(path))
  }

  async function handleUnstageAll() {
    await handleGitAction('unstageAll', (path) => backend.git.unstageAll(path))
  }

  async function handleDiscardAll() {
    showDiscardConfirm = false
    await handleGitAction('discardAll', (path) => backend.git.discardAll(path))
  }

  async function handleStashAll() {
    await handleGitAction('stashAll', (path) => backend.git.stashAll(path))
  }

  async function handleUndoLastCommit() {
    showUndoCommitConfirm = false
    // Thread the returned commit message back into the textarea so the
    // user can edit or re-commit. Undefined means the task threw, in
    // which case the notification already explained the failure.
    const message = await handleGitAction('undoLastCommit', (path) => backend.git.undoLastCommit(path))
    if (typeof message === 'string' && message.length > 0) {
      commitMessage = message
    }
  }

  async function handlePush() {
    await handleGitAction('push', (path) => backend.git.push(path))
  }

  async function handlePushForceWithLease() {
    await handleGitAction('forcePush', (path) => backend.git.pushForceWithLease(path))
  }

  async function handlePull() {
    await handleGitAction('pull', (path) => backend.git.pull(path))
  }

  async function handleFetch() {
    await handleGitAction('fetch', (path) => backend.git.fetch(path))
  }
</script>

<SidebarPanel title="Git">
  {#snippet headerActions()}
    {#if onRefresh}
      <IconButton tooltip="Refresh git status" onclick={onRefresh}>
        <RotateCw size={11} />
      </IconButton>
    {/if}
  {/snippet}
  {#snippet headerExtra()}
    <InfoTooltip ariaLabel="Explain git status badges" contentClass="w-72">
      <div class="space-y-2">
        <p class="font-medium text-bright">Git badges in this panel</p>
        <div class="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1">
          <span class="font-mono font-bold text-warning">M</span>
          <span>Modified content</span>
          <span class="font-mono font-bold text-success">U</span>
          <span>Untracked (new file, not yet staged)</span>
          <span class="font-mono font-bold text-danger">D</span>
          <span>File deleted</span>
          <span class="font-mono font-bold text-accent">R</span>
          <span>File renamed according to git</span>
          <span class="font-mono font-bold">
            <span class="text-success">+12</span>
            <span class="text-muted"> / </span>
            <span class="text-danger">-4</span>
          </span>
          <span
            >Net line change. <code>+</code> means more lines added than removed, <code>-</code> means more lines removed
            than added.</span
          >
        </div>
        <p class="text-muted">
          The same file can appear in both sections if it has staged changes and newer unstaged edits.
        </p>
      </div>
    </InfoTooltip>
  {/snippet}

  {#if !summary}
    <div class="px-2.5 py-3 text-sm text-subtle">Loading git info&hellip;</div>
  {:else if !isRepo}
    <!-- Not a git repository — offer init or clone -->
    <div class="flex flex-col gap-4 px-3 py-4">
      <div class="flex flex-col items-center gap-2 py-4 text-center">
        <GitBranchIcon size={28} class="text-subtle" />
        <p class="text-sm text-muted">This folder is not a git repository.</p>
      </div>

      <Button variant="default" size="sm" class="w-full" onclick={handleInit} disabled={initBusy}>
        Initialize Repository
      </Button>

      <div class="flex flex-col gap-1.5">
        <span class="text-2xs font-medium tracking-wider text-muted uppercase">Or clone</span>
        <Input
          type="text"
          placeholder="https://github.com/..."
          bind:value={cloneUrl}
          onkeydown={(e: KeyboardEvent) => {
            if (e.key === 'Enter') handleClone()
          }}
          disabled={initBusy}
        />
        <Button
          variant="default"
          size="sm"
          class="w-full"
          onclick={handleClone}
          disabled={!cloneUrl.trim() || initBusy}
        >
          Clone Repository
        </Button>
      </div>

      {#if initError}
        <p class="text-xs text-danger">{initError}</p>
      {/if}
    </div>
  {:else}
    <ResizablePaneGroup direction="vertical">
      <ResizablePane defaultSize={60} minSize={15}>
        <div class="h-full overflow-y-auto">
          <GitFileTree
            {summary}
            {projectId}
            {projectPath}
            {hasCommits}
            {onFileClick}
            {onPersistTab}
            {onViewAllChanges}
            bind:commitMessage
            onCommit={handleCommit}
            onStageAll={handleStageAll}
            onUnstageAll={handleUnstageAll}
            onDiscardAll={() => (showDiscardConfirm = true)}
            onStashAll={handleStashAll}
            onUndoLastCommit={() => (showUndoCommitConfirm = true)}
            onPush={handlePush}
            onPushForceWithLease={handlePushForceWithLease}
            onPull={handlePull}
            onFetch={handleFetch}
          />
        </div>
      </ResizablePane>
      <ResizableHandle />
      <ResizablePane defaultSize={40} minSize={15}>
        <div class="h-full overflow-y-auto">
          <GitGraph
            {projectPath}
            onFileClick={onCommitFileClick}
            {onStashFileClick}
            {onPersistTab}
            onMutate={refresh}
          />
        </div>
      </ResizablePane>
    </ResizablePaneGroup>
  {/if}
</SidebarPanel>

<ConfirmDialog
  open={showDiscardConfirm}
  title="Discard All Changes?"
  message="This will permanently discard all unstaged changes and remove untracked files. This cannot be undone."
  confirmLabel="Discard All"
  onConfirm={handleDiscardAll}
  onCancel={() => (showDiscardConfirm = false)}
/>

<ConfirmDialog
  open={showUndoCommitConfirm}
  title="Undo Last Commit?"
  message="This will soft-reset to HEAD~1. Your changes will be preserved as staged files."
  confirmLabel="Undo Commit"
  onConfirm={handleUndoLastCommit}
  onCancel={() => (showUndoCommitConfirm = false)}
/>
