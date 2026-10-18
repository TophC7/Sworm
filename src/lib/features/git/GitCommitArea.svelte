<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { ButtonGroup } from '$lib/components/ui/button-group'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { Textarea } from '$lib/components/ui/input'
  import {
    ArrowUp,
    Check,
    ChevronDown,
    CloudUploadIcon,
    GitCommitIcon,
    PlusCircle,
    RefreshCwIcon,
    Undo2Icon
  } from '$lib/icons/lucideExports'
  import type { GitSummary } from '$lib/types/backend'
  import AheadBehindBadge from './AheadBehindBadge.svelte'
  import {
    abortOperation,
    commitFolder,
    continueOperation,
    isOperationPending,
    publishBranch,
    skipOperation,
    syncFolder,
    undoLastCommit,
    type CommitVariant
  } from './actions.svelte'
  import {
    getCommitState,
    loadCommitPrefs,
    setCommitDraft,
    setPostCommit,
    setSignOff,
    setSkipHooks
  } from './commit.svelte'
  import { resolvePrimaryAction, type PostCommit } from './commitButton'
  import { describeOperation } from './operation'

  let { summary, folderPath }: { summary: GitSummary; folderPath: string } = $props()

  const commit = $derived(getCommitState(folderPath))
  const action = $derived(resolvePrimaryAction(summary, commit.draft, commit.postCommit))
  const display = $derived(summary.operation ? describeOperation(summary.operation) : null)
  const isRebase = $derived(summary.operation?.kind === 'rebase')
  const messageDescription = $derived(isRebase ? 'Rebase keeps the original commit message and author.' : undefined)
  const canCommit = $derived(summary.operation === null && summary.changes.length > 0 && commit.draft.trim().length > 0)
  const canPostCommit = $derived(canCommit && summary.branch !== null && !summary.detached)
  const hasHead = $derived(summary.is_repo && !summary.unborn)
  let localBusy = $state(false)
  const busy = $derived(localBusy || isOperationPending(folderPath))

  $effect(() => {
    void loadCommitPrefs(folderPath)
  })

  async function run(task: () => unknown | Promise<unknown>) {
    if (busy) return
    localBusy = true
    try {
      await task()
    } finally {
      localBusy = false
    }
  }

  function runCommit(variant: CommitVariant) {
    if (summary.operation !== null) return
    const path = folderPath
    return run(() => commitFolder(path, variant))
  }

  function runPrimary() {
    const current = action
    const path = folderPath
    switch (current.kind) {
      case 'continue':
        if (current.enabled) return run(() => continueOperation(path))
        return
      case 'commit':
        if (current.enabled) return runCommit({ kind: 'commit', post: current.post })
        return
      case 'publish':
        return run(() => publishBranch(path, current.branch))
      case 'sync':
        return run(() => syncFolder(path))
    }
  }

  function chooseCommit(post: PostCommit) {
    if (busy || summary.operation !== null) return
    setPostCommit(folderPath, post)
    return runCommit({ kind: 'commit', post })
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault()
      if ((action.kind === 'continue' || action.kind === 'commit') && action.enabled) void runPrimary()
    }
  }

  function runUndo() {
    if (summary.operation !== null) return
    const path = folderPath
    return run(() => undoLastCommit(path))
  }
</script>

<div data-slot="git-commit-area" class="border-b border-edge px-2.5 py-2">
  <Textarea
    rows={2}
    aria-label="Commit message"
    placeholder="Commit message..."
    readonly={isRebase}
    title={messageDescription}
    bind:value={
      () => (isRebase ? (summary.message_template ?? summary.operation?.commit?.subject ?? '') : commit.draft),
      (value) => {
        if (!isRebase) setCommitDraft(folderPath, value)
      }
    }
    onkeydown={handleKeydown}
  />
  <div class="mt-1.5">
    <ButtonGroup class="w-full">
      <Button
        variant="default"
        size="sm"
        class="flex-1 rounded"
        disabled={busy || ((action.kind === 'commit' || action.kind === 'continue') && !action.enabled)}
        onclick={runPrimary}
      >
        {#if action.kind === 'continue'}
          <Check size={13} />
          {action.title}
        {:else if action.kind === 'commit'}
          {#if action.post === 'push'}
            <ArrowUp size={13} />
            Commit &amp; Push
          {:else if action.post === 'sync'}
            <RefreshCwIcon size={13} />
            Commit &amp; Sync
          {:else}
            <Check size={13} />
            Commit
          {/if}
        {:else if action.kind === 'publish'}
          <CloudUploadIcon size={13} />
          Publish Branch
        {:else}
          <RefreshCwIcon size={13} />
          Sync Changes
          <AheadBehindBadge ahead={action.ahead} behind={action.behind} />
        {/if}
      </Button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger
          data-slot="button"
          aria-label="Commit Options"
          class="flex items-center rounded border border-edge bg-raised px-1 py-1 text-muted transition-colors hover:border-accent hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none"
        >
          <ChevronDown size={11} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Content>
          {#if display}
            <DropdownMenu.Item disabled={busy || action.kind !== 'continue' || !action.enabled} onSelect={runPrimary}>
              <Check size={14} />
              Continue {display.title}
            </DropdownMenu.Item>
            {#if display.canSkip}
              <DropdownMenu.Item disabled={busy} onSelect={() => run(() => skipOperation(folderPath))}>
                Skip Commit
              </DropdownMenu.Item>
            {/if}
            <DropdownMenu.Separator />
            <DropdownMenu.Item destructive disabled={busy} onSelect={() => run(() => abortOperation(folderPath))}>
              Abort {display.title}
            </DropdownMenu.Item>
            <DropdownMenu.Separator />
          {/if}
          <DropdownMenu.Item disabled={!canCommit || busy} onSelect={() => chooseCommit('none')}>
            <Check size={14} />
            Commit
          </DropdownMenu.Item>
          <DropdownMenu.Item disabled={!canPostCommit || busy} onSelect={() => chooseCommit('push')}>
            <ArrowUp size={14} />
            Commit &amp; Push
          </DropdownMenu.Item>
          <DropdownMenu.Item disabled={!canPostCommit || busy} onSelect={() => chooseCommit('sync')}>
            <RefreshCwIcon size={14} />
            Commit &amp; Sync
          </DropdownMenu.Item>
          <DropdownMenu.Separator />
          <DropdownMenu.Item
            disabled={!hasHead || summary.operation !== null || busy}
            onSelect={() => runCommit({ kind: 'amend' })}
          >
            <GitCommitIcon size={14} />
            Commit (Amend)
          </DropdownMenu.Item>
          <DropdownMenu.Item disabled={!canCommit || busy} onSelect={() => runCommit({ kind: 'all' })}>
            <PlusCircle size={14} />
            Commit All
          </DropdownMenu.Item>
          <DropdownMenu.Separator />
          <DropdownMenu.CheckboxItem
            closeOnSelect={false}
            bind:checked={() => commit.signOff, (value) => setSignOff(folderPath, value)}
          >
            Sign Off
          </DropdownMenu.CheckboxItem>
          <DropdownMenu.CheckboxItem
            closeOnSelect={false}
            bind:checked={() => commit.skipHooks, (value) => setSkipHooks(folderPath, value)}
          >
            Skip Hooks
          </DropdownMenu.CheckboxItem>
          <DropdownMenu.Separator />
          <DropdownMenu.Item destructive disabled={!hasHead || summary.operation !== null || busy} onSelect={runUndo}>
            <Undo2Icon size={14} />
            Undo Last Commit
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </ButtonGroup>
  </div>
</div>
