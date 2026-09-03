<!--
  @component
  SetUpstreamDialog: pick a remote-tracking branch from the existing
  list, or fall back to a free-text input.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { DialogContent, DialogFooter, DialogHeader, DialogRoot, DialogTitle } from '$lib/components/ui/dialog'
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
      await runGitAction(folderPath, (path) => backend.git.branch.setUpstream(path, branchName, upstream))
    },
    onDone: () => (open = false)
  })

  let canSubmit = $derived(upstream.length > 0 && !submitState.busy)
</script>

<DialogRoot bind:open>
  {#if open}
    <DialogContent>
      <DialogHeader>
        <DialogTitle>Set Upstream</DialogTitle>
      </DialogHeader>

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

      <DialogFooter>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="accent" disabled={!canSubmit} onclick={submitState.submit}>
          {submitState.busy ? 'Saving…' : 'Set upstream'}
        </Button>
      </DialogFooter>
    </DialogContent>
  {/if}
</DialogRoot>
