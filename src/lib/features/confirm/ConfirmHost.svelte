<script lang="ts">
  import { DialogRoot, DialogContent, DialogTitle, DialogDescription, DialogFooter } from '$lib/components/ui/dialog'
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
    <DialogRoot open={true} onOpenChange={dismissRequest(pending.id)}>
      <DialogContent
        escapeKeydownBehavior={pending.busy ? 'ignore' : 'close'}
        interactOutsideBehavior={pending.busy ? 'ignore' : 'close'}
        aria-busy={pending.busy}
      >
        <DialogTitle>{pending.title}</DialogTitle>
        <DialogDescription>{pending.message}</DialogDescription>
        <DialogFooter>
          <Button variant="outline" disabled={pending.busy} onclick={resolvePendingConfirm.bind(null, pending.id, false)}>
            {pending.cancelLabel ?? 'Cancel'}
          </Button>
          <Button variant="accent" disabled={pending.busy} onclick={resolvePendingConfirm.bind(null, pending.id, true)}>
            {pending.confirmLabel ?? 'Confirm'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </DialogRoot>
  {/key}
{/if}
