<!--
  @component
  DeleteBranchDialog: first attempt is a regular delete; if the
  backend returns an unmerged-branch error, the dialog flips to a
  danger state and offers force-delete. Remote branches route
  through the `delete_remote` operation.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import * as Dialog from '$lib/components/ui/dialog'
  import { isGitError } from '$lib/features/git/errors'
  import { runGitAction } from '$lib/features/git/state.svelte'
  import type { BranchSummary } from '$lib/types/backend'
  import { splitRemoteBranchRef } from '$lib/features/git/gitRefs'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    folderPath,
    branch
  }: {
    open?: boolean
    folderPath: string
    branch: BranchSummary
  } = $props()

  let unmergedWarning = $state(false)

  const submitState = runDialogSubmit({
    run: async () => {
      await runGitAction(folderPath, async (path) => {
        if (branch.kind === 'remote') {
          const remoteRef = splitRemoteBranchRef(branch.name)
          if (!remoteRef) {
            throw new Error(`Cannot infer remote for ${branch.name}`)
          }
          await backend.git.branch(path, { kind: 'delete_remote', remote: remoteRef.remote, name: remoteRef.branch })
        } else {
          await backend.git.branch(path, { kind: 'delete', name: branch.name, force: unmergedWarning })
        }
      })
    },
    onDone: () => (open = false),
    onError: (error) => {
      if (branch.kind === 'local' && !unmergedWarning && isGitError(error, 'branchUnmerged')) {
        unmergedWarning = true
        return true
      }
    }
  })
</script>

<Dialog.Root bind:open>
  {#if open}
    <Dialog.Content class={unmergedWarning ? 'border-danger' : ''}>
      <Dialog.Header>
        <Dialog.Title>
          {unmergedWarning ? 'Force delete?' : 'Delete branch?'}
        </Dialog.Title>
      </Dialog.Header>

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
            This branch has unmerged commits. Forcing delete loses any commits not yet merged elsewhere.
          </p>
        {/if}
        <DialogError message={submitState.error} />
      </div>

      <Dialog.Footer>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="destructive" disabled={submitState.busy} onclick={submitState.submit}>
          {submitState.busy ? 'Deleting…' : unmergedWarning ? 'Force delete' : 'Delete'}
        </Button>
      </Dialog.Footer>
    </Dialog.Content>
  {/if}
</Dialog.Root>
