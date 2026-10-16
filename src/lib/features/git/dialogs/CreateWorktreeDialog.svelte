<script lang="ts">
  import { untrack } from 'svelte'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import * as Dialog from '$lib/components/ui/dialog'
  import { Input, Select } from '$lib/components/ui/input'
  import { localHostLabel, selectFolder } from '$lib/features/browser/state.svelte'
  import { createWorktree } from '$lib/features/git/actions.svelte'
  import * as branches from '$lib/features/git/branches.svelte'
  import DialogError from '$lib/features/git/dialogs/DialogError.svelte'
  import { runDialogSubmit } from '$lib/features/git/dialogs/runDialogSubmit.svelte'
  import { FolderOpenIcon } from '$lib/icons/lucideExports'
  import type { GitWorktreeCheckout } from '$lib/types/backend'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { validateBranchName } from '$lib/utils/git/branchValidation'
  import { splitRemotePath } from '$lib/utils/paths'

  let {
    open = $bindable(false),
    folderPath,
    initialCheckout
  }: {
    open?: boolean
    folderPath: string
    initialCheckout?: GitWorktreeCheckout
  } = $props()

  // The parent mounts a fresh draft on each opening. Folder/tab changes must
  // never redirect an already-open draft or an awaited folder selection.
  const initiatingRootPath = untrack(() => folderPath)
  const initial = untrack(() => initialCheckout ?? ({ kind: 'new_branch', name: '', base: 'HEAD' } as const))
  const host = splitRemotePath(initiatingRootPath)?.server ?? localHostLabel()
  const refChoicesId = $props.id()

  let mode = $state<GitWorktreeCheckout['kind']>(initial.kind)
  let name = $state(initial.kind === 'new_branch' ? initial.name : '')
  let existingBranch = $state(initial.kind === 'existing_branch' ? initial.name : '')
  let base = $state(initial.kind === 'new_branch' ? initial.base : 'HEAD')
  let revision = $state(initial.kind === 'detached' ? initial.rev : 'HEAD')
  let destination = $state('')
  let destinationEdited = $state(false)
  let selectedRoot = $state<string | null>(null)
  let suggestionBusy = $state(false)
  let suggestionError = $state<string | null>(null)
  let browseBusy = $state(false)
  let browseError = $state<string | null>(null)
  let suggestionRefresh = $state(0)
  let suggestionGeneration = 0

  let entry = $derived(branches.byFolder.get(initiatingRootPath))
  let existingOptions = $derived(
    entry?.list
      .filter((branch) => branch.kind === 'local' && !branch.isCurrent && branch.worktreePath === null)
      .map((branch) => branch.name) ?? []
  )
  let refOptions = $derived([...new Set(['HEAD', ...(entry?.list.map((branch) => branch.name) ?? [])])])
  let checkout = $derived.by((): GitWorktreeCheckout => {
    if (mode === 'existing_branch') return { kind: 'existing_branch', name: existingBranch }
    if (mode === 'detached') return { kind: 'detached', rev: revision.trim() }
    return { kind: 'new_branch', name, base: base.trim() }
  })
  let nameError = $derived(mode === 'new_branch' && name.length > 0 ? validateBranchName(name) : null)
  let validCheckout = $derived(
    mode === 'existing_branch'
      ? existingOptions.includes(existingBranch)
      : mode === 'detached'
        ? revision.trim().length > 0
        : name.length > 0 && !nameError && base.trim().length > 0
  )

  const submitState = runDialogSubmit({
    run: async () => {
      const path = initiatingRootPath
      const target = destination
      const selection: GitWorktreeCheckout = { ...checkout }
      await createWorktree(path, target, selection, () => (open = false))
    },
    onDone: () => (open = false)
  })

  let blocked = $derived(submitState.busy || browseBusy)
  let canSubmit = $derived(
    validCheckout &&
      destination.trim().length > 0 &&
      !blocked &&
      !suggestionBusy &&
      (destinationEdited || !suggestionError)
  )

  $effect(() => {
    void branches.loadFor(initiatingRootPath)
  })

  $effect(() => {
    // Picking the same root again is still an explicit request for a fresh suggestion.
    void suggestionRefresh
    const selection = checkout
    const root = selectedRoot
    const generation = ++suggestionGeneration
    if (!open || destinationEdited || !validCheckout) {
      suggestionBusy = false
      suggestionError = null
      return
    }

    suggestionBusy = true
    suggestionError = null
    void backend.git
      .defaultWorktreePath(initiatingRootPath, selection, root)
      .then((path) => {
        if (generation === suggestionGeneration && open && !destinationEdited) destination = path
      })
      .catch((error: unknown) => {
        if (generation === suggestionGeneration && open && !destinationEdited) suggestionError = getErrorMessage(error)
      })
      .finally(() => {
        if (generation === suggestionGeneration) suggestionBusy = false
      })
    return () => {
      suggestionGeneration++
    }
  })

  async function browse() {
    if (blocked) return
    browseBusy = true
    browseError = null
    try {
      const root = await selectFolder({ path: initiatingRootPath })
      if (!open || root === null) return
      selectedRoot = root
      destinationEdited = false
      suggestionRefresh++
    } catch (error) {
      if (open) browseError = getErrorMessage(error)
    } finally {
      browseBusy = false
    }
  }

  function dismiss(nextOpen: boolean) {
    if (!blocked) open = nextOpen
  }
</script>

<Dialog.Root {open} onOpenChange={dismiss}>
  {#if open}
    <Dialog.Content
      escapeKeydownBehavior={blocked ? 'ignore' : 'close'}
      interactOutsideBehavior={blocked ? 'ignore' : 'close'}
      aria-busy={blocked}
    >
      <Dialog.Header>
        <Dialog.Title>Create Worktree</Dialog.Title>
        <Dialog.Description>Create an isolated checkout from this repository.</Dialog.Description>
      </Dialog.Header>

      <form
        onsubmit={(event) => {
          event.preventDefault()
          if (canSubmit) void submitState.submit()
        }}
      >
        <fieldset disabled={blocked} class="mt-2 space-y-3">
          <div class="space-y-1 text-xs text-muted">
            <div>Host: <span class="text-fg">{host}</span></div>
            <div class="truncate font-mono text-2xs text-subtle" title={initiatingRootPath}>{initiatingRootPath}</div>
          </div>

          <label class="block space-y-1">
            <span class="text-xs font-medium text-muted">Checkout</span>
            <Select bind:value={mode} class="text-sm">
              <option value="new_branch">New Branch</option>
              <option value="existing_branch">Existing Branch</option>
              <option value="detached">Commit (Detached)</option>
            </Select>
          </label>

          {#if mode === 'new_branch'}
            <label class="block space-y-1">
              <span class="text-xs font-medium text-muted">Branch Name</span>
              <Input
                bind:value={name}
                placeholder="feature/my-branch"
                class="font-mono text-sm"
                spellcheck={false}
                autofocus
              />
              <DialogError message={nameError} />
            </label>
            <label class="block space-y-1">
              <span class="text-xs font-medium text-muted">Based On</span>
              <Input
                bind:value={base}
                list={refChoicesId}
                placeholder="HEAD"
                class="font-mono text-sm"
                spellcheck={false}
              />
            </label>
          {:else if mode === 'existing_branch'}
            <label class="block space-y-1">
              <span class="text-xs font-medium text-muted">Branch</span>
              <Select bind:value={existingBranch} class="font-mono text-sm">
                <option value="" disabled>Select a local branch</option>
                {#each existingOptions as branch (branch)}
                  <option value={branch}>{branch}</option>
                {/each}
              </Select>
            </label>
            {#if existingOptions.length === 0}
              <p class="text-xs text-subtle">No unoccupied local branches available.</p>
            {:else}
              <p class="text-xs text-subtle">Branches already checked out in a worktree are excluded.</p>
            {/if}
          {:else}
            <label class="block space-y-1">
              <span class="text-xs font-medium text-muted">Commit or Ref</span>
              <Input
                bind:value={revision}
                list={refChoicesId}
                placeholder="HEAD"
                class="font-mono text-sm"
                spellcheck={false}
              />
            </label>
            <p class="text-xs text-subtle">Creates a detached HEAD without a branch.</p>
          {/if}

          <datalist id={refChoicesId}>
            {#each refOptions as ref (ref)}<option value={ref}></option>{/each}
          </datalist>

          <div class="space-y-1">
            <label for={`${refChoicesId}-destination`} class="text-xs font-medium text-muted">Destination</label>
            <div class="flex items-center gap-1.5">
              <Input
                id={`${refChoicesId}-destination`}
                bind:value={destination}
                oninput={() => (destinationEdited = true)}
                placeholder="Full destination path"
                class="min-w-0 flex-1 font-mono text-sm"
                spellcheck={false}
              />
              <Button type="button" size="sm" onclick={() => void browse()}>
                <FolderOpenIcon size={14} />
                {browseBusy ? 'Browsing…' : 'Browse…'}
              </Button>
            </div>
            {#if suggestionBusy}<p class="text-2xs text-subtle" role="status">Suggesting destination…</p>{/if}
            <p class="text-2xs text-subtle">Destination and copied files stay on {host}.</p>
            <DialogError message={suggestionError} />
            <DialogError message={browseError} />
          </div>

          <p class="text-xs text-muted">
            Tasks marked <code class="font-mono">worktreeCreated</code> run after files are copied.
          </p>
          <DialogError message={submitState.error} />
        </fieldset>

        <Dialog.Footer>
          <Button type="button" variant="ghost" disabled={blocked} onclick={() => dismiss(false)}>Cancel</Button>
          <Button type="submit" variant="accent" disabled={!canSubmit}>
            {submitState.busy ? 'Creating…' : 'Create Worktree'}
          </Button>
        </Dialog.Footer>
      </form>
    </Dialog.Content>
  {/if}
</Dialog.Root>
