<!--
  @component
  CreateBranchDialog: name + base ref + auto-checkout toggle.

  Validates the branch name client-side against a subset of
  `git check-ref-format` rules so obvious mistakes get inline error
  text. Server-side validation in `validated_ref_name` is the final
  authority, so any name git itself accepts but our regex misses
  still surfaces in the response.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { Checkbox } from '$lib/components/ui/checkbox'
  import {
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogRoot,
    DialogTitle
  } from '$lib/components/ui/dialog'
  import { Input, Select } from '$lib/components/ui/input'
  import * as branches from '$lib/features/git/branches.svelte'
  import { refreshGit } from '$lib/features/git/state.svelte'
  import { validateBranchName } from '$lib/utils/git/branchValidation'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'

  let {
    open = $bindable(false),
    projectId,
    projectPath,
    defaultBase
  }: {
    open?: boolean
    projectId: string
    projectPath: string
    defaultBase: string
  } = $props()

  function initialBase(): string {
    return defaultBase
  }

  let name = $state('')
  let base = $state(initialBase())
  let alsoCheckout = $state(true)

  const submitState = runDialogSubmit({
    run: async () => {
      await backend.git.branch.create(projectPath, name, base, { checkout: alsoCheckout })
      if (alsoCheckout) {
        branches.markRecent(projectId, name)
        await refreshGit(projectId, projectPath)
      }
      await branches.refresh(projectId, projectPath)
    },
    onDone: () => (open = false)
  })

  let entry = $derived(branches.byProject.get(projectId))
  let baseOptions = $derived.by(() => {
    if (!entry) return [] as string[]
    const out = new Set<string>()
    for (const b of entry.list) out.add(b.name)
    return [...out]
  })

  let nameError = $derived(name.length === 0 ? null : validateBranchName(name))
  let canSubmit = $derived(
    name.length > 0 && !nameError && base.length > 0 && !submitState.busy
  )
</script>

<DialogRoot bind:open>
  {#if open}
    <DialogContent>
      <DialogHeader>
        <DialogTitle>Create Branch</DialogTitle>
        <DialogDescription>Branch off an existing ref.</DialogDescription>
      </DialogHeader>

      <div class="mt-2 space-y-3">
        <label class="block space-y-1">
          <span class="text-xs font-medium text-muted">Name</span>
          <Input
            bind:value={name}
            placeholder="feature/my-branch"
            class="text-sm"
            spellcheck={false}
            autofocus
          />
          <DialogError message={nameError} />
        </label>

        <label class="block space-y-1">
          <span class="text-xs font-medium text-muted">Based on</span>
          <Select bind:value={base} class="text-sm">
            {#each baseOptions as option (option)}
              <option value={option}>{option}</option>
            {/each}
          </Select>
        </label>

        <label class="flex items-center gap-2 text-sm text-fg">
          <Checkbox bind:checked={alsoCheckout} />
          <span>Switch to it after creating</span>
        </label>

        <DialogError message={submitState.error} />
      </div>

      <DialogFooter>
        <Button variant="ghost" disabled={submitState.busy} onclick={() => (open = false)}>Cancel</Button>
        <Button variant="accent" disabled={!canSubmit} onclick={submitState.submit}>
          {submitState.busy ? 'Creating…' : 'Create'}
        </Button>
      </DialogFooter>
    </DialogContent>
  {/if}
</DialogRoot>
