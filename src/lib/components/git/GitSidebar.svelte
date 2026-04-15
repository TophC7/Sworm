<script lang="ts">
  import GitGraph from '$lib/components/git/GitGraph.svelte'
  import GitFileTree from '$lib/components/git/GitFileTree.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { Button } from '$lib/components/ui/button'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { Sidebar, SidebarContent, SidebarHeader, SidebarProvider, SidebarTrigger } from '$lib/components/ui/sidebar'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { isGitSidebarCollapsed, setGitSidebarCollapsed } from '$lib/stores/ui.svelte'
  import { refreshGit, runGitAction } from '$lib/stores/git.svelte'
  import { backend } from '$lib/api/backend'
  import type { GitSummary } from '$lib/types/backend'
  import { PanelLeftClose } from '$lib/icons/lucideExports'
  import { GitBranchIcon, RotateCw } from '$lib/icons/lucideExports'

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

  let collapsed = $derived(isGitSidebarCollapsed())
  let hasCommits = $derived(!!summary?.branch)
  let isRepo = $derived(summary?.is_repo ?? true)

  // Confirmation dialog state
  let showDiscardConfirm = $state(false)
  let showUndoCommitConfirm = $state(false)

  // Init/clone state
  let cloneUrl = $state('')
  let initBusy = $state(false)
  let initError = $state<string | null>(null)

  async function handleInit() {
    initBusy = true
    initError = null
    try {
      await backend.git.init(projectPath)
      await refreshGit(projectId, projectPath)
    } catch (e) {
      initError = e instanceof Error ? e.message : String(e)
    } finally {
      initBusy = false
    }
  }

  async function handleClone() {
    if (!cloneUrl.trim()) return
    initBusy = true
    initError = null
    try {
      await backend.git.cloneInPlace(projectPath, cloneUrl.trim())
      cloneUrl = ''
      await refreshGit(projectId, projectPath)
    } catch (e) {
      initError = e instanceof Error ? e.message : String(e)
    } finally {
      initBusy = false
    }
  }

  async function refresh() {
    await refreshGit(projectId, projectPath)
  }

  async function handleGitAction(fn: (path: string) => Promise<unknown>, label: string) {
    try {
      await runGitAction(projectId, projectPath, fn)
    } catch (e) {
      console.error(`${label} failed:`, e)
    }
  }

  async function handleCommit(message: string) {
    await handleGitAction((path) => backend.git.commit(path, message), 'Commit')
  }

  async function handleStageAll() {
    await handleGitAction((path) => backend.git.stageAll(path), 'Stage all')
  }

  async function handleUnstageAll() {
    await handleGitAction((path) => backend.git.unstageAll(path), 'Unstage all')
  }

  async function handleDiscardAll() {
    showDiscardConfirm = false
    await handleGitAction((path) => backend.git.discardAll(path), 'Discard all')
  }

  async function handleStashAll() {
    await handleGitAction((path) => backend.git.stashAll(path), 'Stash all')
  }

  async function handleUndoLastCommit() {
    showUndoCommitConfirm = false
    await handleGitAction((path) => backend.git.undoLastCommit(path), 'Undo commit')
  }

  async function handlePush() {
    await handleGitAction((path) => backend.git.push(path), 'Push')
  }

  async function handlePushForceWithLease() {
    await handleGitAction((path) => backend.git.pushForceWithLease(path), 'Force push')
  }

  async function handlePull() {
    await handleGitAction((path) => backend.git.pull(path), 'Pull')
  }

  async function handleFetch() {
    await handleGitAction((path) => backend.git.fetch(path), 'Fetch')
  }
</script>

<SidebarProvider open={!collapsed} onOpenChange={(open) => setGitSidebarCollapsed(!open)}>
  <Sidebar>
    <SidebarHeader>
      <div class="flex items-center gap-1.5">
        <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Git</span>
        {#if onRefresh}
          <Button variant="ghost" size="icon-sm" onclick={onRefresh} title="Refresh">
            <RotateCw size={11} />
          </Button>
        {/if}
      </div>
      <div class="flex items-center gap-1">
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
        <SidebarTrigger>
          <PanelLeftClose size={12} />
        </SidebarTrigger>
      </div>
    </SidebarHeader>

    <SidebarContent>
      {#if !summary}
        <div class="px-2.5 py-3 text-[0.75rem] text-subtle">Loading git info&hellip;</div>
      {:else if !isRepo}
        <!-- Not a git repository — offer init or clone -->
        <div class="flex flex-col gap-4 px-3 py-4">
          <div class="flex flex-col items-center gap-2 py-4 text-center">
            <GitBranchIcon size={28} class="text-subtle" />
            <p class="text-[0.75rem] text-muted">This folder is not a git repository.</p>
          </div>

          <button
            class="w-full rounded border border-edge bg-raised px-3 py-1.5 text-[0.72rem] font-medium text-fg transition-colors hover:border-accent hover:text-bright disabled:cursor-not-allowed disabled:opacity-40"
            onclick={handleInit}
            disabled={initBusy}
          >
            Initialize Repository
          </button>

          <div class="flex flex-col gap-1.5">
            <span class="text-[0.65rem] font-medium tracking-wider text-muted uppercase">Or clone</span>
            <input
              type="text"
              class="w-full rounded border border-edge bg-surface px-2 py-1.5 text-[0.72rem] text-fg placeholder:text-subtle focus:border-accent/50 focus:outline-none"
              placeholder="https://github.com/..."
              bind:value={cloneUrl}
              onkeydown={(e) => {
                if (e.key === 'Enter') handleClone()
              }}
              disabled={initBusy}
            />
            <button
              class="w-full rounded border border-edge bg-raised px-3 py-1.5 text-[0.72rem] font-medium text-fg transition-colors hover:border-accent hover:text-bright disabled:cursor-not-allowed disabled:opacity-40"
              onclick={handleClone}
              disabled={!cloneUrl.trim() || initBusy}
            >
              Clone Repository
            </button>
          </div>

          {#if initError}
            <p class="text-[0.7rem] text-danger">{initError}</p>
          {/if}
        </div>
      {:else}
        <ResizablePaneGroup direction="vertical">
          <ResizablePane defaultSize={60} minSize={15}>
            <div class="h-full overflow-y-auto">
              <GitFileTree
                {summary}
                {projectPath}
                {hasCommits}
                {onFileClick}
                {onPersistTab}
                {onViewAllChanges}
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
    </SidebarContent>
  </Sidebar>
</SidebarProvider>

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
