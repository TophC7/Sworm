<!--
  @component
  MergeConfirmDialog: confirm `git merge <source>` into the current
  branch. Conflicts route to the existing GitFileTree pane via
  `onConflict`; this dialog never embeds conflict resolution.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { Checkbox } from '$lib/components/ui/checkbox'
  import {
    DialogContent,
    DialogFooter,
    DialogHeader,
    DialogRoot,
    DialogTitle
  } from '$lib/components/ui/dialog'
  import * as branches from '$lib/features/git/branches.svelte'
  import { refreshGit } from '$lib/features/git/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    projectId,
    projectPath,
    source,
    current,
    onConflict
  }: {
    open?: boolean
    projectId: string
    projectPath: string
    source: string
    current: string
    onConflict?: (message: string) => void
  } = $props()

  let noFf = $state(false)

  const submitState = runDialogSubmit({
    run: async () => {
      try {
        await backend.git.branch.merge(projectPath, source, { noFf })
        await Promise.all([branches.refresh(projectId, projectPath), refreshGit(projectId, projectPath)])
      } catch (e) {
        const msg = getErrorMessage(e)
        // Conflicts surface as a server error containing 'conflict'.
        // Close the dialog and let the parent show a non-modal notice
        // routing the user to GitFileTree.
        if (msg.toLowerCase().includes('conflict')) {
          onConflict?.(msg)
          return
        }
        throw e
      }
    },
    onDone: () => (open = false)
  })

</script>

<DialogRoot bind:open>
  {#if open}
    <DialogContent>
      <DialogHeader>
        <DialogTitle>Merge {source} into {current}?</DialogTitle>
      </DialogHeader>

      <div class="mt-2 space-y-2 text-sm">
        <p class="text-muted">
          Brings the commits from <span class="font-mono text-fg">{source}</span>
          into <span class="font-mono text-fg">{current}</span>.
        </p>
        <label class="flex items-center gap-2 text-sm text-fg">
          <Checkbox bind:checked={noFf} />
          <span>Always create a merge commit (no fast-forward)</span>
        </label>
        <DialogError message={submitState.error} />
      </div>

      <DialogFooter>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="accent" disabled={submitState.busy} onclick={submitState.submit}>
          {submitState.busy ? 'Merging…' : 'Merge'}
        </Button>
      </DialogFooter>
    </DialogContent>
  {/if}
</DialogRoot>
