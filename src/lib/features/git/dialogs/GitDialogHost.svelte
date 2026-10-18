<script lang="ts">
  import CreateBranchDialog from './CreateBranchDialog.svelte'
  import CreateWorktreeDialog from './CreateWorktreeDialog.svelte'
  import DeleteBranchDialog from './DeleteBranchDialog.svelte'
  import MergeConfirmDialog from './MergeConfirmDialog.svelte'
  import RenameBranchDialog from './RenameBranchDialog.svelte'
  import SetUpstreamDialog from './SetUpstreamDialog.svelte'
  import { closeGitDialog, getGitDialog } from './host.svelte'

  const dialog = $derived(getGitDialog())
</script>

{#if dialog}
  {#key dialog.id}
    {@const id = dialog.id}
    {#if dialog.request.kind === 'create_branch'}
      {@const request = dialog.request}
      <CreateBranchDialog
        bind:open={
          () => true,
          (open) => {
            if (!open) closeGitDialog(id)
          }
        }
        folderPath={request.folderPath}
        defaultBase={request.base}
      />
    {:else if dialog.request.kind === 'create_worktree'}
      {@const request = dialog.request}
      <CreateWorktreeDialog
        bind:open={
          () => true,
          (open) => {
            if (!open) closeGitDialog(id)
          }
        }
        folderPath={request.folderPath}
        initialCheckout={request.checkout}
      />
    {:else if dialog.request.kind === 'rename_branch'}
      {@const request = dialog.request}
      <RenameBranchDialog
        bind:open={
          () => true,
          (open) => {
            if (!open) closeGitDialog(id)
          }
        }
        folderPath={request.folderPath}
        oldName={request.branch}
      />
    {:else if dialog.request.kind === 'delete_branch'}
      {@const request = dialog.request}
      <DeleteBranchDialog
        bind:open={
          () => true,
          (open) => {
            if (!open) closeGitDialog(id)
          }
        }
        folderPath={request.folderPath}
        branch={request.branch}
      />
    {:else if dialog.request.kind === 'set_upstream'}
      {@const request = dialog.request}
      <SetUpstreamDialog
        bind:open={
          () => true,
          (open) => {
            if (!open) closeGitDialog(id)
          }
        }
        folderPath={request.folderPath}
        branchName={request.branch}
        initialUpstream={request.initialUpstream}
      />
    {:else if dialog.request.kind === 'merge'}
      {@const request = dialog.request}
      <MergeConfirmDialog
        bind:open={
          () => true,
          (open) => {
            if (!open) closeGitDialog(id)
          }
        }
        folderPath={request.folderPath}
        source={request.source}
        current={request.current}
      />
    {/if}
  {/key}
{/if}
