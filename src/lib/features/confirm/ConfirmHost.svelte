<script lang="ts">
  import * as Dialog from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { getPendingConfirm, resolvePendingConfirm } from '$lib/features/confirm/service.svelte'

  const pending = $derived(getPendingConfirm())

  function dismissRequest(id: number) {
    return (open: boolean) => {
      if (!open) void resolvePendingConfirm(id, false)
    }
  }
</script>

{#if pending}
  <!-- Each request gets fresh open state; callbacks capture its id before a queue advance. -->
  {#key pending.id}
    <Dialog.Root open={true} onOpenChange={dismissRequest(pending.id)}>
      <Dialog.Content
        escapeKeydownBehavior={pending.busy ? 'ignore' : 'close'}
        interactOutsideBehavior={pending.busy ? 'ignore' : 'close'}
        aria-busy={pending.busy}
      >
        <Dialog.Title>{pending.title}</Dialog.Title>
        <Dialog.Description>{pending.message}</Dialog.Description>
        <Dialog.Footer>
          <Button variant="outline" disabled={pending.busy} onclick={resolvePendingConfirm.bind(null, pending.id, false)}>
            {pending.cancelLabel ?? 'Cancel'}
          </Button>
          <Button variant="accent" disabled={pending.busy} onclick={resolvePendingConfirm.bind(null, pending.id, true)}>
            {pending.confirmLabel ?? 'Confirm'}
          </Button>
        </Dialog.Footer>
      </Dialog.Content>
    </Dialog.Root>
  {/key}
{/if}
