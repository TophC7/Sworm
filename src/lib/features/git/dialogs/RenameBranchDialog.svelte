<!--
  @component
  RenameBranchDialog: pre-filled name input, same validation as
  CreateBranchDialog.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import * as Dialog from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import { runGitAction } from '$lib/features/git/state.svelte'
  import { validateBranchName } from '$lib/utils/git/branchValidation'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    folderPath,
    oldName
  }: {
    open?: boolean
    folderPath: string
    oldName: string
  } = $props()

  function initialName(): string {
    return oldName
  }

  let name = $state(initialName())

  const submitState = runDialogSubmit({
    run: async () => {
      await runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'rename', oldName, newName: name }))
    },
    onDone: () => (open = false)
  })

  let nameError = $derived(name.length === 0 ? null : validateBranchName(name))
  let canSubmit = $derived(name.length > 0 && !nameError && name !== oldName && !submitState.busy)
</script>

<Dialog.Root bind:open>
  {#if open}
    <Dialog.Content>
      <Dialog.Header>
        <Dialog.Title>Rename Branch</Dialog.Title>
      </Dialog.Header>

      <div class="mt-2 space-y-2">
        <p class="text-xs text-muted">From <span class="font-mono text-fg">{oldName}</span></p>
        <Input bind:value={name} class="text-sm" spellcheck={false} autofocus />
        <DialogError message={nameError} />
        <DialogError message={submitState.error} />
      </div>

      <Dialog.Footer>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="accent" disabled={!canSubmit} onclick={submitState.submit}>
          {submitState.busy ? 'Renaming…' : 'Rename'}
        </Button>
      </Dialog.Footer>
    </Dialog.Content>
  {/if}
</Dialog.Root>
