<script lang="ts">
  import ConfirmDialog from '$lib/components/dialogs/ConfirmDialog.svelte'
  import { getPendingConfirm, resolvePendingConfirm } from '$lib/features/confirm/service.svelte'

  const pending = $derived(getPendingConfirm())
</script>

{#if pending}
  <!-- {#key} forces a fresh ConfirmDialog per request so Bits UI's internal
       open-state can't get wedged between back-to-back confirms. -->
  {#key pending.id}
    <ConfirmDialog
      open={true}
      title={pending.title}
      message={pending.message}
      confirmLabel={pending.confirmLabel ?? 'Confirm'}
      cancelLabel={pending.cancelLabel ?? 'Cancel'}
      onConfirm={() => resolvePendingConfirm(true)}
      onCancel={() => resolvePendingConfirm(false)}
    />
  {/key}
{/if}
