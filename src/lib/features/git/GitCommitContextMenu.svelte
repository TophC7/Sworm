<script lang="ts">
  import type { Snippet } from 'svelte'
  import { backend } from '$lib/api/backend'
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import {
    cherryPickCommit,
    copyToClipboard,
    createTag,
    deleteTag,
    resetToCommit,
    revertCommit
  } from '$lib/features/git/actions.svelte'
  import { checkoutRef } from '$lib/features/git/branches.svelte'
  import CreateBranchDialog from '$lib/features/git/dialogs/CreateBranchDialog.svelte'
  import CreateWorktreeDialog from '$lib/features/git/dialogs/CreateWorktreeDialog.svelte'
  import { localNameForRemoteRef } from '$lib/features/git/gitRefs'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import { openCommitDiff, openRangeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import {
    CherryIcon,
    CloudIcon,
    CopyIcon,
    FolderOpenIcon,
    GitBranchIcon,
    GitCommitIcon,
    GitCompareIcon,
    RotateCcwIcon,
    TagIcon,
    Trash2,
    Undo2Icon
  } from '$lib/icons/lucideExports'
  import type { GitRef, GraphCommit } from '$lib/types/backend'

  let { folderPath, commit, children }: { folderPath: string; commit: GraphCommit; children: Snippet } = $props()

  let paused = $derived(getGitSummary(folderPath)?.operation != null)
  let checkoutRefs = $derived(commit.refs.filter((ref) => ref.kind !== 'branch' || !ref.current))
  let tagRefs = $derived(commit.refs.filter((ref) => ref.kind === 'tag'))
  let createBranchOpen = $state(false)
  let createWorktreeOpen = $state(false)

  function checkout(ref: GitRef) {
    void checkoutRef(
      folderPath,
      ref.kind === 'branch'
        ? { kind: 'branch', name: ref.name }
        : ref.kind === 'remote_branch'
          ? { kind: 'remote', remoteName: ref.name, localName: localNameForRemoteRef(ref.name) }
          : { kind: 'detached', rev: ref.name, label: ref.name }
    )
  }

  async function copyCommitMessage() {
    let text = commit.message
    try {
      const detail = await backend.git.getCommitDetail(folderPath, commit.hash)
      if (detail) {
        const body = detail.body.trim()
        text = body ? `${detail.message}\n\n${body}` : detail.message
      }
    } catch (e) {
      console.error('Commit detail failed; copying subject only:', e)
    }
    await copyToClipboard(text, 'Copy commit message failed')
  }
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger class="contents">
    {@render children()}
  </ContextMenu.Trigger>
  <ContextMenu.Content>
    <ContextMenu.Item onSelect={() => void openCommitDiff(folderPath, commit.hash, commit.short_hash, commit.message)}>
      <GitCommitIcon size={14} />
      <span>Open Changes</span>
    </ContextMenu.Item>
    <ContextMenu.Separator />
    {#if checkoutRefs.length > 0}
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger disabled={paused}>
          <GitBranchIcon size={14} />
          <span>Checkout</span>
        </ContextMenu.SubTrigger>
        <ContextMenu.SubContent>
          {#each checkoutRefs as ref (`${ref.kind}:${ref.name}`)}
            <ContextMenu.Item disabled={paused} onSelect={() => checkout(ref)}>
              {#if ref.kind === 'branch'}
                <GitBranchIcon size={14} />
              {:else if ref.kind === 'remote_branch'}
                <CloudIcon size={14} />
              {:else}
                <TagIcon size={14} />
              {/if}
              <span>{ref.name}</span>
            </ContextMenu.Item>
          {/each}
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
    {/if}
    <ContextMenu.Item
      disabled={paused}
      onSelect={() => void checkoutRef(folderPath, { kind: 'detached', rev: commit.hash, label: commit.short_hash })}
    >
      <GitCommitIcon size={14} />
      <span>Checkout (Detached)</span>
    </ContextMenu.Item>
    <ContextMenu.Separator />
    <ContextMenu.Item onSelect={() => (createBranchOpen = true)}>
      <GitBranchIcon size={14} />
      <span>Create Branch…</span>
    </ContextMenu.Item>
    <ContextMenu.Item onSelect={() => void createTag(folderPath, commit.hash, commit.short_hash)}>
      <TagIcon size={14} />
      <span>Create Tag…</span>
    </ContextMenu.Item>
    <ContextMenu.Item onSelect={() => (createWorktreeOpen = true)}>
      <FolderOpenIcon size={14} />
      <span>Create Worktree…</span>
    </ContextMenu.Item>
    <ContextMenu.Separator />
    <ContextMenu.Item disabled={paused} onSelect={() => void cherryPickCommit(folderPath, commit.hash)}>
      <CherryIcon size={14} />
      <span>Cherry Pick</span>
    </ContextMenu.Item>
    <ContextMenu.Item disabled={paused} onSelect={() => void revertCommit(folderPath, commit.hash)}>
      <Undo2Icon size={14} />
      <span>Revert</span>
    </ContextMenu.Item>
    <ContextMenu.Sub>
      <ContextMenu.SubTrigger disabled={paused}>
        <RotateCcwIcon size={14} />
        <span>Reset HEAD to Here</span>
      </ContextMenu.SubTrigger>
      <ContextMenu.SubContent>
        <ContextMenu.Item
          disabled={paused}
          onSelect={() => void resetToCommit(folderPath, commit.hash, commit.short_hash, 'soft')}
        >
          Soft: Keep Changes Staged
        </ContextMenu.Item>
        <ContextMenu.Item
          disabled={paused}
          onSelect={() => void resetToCommit(folderPath, commit.hash, commit.short_hash, 'mixed')}
        >
          Mixed: Keep Changes Unstaged
        </ContextMenu.Item>
        <ContextMenu.Item
          destructive
          disabled={paused}
          onSelect={() => void resetToCommit(folderPath, commit.hash, commit.short_hash, 'hard')}
        >
          Hard: Discard Changes
        </ContextMenu.Item>
      </ContextMenu.SubContent>
    </ContextMenu.Sub>
    <ContextMenu.Separator />
    <ContextMenu.Item
      onSelect={() =>
        void openRangeDiff(folderPath, { rev: commit.hash, label: commit.short_hash }, { rev: 'HEAD', label: 'HEAD' })}
    >
      <GitCompareIcon size={14} />
      <span>Compare with HEAD</span>
    </ContextMenu.Item>
    <ContextMenu.Item
      onSelect={() => void openRangeDiff(folderPath, { rev: commit.hash, label: commit.short_hash }, null)}
    >
      <GitCompareIcon size={14} />
      <span>Compare with Working Tree</span>
    </ContextMenu.Item>
    {#if tagRefs.length > 0}
      <ContextMenu.Sub>
        <ContextMenu.SubTrigger>
          <Trash2 size={14} />
          <span>Delete Tag</span>
        </ContextMenu.SubTrigger>
        <ContextMenu.SubContent>
          {#each tagRefs as ref (ref.name)}
            <ContextMenu.Item destructive onSelect={() => void deleteTag(folderPath, ref.name)}>
              <TagIcon size={14} />
              <span>{ref.name}</span>
            </ContextMenu.Item>
          {/each}
        </ContextMenu.SubContent>
      </ContextMenu.Sub>
    {/if}
    <ContextMenu.Separator />
    <ContextMenu.Item onSelect={() => void copyToClipboard(commit.hash, 'Copy commit hash failed')}>
      <CopyIcon size={14} />
      <span>Copy Commit Hash</span>
    </ContextMenu.Item>
    <ContextMenu.Item onSelect={() => void copyCommitMessage()}>
      <CopyIcon size={14} />
      <span>Copy Commit Message</span>
    </ContextMenu.Item>
  </ContextMenu.Content>
</ContextMenu.Root>

{#if createBranchOpen}
  <CreateBranchDialog bind:open={createBranchOpen} {folderPath} defaultBase={commit.short_hash} />
{/if}
{#if createWorktreeOpen}
  <CreateWorktreeDialog
    bind:open={createWorktreeOpen}
    {folderPath}
    initialCheckout={{ kind: 'new_branch', name: '', base: commit.hash }}
  />
{/if}
