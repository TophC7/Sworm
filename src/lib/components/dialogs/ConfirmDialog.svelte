<script lang="ts">
  import { DialogRoot, DialogContent, DialogTitle, DialogDescription, DialogFooter } from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'

  let {
    open = false,
    title,
    message,
    confirmLabel = 'Confirm',
    cancelLabel = 'Cancel',
    showCancel = true,
    onConfirm,
    onCancel
  }: {
    open?: boolean
    title: string
    message: string
    confirmLabel?: string
    cancelLabel?: string
    showCancel?: boolean
    onConfirm?: () => void
    onCancel?: () => void
  } = $props()
</script>

<DialogRoot
  bind:open
  onOpenChange={(v) => {
    if (!v) onCancel?.()
  }}
>
  <DialogContent>
    <DialogTitle>{title}</DialogTitle>
    <DialogDescription>{message}</DialogDescription>
    <DialogFooter>
      {#if showCancel}
        <Button variant="outline" onclick={onCancel}>{cancelLabel}</Button>
      {/if}
      <Button variant="accent" onclick={onConfirm}>{confirmLabel}</Button>
    </DialogFooter>
  </DialogContent>
</DialogRoot>
