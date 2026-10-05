<script lang="ts">
  import GitGraph from '$lib/features/git/GitGraph.svelte'
  import GitFileTree from '$lib/features/git/GitFileTree.svelte'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import { Button } from '$lib/components/ui/button'
  import { Alert } from '$lib/components/ui/alert'
  import { Input } from '$lib/components/ui/input'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { ensureGitWatch, getGitFreshness, getGitSummary } from '$lib/features/git/state.svelte'
  import { backend } from '$lib/api/backend'
  import { AlertTriangle, GitBranchIcon } from '$lib/icons/lucideExports'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'

  let { folderPath }: { folderPath: string } = $props()
  let summary = $derived(getGitSummary(folderPath))
  let freshness = $derived(getGitFreshness(folderPath))
  let readError = $derived(freshness.readError)
  let watchError = $derived(freshness.watchError)
  let isRepo = $derived(summary?.is_repo ?? true)
  let showWatchError = $derived(!!watchError && (summary?.is_repo ?? !!readError))


  // Init/clone state
  let cloneUrl = $state('')
  let initBusy = $state(false)
  let initError = $state<string | null>(null)

  async function handleInit() {
    initBusy = true
    initError = null
    await runNotifiedTask(
      async () => {
        await backend.git.init(folderPath)
        await ensureGitWatch(folderPath, 'all')
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
        await backend.git.cloneInPlace(folderPath, targetUrl)
        await ensureGitWatch(folderPath, 'all')
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

</script>

<SidebarPanel title="Git">
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

  <div class="flex h-full min-h-0 flex-col">
    {#if readError}
      <Alert variant="danger" layout="banner">
        <AlertTriangle size={13} class="mt-0.5 shrink-0" />
        <div class="min-w-0">
          <p class="font-medium">{summary ? 'Git status is stale.' : 'Git status unavailable.'}</p>
          <p class="mt-0.5 text-2xs break-words text-muted">{readError}</p>
        </div>
      </Alert>
    {/if}
    {#if showWatchError}
      <Alert variant="warning" layout="banner">
        <AlertTriangle size={13} class="mt-0.5 shrink-0" />
        <div class="min-w-0">
          <p class="font-medium">Git change watcher is degraded. Changes may be delayed.</p>
          <p class="mt-0.5 text-2xs break-words text-muted">{watchError}</p>
        </div>
      </Alert>
    {/if}
    <div class="min-h-0 flex-1">
      {#if !summary}
        <div class="px-2.5 py-3 text-sm text-subtle">
          {readError ? 'No git status available.' : 'Loading git info…'}
        </div>
      {:else if !isRepo}
        <!-- Not a git repository. Offer init or clone. -->
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
              <GitFileTree {summary} {folderPath} />
            </div>
          </ResizablePane>
          <ResizableHandle />
          <ResizablePane defaultSize={40} minSize={15}>
            <div class="h-full overflow-y-auto">
              <GitGraph {folderPath} />
            </div>
          </ResizablePane>
        </ResizablePaneGroup>
      {/if}
    </div>
  </div>
</SidebarPanel>

