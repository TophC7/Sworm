<script lang="ts">
  import GitGraph from '$lib/components/GitGraph.svelte'
  import GitFileTree from '$lib/components/GitFileTree.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { Button } from '$lib/components/ui/button'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { Sidebar, SidebarContent, SidebarHeader, SidebarProvider, SidebarTrigger } from '$lib/components/ui/sidebar'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { isGitSidebarCollapsed, setGitSidebarCollapsed } from '$lib/stores/ui.svelte'
  import { refreshGit } from '$lib/stores/git.svelte'
  import { backend } from '$lib/api/backend'
  import type { GitSummary } from '$lib/types/backend'
  import { PanelLeftClose } from '@lucide/svelte'
  import RotateCw from '@lucide/svelte/icons/rotate-cw'

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

  // Confirmation dialog state
  let showDiscardConfirm = $state(false)
  let showUndoCommitConfirm = $state(false)

  async function refresh() {
    await refreshGit(projectId, projectPath)
  }

  async function gitAction(fn: () => Promise<unknown>, label: string) {
    try {
      await fn()
      await refresh()
    } catch (e) {
      console.error(`${label} failed:`, e)
    }
  }

  async function handleCommit(message: string) {
    await gitAction(() => backend.git.commit(projectPath, message), 'Commit')
  }

  async function handleStageAll() {
    await gitAction(() => backend.git.stageAll(projectPath), 'Stage all')
  }

  async function handleUnstageAll() {
    await gitAction(() => backend.git.unstageAll(projectPath), 'Unstage all')
  }

  async function handleDiscardAll() {
    showDiscardConfirm = false
    await gitAction(() => backend.git.discardAll(projectPath), 'Discard all')
  }

  async function handleStashAll() {
    await gitAction(() => backend.git.stashAll(projectPath), 'Stash all')
  }

  async function handleUndoLastCommit() {
    showUndoCommitConfirm = false
    await gitAction(() => backend.git.undoLastCommit(projectPath), 'Undo commit')
  }

  async function handlePush() {
    await gitAction(() => backend.git.push(projectPath), 'Push')
  }

  async function handlePushForceWithLease() {
    await gitAction(() => backend.git.pushForceWithLease(projectPath), 'Force push')
  }

  async function handlePull() {
    await gitAction(() => backend.git.pull(projectPath), 'Pull')
  }

  async function handleFetch() {
    await gitAction(() => backend.git.fetch(projectPath), 'Fetch')
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
      {#if summary}
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
      {:else}
        <div class="px-2.5 py-3 text-[0.75rem] text-subtle">Loading git info&hellip;</div>
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
