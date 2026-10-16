<script lang="ts">
  import { Button, IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import * as Alert from '$lib/components/ui/alert'
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { sidebarRowVariants } from '$lib/components/ui/sidebar-row'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { migrateWorktreeChanges, pruneWorktrees, removeWorktree } from '$lib/features/git/actions.svelte'
  import CreateWorktreeDialog from '$lib/features/git/dialogs/CreateWorktreeDialog.svelte'
  import * as worktrees from '$lib/features/git/worktrees.svelte'
  import { openFolder } from '$lib/features/workbench/state.svelte'
  import {
    AlertTriangle,
    FolderOpenIcon,
    GitBranchIcon,
    GitCommitIcon,
    GitMergeIcon,
    LoaderCircle,
    Lock,
    MoreHorizontalIcon,
    Plus,
    RefreshCwIcon,
    Trash2
  } from '$lib/icons/lucideExports'
  import type { GitWorktree } from '$lib/types/backend'
  import { getErrorMessage } from '$lib/utils/client-error'

  let { folderPath }: { folderPath: string } = $props()

  let entry = $derived(worktrees.byFolder.get(folderPath))
  let createOpen = $state(false)
  let busy = $state(false)
  let actionError = $state<{ path: string; message: string } | null>(null)
  let error = $derived(actionError?.path === folderPath ? actionError.message : (entry?.error ?? null))

  $effect(() => {
    void worktrees.loadFor(folderPath)
  })

  function canOpen(worktree: GitWorktree): boolean {
    return !worktree.bare && worktree.prunable === null
  }

  async function runAction(action: (path: string) => Promise<void>) {
    if (busy) return
    const path = folderPath
    busy = true
    actionError = null
    try {
      await action(path)
    } catch (e) {
      actionError = { path, message: getErrorMessage(e) }
    } finally {
      busy = false
    }
  }

  function refresh() {
    actionError = null
    void worktrees.refreshWorktrees(folderPath)
  }
</script>

<div data-slot="git-worktrees" class="flex h-full min-h-0 flex-col overflow-hidden text-base">
  <div class="flex shrink-0 items-center gap-1.5 border-b border-edge px-2 py-1.5">
    <Button size="sm" variant="ghost" disabled={busy} onclick={() => (createOpen = true)}>
      <Plus size={14} />
      Create
    </Button>
    <div class="flex-1"></div>
    <IconButton tooltip="Refresh Worktrees" disabled={busy || entry?.loading} onclick={refresh}>
      <RefreshCwIcon size={13} class={entry?.loading ? 'animate-spin' : ''} />
    </IconButton>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class={iconButtonVariants()} aria-label="Worktree actions" disabled={busy}>
        <MoreHorizontalIcon size={13} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Item
          disabled={busy || entry?.loading || !entry?.inventory}
          onSelect={() => void runAction(pruneWorktrees)}
        >
          <Trash2 size={14} />
          <span>Prune Stale Worktrees</span>
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>

  {#if error}
    <Alert.Root variant="danger" layout="banner">
      <AlertTriangle size={13} class="shrink-0" />
      <span class="min-w-0 flex-1 break-words">{error}</span>
      <Button size="xs" variant="ghost" disabled={busy || entry?.loading} onclick={refresh}>Retry</Button>
    </Alert.Root>
  {/if}

  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if !entry || (entry.loading && !entry.inventory)}
      <div class="flex items-center gap-1.5 px-2.5 py-2 text-sm text-subtle" role="status">
        <LoaderCircle size={12} class="animate-spin" />
        Loading worktrees…
      </div>
    {:else if entry.inventory?.worktrees.length === 0}
      <div class="px-2.5 py-2 text-sm text-subtle">No worktrees found.</div>
    {:else if entry.inventory}
      <Tooltip.Provider delayDuration={400} skipDelayDuration={100}>
        {#each entry.inventory.worktrees as worktree (worktree.path)}
          {@const openable = canOpen(worktree)}
          <ContextMenu.Root>
            <ContextMenu.Trigger class="contents">
              <Tooltip.Root>
                <Tooltip.Trigger
                  type="button"
                  data-slot="worktree-row"
                  class="{sidebarRowVariants({ variant: 'leaf', pressed: worktree.isCurrent })} h-auto min-h-8 py-1.5"
                  aria-disabled={!openable || busy}
                  aria-label={`Open ${worktree.branch ?? 'detached'} worktree: ${worktree.path}`}
                  onclick={() => {
                    if (openable && !busy) void runAction(() => openFolder(worktree.path))
                  }}
                >
                  {#if worktree.branch}
                    <GitBranchIcon
                      size={12}
                      class={worktree.isCurrent ? 'shrink-0 text-accent' : 'shrink-0 text-muted'}
                    />
                  {:else}
                    <GitCommitIcon size={12} class="shrink-0 text-muted" />
                  {/if}
                  <span class="min-w-0 flex-1 space-y-0.5">
                    <span class="flex min-w-0 items-center gap-1.5">
                      <span class="min-w-0 flex-1 truncate font-mono">
                        {worktree.branch ??
                          (worktree.bare ? 'Bare repository' : `Detached ${worktree.head?.slice(0, 8) ?? 'HEAD'}`)}
                      </span>
                      {#if worktree.isMain}<span class="text-2xs text-muted">Main</span>{/if}
                      {#if worktree.isCurrent}<span class="text-2xs text-accent">Current</span>{/if}
                      {#if worktree.locked !== null}<Lock
                          size={11}
                          class="shrink-0 text-warning"
                          aria-label="Locked"
                        />{/if}
                      {#if worktree.prunable !== null}<AlertTriangle
                          size={11}
                          class="shrink-0 text-warning"
                          aria-label="Prunable"
                        />{/if}
                    </span>
                    <span class="block truncate font-mono text-2xs text-subtle">{worktree.path}</span>
                  </span>
                </Tooltip.Trigger>
                <Tooltip.Content side="right" align="center" class="max-w-90 space-y-1">
                  <div class="font-mono text-xs break-all">{worktree.path}</div>
                  {#if worktree.head}<div class="font-mono text-2xs text-muted">{worktree.head}</div>{/if}
                  {#if worktree.bare}<div class="text-xs text-muted">
                      Bare repositories cannot be opened as a worktree.
                    </div>{/if}
                  {#if worktree.locked !== null}
                    <div class="text-xs text-warning">
                      Locked{worktree.locked ? `: ${worktree.locked}` : '. Removal is disabled.'}
                    </div>
                  {/if}
                  {#if worktree.prunable !== null}
                    <div class="text-xs text-warning">
                      Prunable{worktree.prunable ? `: ${worktree.prunable}` : '. This worktree is unavailable.'}
                    </div>
                    <div class="text-xs text-muted">Prune stale worktrees to remove this entry.</div>
                  {/if}
                </Tooltip.Content>
              </Tooltip.Root>
            </ContextMenu.Trigger>
            <ContextMenu.Content>
              <ContextMenu.Item
                disabled={busy || !openable}
                onSelect={() => void runAction(() => openFolder(worktree.path))}
              >
                <FolderOpenIcon size={14} />
                <span>Open Worktree</span>
              </ContextMenu.Item>
              <ContextMenu.Item
                disabled={busy || !openable || worktree.isMain}
                onSelect={() => void runAction((path) => migrateWorktreeChanges(path, worktree.path))}
              >
                <GitMergeIcon size={14} />
                <span>Migrate Changes to Main</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item
                destructive
                disabled={busy || !openable || worktree.isMain || worktree.locked !== null}
                onSelect={() => void runAction((path) => removeWorktree(path, worktree.path))}
              >
                <Trash2 size={14} />
                <span>Remove Worktree…</span>
              </ContextMenu.Item>
            </ContextMenu.Content>
          </ContextMenu.Root>
        {/each}
      </Tooltip.Provider>
    {/if}
  </div>
</div>

{#if createOpen}
  <CreateWorktreeDialog bind:open={createOpen} {folderPath} />
{/if}
