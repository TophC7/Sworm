<!--
  @component
  SetUpstreamDialog: pick a remote-tracking branch from the existing
  list, or fall back to a free-text input.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import * as Dialog from '$lib/components/ui/dialog'
  import { Input, Select } from '$lib/components/ui/input'
  import * as branches from '$lib/features/git/branches.svelte'
  import { runGitAction } from '$lib/features/git/state.svelte'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    folderPath,
    branchName,
    initialUpstream = ''
  }: {
    open?: boolean
    folderPath: string
    branchName: string
    initialUpstream?: string
  } = $props()

  let entry = $derived(branches.byFolder.get(folderPath))
  let suggestions = $derived.by(() => {
    if (!entry) return [] as string[]
    return entry.list.filter((b) => b.kind === 'remote').map((b) => b.name)
  })

  function initialUpstreamValue(): string {
    return initialUpstream
  }

  let upstream = $state(initialUpstreamValue())

  const submitState = runDialogSubmit({
    run: async () => {
      await runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'set_upstream', branch: branchName, upstream }))
    },
    onDone: () => (open = false)
  })

  let canSubmit = $derived(upstream.length > 0 && !submitState.busy)
</script>

<Dialog.Root bind:open>
  {#if open}
    <Dialog.Content>
      <Dialog.Header>
        <Dialog.Title>Set Upstream</Dialog.Title>
      </Dialog.Header>

      <div class="mt-2 space-y-2 text-sm">
        <p class="text-muted">
          For <span class="font-mono text-fg">{branchName}</span>
        </p>

        {#if suggestions.length > 0}
          <Select bind:value={upstream} class="text-sm">
            {#each suggestions as option (option)}
              <option value={option}>{option}</option>
            {/each}
          </Select>
        {/if}

        <Input bind:value={upstream} placeholder="origin/main" class="text-sm" spellcheck={false} />

        <DialogError message={submitState.error} />
      </div>

      <Dialog.Footer>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="accent" disabled={!canSubmit} onclick={submitState.submit}>
          {submitState.busy ? 'Saving…' : 'Set upstream'}
        </Button>
      </Dialog.Footer>
    </Dialog.Content>
  {/if}
</Dialog.Root>
