<!--
  @component
  CheckoutDialog: confirms a switch when the working tree is dirty.

  Shown only when `safeCheckout` raised `DirtyCheckoutError`. Two
  options: stash & switch (auto-stash includes untracked files), or
  cancel. There is no force-switch option in v1.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import {
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogRoot,
    DialogTitle
  } from '$lib/components/ui/dialog'
  import * as branches from '$lib/features/git/branches.svelte'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'
  import { runGitAction } from '$lib/features/git/state.svelte'
  import type { GitSummary } from '$lib/types/backend'

  let {
    open = $bindable(false),
    branchName,
    remoteBranchName = null,
    summary,
    folderPath,
    onSwitched
  }: {
    open?: boolean
    branchName: string
    remoteBranchName?: string | null
    summary: GitSummary | null
    folderPath: string
    onSwitched?: () => void
  } = $props()

  let staged = $derived(summary?.staged_count ?? 0)
  let unstaged = $derived(summary?.unstaged_count ?? 0)
  let untracked = $derived(summary?.untracked_count ?? 0)

  const submitState = runDialogSubmit({
    run: async () => {
      if (!branchName) return
      await runGitAction(folderPath, async (path) => {
        await backend.git.stashAll(path, `auto-stash before switch to ${branchName}`)
        if (remoteBranchName) {
          await backend.git.branch.checkoutRemoteAsLocal(path, remoteBranchName, branchName)
        } else {
          await backend.git.branch.checkout(path, branchName)
        }
      })
      branches.markRecent(folderPath, branchName)
    },
    onDone: () => {
      open = false
      onSwitched?.()
    }
  })

  function cancel() {
    if (submitState.busy) return
    open = false
  }
</script>

<DialogRoot bind:open>
  {#if open}
    <DialogContent onModalClose={cancel}>
      <DialogHeader>
        <DialogTitle>Switch to {branchName}?</DialogTitle>
        <DialogDescription>
          This working tree has uncommitted changes. Stashing them before switching keeps them recoverable from the
          Stashes tab.
        </DialogDescription>
      </DialogHeader>

      <div class="mt-2 space-y-2 text-sm text-muted">
        <div class="flex flex-wrap gap-x-3 gap-y-1">
          {#if staged > 0}
            <span><span class="text-fg">{staged}</span> staged</span>
          {/if}
          {#if unstaged > 0}
            <span><span class="text-fg">{unstaged}</span> unstaged</span>
          {/if}
          {#if untracked > 0}
            <span><span class="text-fg">{untracked}</span> untracked</span>
          {/if}
        </div>
        <p class="text-xs text-subtle">
          Other sessions in this folder share the same working tree, so switching changes the branch for them too.
        </p>
        <DialogError message={submitState.error} />
      </div>

      <DialogFooter>
        <Button variant="ghost" disabled={submitState.busy} onclick={cancel}>Cancel</Button>
        <Button variant="accent" disabled={submitState.busy} onclick={submitState.submit}>
          {submitState.busy ? 'Stashing…' : 'Stash & switch'}
        </Button>
      </DialogFooter>
    </DialogContent>
  {/if}
</DialogRoot>
