<!--
  @component
  CompareBranchModal: file list for a branch against HEAD.
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
  import { openTextSnapshot } from '$lib/features/workbench/surfaces/text/service.svelte'
  import * as branches from '$lib/features/git/branches.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import type { FileDiff } from '$lib/types/backend'

  let {
    open = $bindable(false),
    projectId,
    projectPath,
    branchName
  }: {
    open?: boolean
    projectId: string
    projectPath: string
    branchName: string
  } = $props()

  let files = $state<FileDiff[]>([])
  let loading = $state(false)
  let error = $state<string | null>(null)

  $effect(() => {
    if (!open) return
    void load()
  })

  async function load() {
    loading = true
    error = null
    try {
      files = await backend.git.branch.diffAgainstHead(projectPath, branchName)
    } catch (e) {
      error = getErrorMessage(e)
      files = []
    } finally {
      loading = false
    }
  }

  async function openAtBranch(filePath: string) {
    // git_show_file only accepts hex commit hashes or `stash@{N}`,
    // so resolve the branch to its tip hash before opening; keep the
    // branch name as the user-visible ref label.
    const entry = branches.byProject.get(projectId)
    const summary = entry?.list.find((b) => b.name === branchName)
    const hash = summary?.tip.hash
    if (!hash) {
      console.error('Cannot resolve tip hash for', branchName)
      return
    }
    try {
      await openTextSnapshot(projectId, filePath, hash, branchName)
    } catch (e) {
      console.error('Failed to open snapshot:', e)
    }
  }
</script>

<DialogRoot bind:open>
  {#if open}
    <DialogContent class="max-w-[640px]">
      <DialogHeader>
        <DialogTitle>Compare {branchName} with HEAD</DialogTitle>
      </DialogHeader>

      <div class="mt-2 max-h-[420px] overflow-y-auto rounded border border-edge bg-overlay">
        {#if loading}
          <div class="px-3 py-2 text-sm text-subtle">Loading file list…</div>
        {:else if error}
          <div class="px-3 py-2 text-sm text-danger">{error}</div>
        {:else if files.length === 0}
          <div class="px-3 py-2 text-sm text-subtle">No differences.</div>
        {:else}
          <ul class="divide-y divide-edge/50">
            {#each files as file (file.path)}
              <li>
                <button
                  type="button"
                  class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-raised focus-visible:shadow-focus-ring focus-visible:outline-none"
                  onclick={() => openAtBranch(file.path)}
                >
                  <GitStatusBadge status={file.status} />
                  <span class="min-w-0 flex-1 truncate font-mono text-xs">{file.path}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <DialogFooter>
        <Button variant="ghost" onclick={() => (open = false)}>Close</Button>
      </DialogFooter>
    </DialogContent>
  {/if}
</DialogRoot>
