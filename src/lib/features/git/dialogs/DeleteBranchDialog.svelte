<!--
  @component
  DeleteBranchDialog: first attempt is a regular delete; if the
  backend returns an unmerged-branch error, the dialog flips to a
  danger state and offers force-delete. Remote branches route
  through `deleteRemote`.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import {
    DialogContent,
    DialogFooter,
    DialogHeader,
    DialogRoot,
    DialogTitle
  } from '$lib/components/ui/dialog'
  import * as branches from '$lib/features/git/branches.svelte'
  import type { BranchSummary } from '$lib/types/backend'
  import { splitRemoteBranchRef } from '$lib/features/git/gitRefs'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    projectId,
    projectPath,
    branch
  }: {
    open?: boolean
    projectId: string
    projectPath: string
    branch: BranchSummary
  } = $props()

  interface BranchUnmergedError {
    kind: 'branchUnmerged'
    branch: string
    message: string
  }

  function isBranchUnmergedError(error: unknown): error is BranchUnmergedError {
    return (
      typeof error === 'object' &&
      error !== null &&
      'kind' in error &&
      (error as { kind?: unknown }).kind === 'branchUnmerged'
    )
  }

  let unmergedWarning = $state(false)

  const submitState = runDialogSubmit({
    run: async () => {
      if (branch.kind === 'remote') {
        const remoteRef = splitRemoteBranchRef(branch.name)
        if (!remoteRef) {
          throw new Error(`Cannot infer remote for ${branch.name}`)
        }
        await backend.git.branch.deleteRemote(projectPath, remoteRef.remote, remoteRef.branch)
      } else {
        await backend.git.branch.delete(projectPath, branch.name, { force: unmergedWarning })
      }
      await branches.refresh(projectId, projectPath)
    },
    onDone: () => (open = false),
    onError: (error) => {
      if (branch.kind === 'local' && !unmergedWarning && isBranchUnmergedError(error)) {
        unmergedWarning = true
        return true
      }
    }
  })

</script>

<DialogRoot bind:open>
  {#if open}
    <DialogContent class={unmergedWarning ? 'border-danger' : ''}>
      <DialogHeader>
        <DialogTitle>
          {unmergedWarning ? 'Force delete?' : 'Delete branch?'}
        </DialogTitle>
      </DialogHeader>

      <div class="mt-2 space-y-2 text-sm">
        <p>
          <span class="text-muted">Branch:</span>
          <span class="ml-1 font-mono text-fg">{branch.name}</span>
        </p>
        <p class="text-xs text-muted">
          {#if branch.kind === 'remote'}
            This deletes the branch on its remote.
          {:else}
            {branch.ahead} ahead, {branch.behind} behind upstream.
          {/if}
        </p>
        {#if unmergedWarning}
          <p class="text-xs text-danger">
            This branch has unmerged commits. Forcing delete loses any commits not yet merged
            elsewhere.
          </p>
        {/if}
        <DialogError message={submitState.error} />
      </div>

      <DialogFooter>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="destructive" disabled={submitState.busy} onclick={submitState.submit}>
          {submitState.busy
            ? 'Deleting…'
            : unmergedWarning
              ? 'Force delete'
              : 'Delete'}
        </Button>
      </DialogFooter>
    </DialogContent>
  {/if}
</DialogRoot>
