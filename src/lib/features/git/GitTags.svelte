<script lang="ts">
  import { Button, iconButtonVariants } from '$lib/components/ui/button'
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { sidebarRowVariants } from '$lib/components/ui/sidebar-row'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { copyToClipboard, createTag, deleteRemoteTag, deleteTag, pushTag } from '$lib/features/git/actions.svelte'
  import { checkoutRef } from '$lib/features/git/branches.svelte'
  import CreateBranchDialog from '$lib/features/git/dialogs/CreateBranchDialog.svelte'
  import CreateWorktreeDialog from '$lib/features/git/dialogs/CreateWorktreeDialog.svelte'
  import { getGitSummary, getTags, loadTags } from '$lib/features/git/state.svelte'
  import { openCommitDiff, openRangeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import {
    CloudUploadIcon,
    CopyIcon,
    FolderOpenIcon,
    GitBranchIcon,
    GitCommitIcon,
    GitCompareIcon,
    MoreHorizontalIcon,
    Plus,
    TagIcon
  } from '$lib/icons/lucideExports'
  import { formatFullDate, timeAgo } from '$lib/utils/date'

  let { folderPath }: { folderPath: string } = $props()

  let tags = $derived(getTags(folderPath))
  let summary = $derived(getGitSummary(folderPath))
  let createBranchOpen = $state(false)
  let createBranchBase = $state('')
  let createWorktreeOpen = $state(false)
  let createWorktreeBase = $state('')

  $effect(() => {
    void loadTags(folderPath)
  })
</script>

<div data-slot="git-tags" class="flex h-full min-h-0 flex-col overflow-hidden text-base">
  <div class="flex shrink-0 items-center gap-1.5 border-b border-edge px-2 py-1.5">
    <Button
      size="sm"
      variant="ghost"
      disabled={summary?.unborn}
      onclick={() => void createTag(folderPath, 'HEAD', 'HEAD')}
    >
      <Plus size={14} />
      Create
    </Button>
    <div class="flex-1"></div>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class={iconButtonVariants()} aria-label="Tag Actions">
        <MoreHorizontalIcon size={13} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Item onSelect={() => void pushTag(folderPath, null)}>
          <CloudUploadIcon size={14} />
          <span>Push All Tags…</span>
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if tags === null}
      <div class="px-2.5 py-2 text-sm text-subtle" role="status">Loading tags…</div>
    {:else if tags.length === 0}
      <div class="px-2.5 py-2 text-sm text-subtle">No tags.</div>
    {:else}
      <Tooltip.Provider delayDuration={400} skipDelayDuration={100}>
        {#each tags as tag (tag.name)}
          <ContextMenu.Root>
            <ContextMenu.Trigger class="contents">
              <Tooltip.Root>
                <Tooltip.Trigger
                  type="button"
                  data-slot="tag-row"
                  class={sidebarRowVariants({ variant: 'leaf' })}
                  onclick={() => void openCommitDiff(folderPath, tag.hash, tag.shortHash, tag.subject)}
                >
                  <TagIcon size={12} class="shrink-0 text-muted" />
                  <span class="min-w-0 flex-1 truncate font-mono">{tag.name}</span>
                  <span class="shrink-0 font-mono text-2xs text-subtle">{tag.shortHash}</span>
                  <span class="shrink-0 font-mono text-2xs text-subtle">{timeAgo(tag.date)}</span>
                </Tooltip.Trigger>
                <Tooltip.Content side="right" align="center" class="max-w-80 space-y-1">
                  <p class="text-sm leading-snug text-fg">{tag.subject}</p>
                  <div class="text-xs text-muted">{tag.annotated ? 'Annotated' : 'Lightweight'}</div>
                  <div class="text-xs text-subtle">{formatFullDate(tag.date)}</div>
                </Tooltip.Content>
              </Tooltip.Root>
            </ContextMenu.Trigger>
            <ContextMenu.Content>
              <ContextMenu.Item onSelect={() => void openCommitDiff(folderPath, tag.hash, tag.shortHash, tag.subject)}>
                <GitCommitIcon size={14} />
                <span>Open Changes</span>
              </ContextMenu.Item>
              <ContextMenu.Item
                onSelect={() => void checkoutRef(folderPath, { kind: 'detached', rev: tag.name, label: tag.name })}
              >
                <GitCommitIcon size={14} />
                <span>Checkout (Detached)</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item
                onSelect={() => {
                  createBranchBase = tag.name
                  createBranchOpen = true
                }}
              >
                <GitBranchIcon size={14} />
                <span>Create Branch…</span>
              </ContextMenu.Item>
              <ContextMenu.Item
                onSelect={() => {
                  createWorktreeBase = tag.name
                  createWorktreeOpen = true
                }}
              >
                <FolderOpenIcon size={14} />
                <span>Create Worktree…</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item
                onSelect={() =>
                  void openRangeDiff(folderPath, { rev: tag.name, label: tag.name }, { rev: 'HEAD', label: 'HEAD' })}
              >
                <GitCompareIcon size={14} />
                <span>Compare with HEAD</span>
              </ContextMenu.Item>
              <ContextMenu.Item
                onSelect={() => void openRangeDiff(folderPath, { rev: tag.name, label: tag.name }, null)}
              >
                <GitCompareIcon size={14} />
                <span>Compare with Working Tree</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item onSelect={() => void pushTag(folderPath, tag.name)}>
                <CloudUploadIcon size={14} />
                <span>Push Tag…</span>
              </ContextMenu.Item>
              <ContextMenu.Item destructive onSelect={() => void deleteTag(folderPath, tag.name)}>
                <TagIcon size={14} />
                <span>Delete Tag…</span>
              </ContextMenu.Item>
              <ContextMenu.Item destructive onSelect={() => void deleteRemoteTag(folderPath, tag.name)}>
                <TagIcon size={14} />
                <span>Delete Tag from Remote…</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item onSelect={() => void copyToClipboard(tag.name, 'Copy tag name failed')}>
                <CopyIcon size={14} />
                <span>Copy Tag Name</span>
              </ContextMenu.Item>
              <ContextMenu.Item onSelect={() => void copyToClipboard(tag.hash, 'Copy commit hash failed')}>
                <CopyIcon size={14} />
                <span>Copy Commit Hash</span>
              </ContextMenu.Item>
            </ContextMenu.Content>
          </ContextMenu.Root>
        {/each}
      </Tooltip.Provider>
    {/if}
  </div>
</div>

{#if createBranchOpen}
  <CreateBranchDialog bind:open={createBranchOpen} {folderPath} defaultBase={createBranchBase} />
{/if}

{#if createWorktreeOpen}
  <CreateWorktreeDialog
    bind:open={createWorktreeOpen}
    {folderPath}
    initialCheckout={{ kind: 'new_branch', name: '', base: createWorktreeBase }}
  />
{/if}
