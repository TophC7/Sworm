<!--
  @component
  MergeConfirmDialog: confirm `git merge <source>` into the current
  branch. Conflicts route to Source Control; this dialog never embeds
  conflict resolution.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { Checkbox } from '$lib/components/ui/checkbox'
  import * as Dialog from '$lib/components/ui/dialog'
  import { showMergeConflicts } from '$lib/features/git/actions.svelte'
  import { isGitError } from '$lib/features/git/errors'
  import { runGitAction } from '$lib/features/git/state.svelte'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    folderPath,
    source,
    current
  }: {
    open?: boolean
    folderPath: string
    source: string
    current: string
  } = $props()

  let noFf = $state(false)

  const submitState = runDialogSubmit({
    run: async () => {
      try {
        await runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'merge', source, noFf }))
      } catch (e) {
        if (isGitError(e, 'mergeConflicts')) {
          showMergeConflicts(e.message)
          return
        }
        throw e
      }
    },
    onDone: () => (open = false)
  })
</script>

<Dialog.Root bind:open>
  {#if open}
    <Dialog.Content>
      <Dialog.Header>
        <Dialog.Title>Merge {source} into {current}?</Dialog.Title>
      </Dialog.Header>

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

      <Dialog.Footer>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="accent" disabled={submitState.busy} onclick={submitState.submit}>
          {submitState.busy ? 'Merging…' : 'Merge'}
        </Button>
      </Dialog.Footer>
    </Dialog.Content>
  {/if}
</Dialog.Root>
